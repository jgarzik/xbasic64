//! Code generator - emits x86-64 assembly from AST
//!
//! # Architecture Overview
//!
//! This module translates the BASIC AST directly to x86-64 assembly using Intel syntax.
//! There is no intermediate representation - we generate assembly in a single pass over
//! the AST (after a preliminary pass to collect DATA statements).
//!
//! The generated code follows the System V AMD64 ABI for compatibility with libc functions.
//! Output is assembled with the system assembler (`as`) and linked with `cc`.
//!
//! # Register Conventions
//!
//! We use a simple convention for where computed values live:
//!
//! | Type           | Primary Register | Notes                              |
//! |----------------|------------------|------------------------------------|
//! | Integer (i16)  | `eax` (low 16)   | Stored as i16, computed as i32     |
//! | Long (i32)     | `eax`            | 32-bit signed integer              |
//! | Single (f32)   | `xmm0`           | 32-bit float, uses SSE             |
//! | Double (f64)   | `xmm0`           | 64-bit float, uses SSE             |
//! | String         | `rax` + `rdx`    | (pointer, length) pair             |
//!
//! For binary operations, after evaluating both operands:
//! - Left operand is restored to `eax`/`xmm0`
//! - Right operand is placed in `ecx`/`xmm1`
//!
//! # Storage Model
//!
//! Storage is addressed uniformly as 8-byte words through [`Loc`], which is
//! either a `.bss` symbol or an `rbp`-relative frame offset. Callers ask for
//! "word N of this variable" and never compute offsets themselves.
//!
//! **Module-level variables and arrays live in `.bss`** (`_var_<NAME>`,
//! `_arr_<NAME>`, RIP-relative). This gives three things at once: the loader
//! zero-fills them, so an unassigned variable reads as `0` / `""` as BASIC
//! requires; a symbol is frame-independent, so a `SUB` and the main program
//! refer to the same storage; and access costs exactly what a frame reference
//! did.
//!
//! **Procedure locals live on the stack** and are zeroed by the prologue (see
//! `emit_zero_frame`), so each invocation starts clean and recursion works.
//!
//! ```text
//! High addresses
//! ┌─────────────────────┐
//! │   Return address    │  ← pushed by `call`, rsp+8 on entry
//! ├─────────────────────┤
//! │   Saved rbp         │  ← push rbp; mov rbp, rsp
//! ├─────────────────────┤  ← rbp points here
//! │   Local var 1       │  [rbp - 8]
//! │   Local var 2       │  [rbp - 16]
//! │   ...               │
//! │   Local var N       │  [rbp - N*8]
//! ├─────────────────────┤  ← rsp after prologue
//! │   Temp space        │  (for expression evaluation)
//! └─────────────────────┘
//! Low addresses
//! ```
//!
//! Word layout within a variable's storage:
//!
//! | kind           | word 0          | word 1      | word 1+i      |
//! |----------------|-----------------|-------------|---------------|
//! | numeric scalar | value           | --          | --            |
//! | string scalar  | pointer         | length      | --            |
//! | array          | element pointer | dim 0 bound | dim *i* bound |
//!
//! Numeric scalars occupy one 8-byte word regardless of declared type; the
//! narrower types are stored and loaded with narrower operands within it.
//!
//! Scoping rules follow LANGREF: a name used anywhere at module level is
//! global and shared with every procedure; parameters and a FUNCTION's return
//! pseudo-variable are always local and shadow a global of the same name; a
//! name used only inside a procedure is local to it.
//!
//! A record declared with `TYPE` is laid out as consecutive words too, so a
//! field is reached exactly the way a variable is -- a base [`Loc`] plus a word
//! offset. Only an array element, whose address is not known until run time,
//! needs a separate indirect path.
//!
//! # Private `_proc_*` calling convention
//!
//! Calls to user procedures do not follow the platform ABI, since the symbols
//! are private to the compiled program. `classify_params` is the single
//! definition, called by both the call site and the prologue:
//!
//! - Numeric arguments travel as f64 bit patterns in *integer* registers, and
//!   the callee narrows them to the declared type.
//! - A string occupies two slots, pointer then length.
//! - A record is passed as a pointer to the caller's copy, which the prologue
//!   copies into a local slot, giving by-value semantics.
//! - A parameter is never split between registers and the stack, and spilling
//!   is monotone: once one parameter goes to the stack, so do all later ones.
//! - Stack arguments sit at `[rbp + 16 + 8i]`, with no Win64 shadow space.
//!
//! # Semantic analysis
//!
//! Code generation assumes `sema` has already run and accepted the program: it
//! consults [`Symbols`] rather than guessing what a name means, and the
//! remaining `expect`/`unreachable!` sites are invariants sema establishes, not
//! reachable failure modes.
//!
//! # Stack Alignment (Critical for ABI Compliance)
//!
//! The System V AMD64 ABI requires 16-byte stack alignment before `call` instructions.
//! We maintain this invariant:
//!
//! 1. **Function entry**: After `call` pushes return address, `rsp % 16 == 8`
//! 2. **After `push rbp`**: `rsp % 16 == 0`
//! 3. **Prologue `sub rsp, N`**: N is rounded up to multiple of 16
//! 4. **Temporaries**: We use `sub rsp, 16` / `add rsp, 16` (never 8-byte pushes for temps)
//!
//! This ensures the stack is always 16-byte aligned before any `call` instruction,
//! which is required for SSE operations and varargs functions like `printf`.
//!
//! # Type Coercion
//!
//! BASIC performs automatic type promotion following this hierarchy:
//!
//! ```text
//! Integer (%) → Long (&) → Single (!) → Double (#)
//! ```
//!
//! Binary operations promote both operands to a common type. Special rules:
//! - `/` (division) always produces Double
//! - `\` (integer division) always produces Long
//! - `^` (power) always produces Double (uses libm `pow`)
//! - Comparisons return Long (-1 for true, 0 for false)
//!
//! Coercion instructions:
//! - Int→Float: `cvtsi2sd xmm0, eax` (or `cvtsi2ss` for Single)
//! - Float→Int: `cvttsd2si eax, xmm0` (truncation) or `cvtsd2si` (rounding)
//! - Single↔Double: `cvtss2sd` / `cvtsd2ss`
//!
//! # String Representation
//!
//! Strings are represented as (pointer, length) pairs, NOT null-terminated internally.
//! This allows efficient substring operations without copying.
//!
//! - String values: `rax` = pointer to characters, `rdx` = length
//! - String variables: two words of the variable's own storage -- word 0 is the
//!   pointer, word 1 the length. Both are reserved when the variable is first
//!   seen, so the length can never land in a neighbouring variable's slot.
//! - An unassigned string is `(NULL, 0)`, which the runtime's print helpers
//!   treat as empty rather than dereferencing.
//!
//! String literals are emitted in the `.data` section with labels `_str_N`.
//!
//! # Calling Convention (System V AMD64)
//!
//! For calling libc and runtime functions:
//!
//! | Argument # | Integer/Pointer | Float      |
//! |------------|-----------------|------------|
//! | 1          | `rdi`           | `xmm0`     |
//! | 2          | `rsi`           | `xmm1`     |
//! | 3          | `rdx`           | `xmm2`     |
//! | 4          | `rcx`           | `xmm3`     |
//! | 5          | `r8`            | `xmm4`     |
//! | 6          | `r9`            | `xmm5`     |
//!
//! Return values: integers in `rax`, floats in `xmm0`.
//!
//! **Caller-saved** (may be clobbered by calls): `rax`, `rcx`, `rdx`, `rsi`, `rdi`,
//! `r8`-`r11`, `xmm0`-`xmm15`
//!
//! **Callee-saved** (preserved across calls): `rbx`, `rbp`, `r12`-`r15`
//!
//! # Expression Evaluation Pattern
//!
//! Binary expressions follow this pattern to handle nested subexpressions safely:
//!
//! ```asm
//! ; Evaluate left operand → result in eax/xmm0
//! ; Save to stack (16-byte aligned temp):
//!     sub rsp, 16
//!     mov [rsp], eax          ; or movsd [rsp], xmm0
//! ; Evaluate right operand → result in eax/xmm0
//! ; Move right to secondary, restore left:
//!     mov ecx, eax            ; right operand
//!     mov eax, [rsp]          ; left operand
//!     add rsp, 16
//! ; Perform operation:
//!     add eax, ecx            ; result in eax
//! ```
//!
//! The 16-byte temp allocation (not 8) is critical: it maintains the 16-byte
//! alignment invariant in case evaluating the right operand involves function calls.
//!
//! # Runtime Library
//!
//! The compiler embeds a runtime library (from `src/runtime/*.s`) that provides:
//! - `_rt_print_*`: Output functions
//! - `_rt_input_*`: Input functions
//! - `_rt_*` string functions: `_rt_left`, `_rt_mid`, `_rt_right`, `_rt_instr`, etc.
//! - `_rt_*` math functions: `_rt_rnd`, `_rt_timer`, etc.
//! - `_rt_read_*`: DATA/READ support
//! - `_rt_file_*`: File I/O
//!
//! These use the same calling convention and are linked into the final executable.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::abi::{Abi, PlatformAbi};
use crate::parser::TypeRef;
use crate::parser::*;
use crate::sema::{Scope as SemaScope, Symbols};
use crate::using;
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

/// Simple math functions: BASIC name -> libc function name
static LIBC_MATH_FNS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("SIN", "sin"),
        ("COS", "cos"),
        ("TAN", "tan"),
        ("ATN", "atan"),
        ("EXP", "exp"),
        ("LOG", "log"),
    ])
});

/// Inline math functions: BASIC name -> x86-64 instruction
static INLINE_MATH_FNS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("SQR", "sqrtsd xmm0, xmm0"),
        ("INT", "roundsd xmm0, xmm0, 1"),
        ("FIX", "roundsd xmm0, xmm0, 3"),
    ])
});

/// Symbol prefix from platform ABI (underscore on macOS, empty on Linux/Windows)
const PREFIX: &str = PlatformAbi::SYMBOL_PREFIX;

/// Win64 ABI requires 32 bytes of shadow space before each call
#[cfg(windows)]
const WIN64_SHADOW_SPACE: i32 = 32;

/// Win64: stack space for calls with 5 args (shadow + 5th arg + alignment)
#[cfg(windows)]
const WIN64_5ARG_STACK_SPACE: i32 = 48;

/// Win64: offset to 5th argument on stack (after shadow space)
#[cfg(windows)]
const WIN64_5TH_ARG_OFFSET: i32 = 32;

/// Stack space for temporary values (must be 16-byte aligned)
const STACK_TEMP_SPACE: i32 = 16;

/// Maximum expression nesting depth before warning (each level uses 16 bytes of stack)
const MAX_EXPR_DEPTH: u32 = 256;

/// GOSUB stack size in bytes (64K entries * 8 bytes = 512KB)
const GOSUB_STACK_SIZE: i32 = 524288;

/// ASCII character codes
const ASCII_TAB: i64 = 9;
const ASCII_COMMA: i64 = 44;
const ASCII_QUOTE: i64 = 34;

fn is_string_var(name: &str) -> bool {
    name.ends_with('$')
}

/// Map a BASIC identifier to an assembler-safe symbol.
///
/// BASIC names keep their type suffixes (`A$`, `N%`), which are not valid
/// symbol characters. The mapping is injective because `_` escapes itself, so
/// distinct BASIC names cannot collide: `A_S` becomes `A__S` while `A$`
/// becomes `A_S`.
fn mangle(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    for c in name.chars() {
        match c {
            '_' => out.push_str("__"),
            '%' => out.push_str("_I"),
            '&' => out.push_str("_L"),
            '!' => out.push_str("_F"),
            '#' => out.push_str("_D"),
            '$' => out.push_str("_S"),
            c => out.push(c),
        }
    }
    out
}

/// Where a variable or array descriptor lives.
///
/// Storage is addressed uniformly as a sequence of 8-byte words, so callers ask
/// for "word N of this variable" and never do offset arithmetic themselves.
/// Word layout by kind:
///
/// | kind            | word 0            | word 1        | word 1+i          |
/// |-----------------|-------------------|---------------|-------------------|
/// | numeric scalar  | value             | --            | --                |
/// | string scalar   | pointer           | length        | --                |
/// | array           | element pointer   | dim 0 bound   | dim *i* bound     |
///
/// Today every `Loc` is a `Frame`; the `Global` variant exists so that moving
/// module-level storage to `.bss` is a change of location, not of every access
/// site.
#[derive(Clone, Debug, PartialEq)]
enum Loc {
    /// RIP-relative, at assembler symbol `sym`.
    Global(String),
    /// rbp-relative address of word 0 (negative, since the frame grows down).
    Frame(i32),
}

impl Loc {
    /// Operand for word `n`, with an explicit operand size.
    fn at(&self, size: &str, n: i32) -> String {
        match self {
            Loc::Global(sym) => format!("{} [rip + {} + {}]", size, sym, n * 8),
            Loc::Frame(off) => format!("{} [rbp + {}]", size, off + n * 8),
        }
    }

    /// Operand for word `n` as a QWORD, the common case.
    fn q(&self, n: i32) -> String {
        self.at("QWORD PTR", n)
    }

    /// A location `n` words further along, used to reach a record field.
    fn offset_words(&self, n: i32) -> Loc {
        match self {
            Loc::Global(sym) => Loc::Global(format!("{} + {}", sym, n * 8)),
            Loc::Frame(off) => Loc::Frame(off + n * 8),
        }
    }
}

/// A runtime error the generated code can raise.
///
/// Each variant names a message constant in the runtime. The BASIC line number
/// is passed separately at the call site, so the set of messages stays small no
/// matter how many check sites exist.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum RtError {
    Subscript,
    DivideByZero,
    Domain,
    Overflow,
    Undim,
    OutOfMemory,
}

impl RtError {
    /// Assembler symbol holding the message text.
    fn symbol(self) -> &'static str {
        match self {
            RtError::Subscript => "_err_subscript",
            RtError::DivideByZero => "_err_div0",
            RtError::Domain => "_err_domain",
            RtError::Overflow => "_err_overflow",
            RtError::Undim => "_err_undim",
            RtError::OutOfMemory => "_err_memory",
        }
    }

    /// Short tag used to build a unique trampoline label.
    fn tag(self) -> &'static str {
        match self {
            RtError::Subscript => "sub",
            RtError::DivideByZero => "div0",
            RtError::Domain => "dom",
            RtError::Overflow => "ovf",
            RtError::Undim => "undim",
            RtError::OutOfMemory => "mem",
        }
    }
}

/// Code generation options.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Emit runtime safety checks (array bounds, division by zero, ...).
    pub checks: bool,
}

impl Default for Options {
    fn default() -> Self {
        // Checks are on unless the user explicitly opts out.
        Options { checks: true }
    }
}

/// A file number, ready to be placed in an argument register.
enum FileNum {
    /// A literal, placed directly as an immediate.
    Imm(i64),
    /// An evaluated expression, parked in a frame slot.
    Slot(Loc),
}

/// One 8-byte physical argument slot in the private `_proc_*` convention.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Slot {
    /// `INT_ARG_REGS[i]`.
    Reg(usize),
    /// Caller: `[rsp + 8*i]` at the moment of the `call`.
    /// Callee: `[rbp + 16 + 8*i]` after `push rbp; mov rbp, rsp`.
    Stk(usize),
}

/// Where one parameter's words are passed.
#[derive(Clone, Debug)]
struct ParamPlace {
    ty: DataType,
    ptr: Slot,
    /// Second slot, for a string's length.
    len: Option<Slot>,
}

/// Variable storage information
#[derive(Clone)]
struct VarInfo {
    loc: Loc,
    data_type: DataType,
}

/// Metadata for array storage
#[derive(Clone)]
struct ArrayInfo {
    /// Word 0 holds the malloc'd element pointer; word 1+i holds dimension i's
    /// element count (already the declared bound + 1).
    loc: Loc,
    ndims: usize,
}

#[derive(Default)]
pub struct CodeGen {
    output: String,
    vars: HashMap<String, VarInfo>, // variable name -> variable info
    arrays: HashMap<String, ArrayInfo>, // array name -> array metadata
    stack_offset: i32,              // current stack offset
    label_counter: u32,             // for generating unique labels
    string_literals: Vec<String>,   // string constants
    data_items: Vec<Literal>,       // DATA values
    current_proc: Option<String>,   // current SUB/FUNCTION name
    proc_vars: HashMap<String, VarInfo>, // local variables for current proc
    proc_arrays: HashMap<String, ArrayInfo>, // arrays DIM'd inside the current proc
    /// Names resolved by semantic analysis; codegen consults this instead of
    /// guessing from an identifier's spelling.
    symbols: Symbols,
    /// Code generation options (currently just whether checks are emitted).
    opts: Options,
    /// BASIC line of the statement being compiled, for runtime diagnostics.
    current_line: u32,
    /// Error trampolines needed so far, keyed by (error, line) so that sites
    /// sharing both share one trampoline. Ordered for reproducible output.
    error_sites: BTreeMap<(RtError, u32), String>,
    /// Enclosing loops, innermost last, so EXIT FOR / EXIT DO know where to
    /// jump. Each entry is (is_for, end label).
    loop_stack: Vec<(bool, String)>,
    /// Label of the current procedure's epilogue, for EXIT SUB / EXIT FUNCTION.
    proc_exit_label: Option<String>,
    /// Storage for module-level variables declared with `DIM ... AS`.
    record_vars: HashMap<String, Loc>,
    /// The same, for the current procedure's typed locals and parameters.
    /// Kept separate from `record_vars` and cleared per procedure: a shared
    /// map let one procedure's frame slot be reused by the next, which then
    /// read and wrote below its own `rsp`.
    proc_record_vars: HashMap<String, Loc>,
    /// Declared types of the current procedure's typed parameters.
    proc_types: HashMap<String, TypeRef>,
    /// Module-level typed variables and their size in words, for .bss.
    record_globals: BTreeMap<String, i32>,
    /// Element size in words for module-level arrays of records, by
    /// upper-case name.
    record_elem_words: HashMap<String, i32>,
    /// The same, for arrays declared inside the current procedure.
    proc_record_elem_words: HashMap<String, i32>,
    /// DATA item index reached at each numeric line label, for RESTORE.
    data_line_index: HashMap<u32, usize>,
    /// The same for named labels.
    data_label_index: HashMap<String, usize>,
    gosub_used: bool, // whether GOSUB is used (need return stack)
    expr_depth: u32,  // current expression nesting depth
}

impl CodeGen {
    fn emit(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    /// Get the integer argument register for a given argument position (0-based)
    fn arg_reg(n: usize) -> &'static str {
        PlatformAbi::INT_ARG_REGS
            .get(n)
            .expect("argument index out of bounds")
    }

    /// Emit a mov instruction to set up an integer argument from a register
    fn emit_arg_reg(&mut self, arg_n: usize, src_reg: &str) {
        let dst = Self::arg_reg(arg_n);
        if dst != src_reg {
            self.emit(&format!("    mov {}, {}", dst, src_reg));
        }
    }

    /// Emit a mov instruction to set up an integer argument from an immediate
    fn emit_arg_imm(&mut self, arg_n: usize, value: i64) {
        let dst = Self::arg_reg(arg_n);
        self.emit(&format!("    mov {}, {}", dst, value));
    }

    /// Emit a lea instruction to set up an integer argument from a memory reference
    fn emit_arg_lea(&mut self, arg_n: usize, mem: &str) {
        let dst = Self::arg_reg(arg_n);
        self.emit(&format!("    lea {}, {}", dst, mem));
    }

    /// Call a libc function with proper shadow space on Win64
    fn emit_call_libc(&mut self, func: &str) {
        #[cfg(windows)]
        {
            self.emit(&format!("    sub rsp, {}", WIN64_SHADOW_SPACE));
            self.emit(&format!("    call {}{}", PREFIX, func));
            self.emit(&format!("    add rsp, {}", WIN64_SHADOW_SPACE));
        }
        #[cfg(not(windows))]
        {
            self.emit(&format!("    call {}{}", PREFIX, func));
        }
    }

    /// Emit type-specific instruction for binary operations
    fn emit_typed(
        &mut self,
        work_type: DataType,
        int_instr: &str,
        single_instr: &str,
        double_instr: &str,
    ) {
        match work_type {
            DataType::Integer | DataType::Long => self.emit(int_instr),
            DataType::Single => self.emit(single_instr),
            _ => self.emit(double_instr),
        }
    }

    /// Convert float operands to integers (truncate). Used for IntDiv, Mod, logical ops.
    fn emit_cvt_float_to_int(&mut self, work_type: DataType) {
        if !work_type.is_integer() {
            self.emit_typed(
                work_type,
                "",
                "    cvttss2si eax, xmm0",
                "    cvttsd2si eax, xmm0",
            );
            self.emit_typed(
                work_type,
                "",
                "    cvttss2si ecx, xmm1",
                "    cvttsd2si ecx, xmm1",
            );
        }
    }

    /// Convert integer/single operands to double. Used for Div, Pow.
    fn emit_cvt_to_double(&mut self, work_type: DataType) {
        match work_type {
            DataType::Integer | DataType::Long => {
                self.emit("    cvtsi2sd xmm0, eax");
                self.emit("    cvtsi2sd xmm1, ecx");
            }
            DataType::Single => {
                self.emit("    cvtss2sd xmm0, xmm0");
                self.emit("    cvtss2sd xmm1, xmm1");
            }
            _ => {}
        }
    }

    fn emit_label(&mut self, label: &str) {
        self.output.push_str(label);
        self.output.push_str(":\n");
    }

    fn new_label(&mut self, prefix: &str) -> String {
        let label = format!(".L{}_{}", prefix, self.label_counter);
        self.label_counter += 1;
        label
    }

    fn add_string_literal(&mut self, s: &str) -> usize {
        let idx = self.string_literals.len();
        self.string_literals.push(s.to_string());
        idx
    }

    /// Get variable info, allocating if necessary
    fn get_var_info(&mut self, name: &str) -> VarInfo {
        if self.current_proc.is_some() {
            // Check local variables first
            if let Some(info) = self.proc_vars.get(name) {
                return info.clone();
            }
        }

        if let Some(info) = self.vars.get(name) {
            return info.clone();
        }

        // Allocate new variable - determine type from suffix.
        // A string needs two words (pointer, length); everything else one.
        // Reserving both here, once, is what keeps the length word inside the
        // variable's own storage: it used to be scavenged at `offset - 8`,
        // which is the *next* variable's slot unless something else happened to
        // reserve it first.
        let data_type = DataType::from_suffix(name);
        let words = Self::words_for(data_type);

        if self.current_proc.is_some() {
            // Not a module-level name, so it is local to this procedure: a
            // fresh, zeroed slot per invocation, which is what makes recursion
            // work.
            self.stack_offset -= 8 * words;
            let info = VarInfo {
                loc: Loc::Frame(self.stack_offset),
                data_type,
            };
            self.proc_vars.insert(name.to_string(), info.clone());
            info
        } else {
            // Module-level: static storage, shared with every procedure and
            // zero-initialized by the loader.
            let info = VarInfo {
                loc: Loc::Global(format!("_var_{}", mangle(name))),
                data_type,
            };
            self.vars.insert(name.to_string(), info.clone());
            info
        }
    }

    /// Find an array, preferring one DIM'd in the current procedure over a
    /// module-level array of the same name.
    fn lookup_array(&self, name: &str) -> Option<&ArrayInfo> {
        if self.current_proc.is_some() {
            if let Some(info) = self.proc_arrays.get(name) {
                return Some(info);
            }
        }
        self.arrays.get(name)
    }

    /// Value representation of a parameter: its `AS` clause when it has one,
    /// otherwise the type its name's suffix implies.
    ///
    /// A record parameter is passed as a pointer to the caller's copy, which
    /// the prologue then copies into a local slot, so it arrives by value like
    /// every other parameter without needing a new addressing mode.
    fn param_data_type(p: &Param) -> DataType {
        match &p.ty {
            Some(TypeRef::Record(_)) => DataType::Long, // a pointer
            Some(t) => Self::data_type_of(t),
            None => DataType::from_suffix(&p.name),
        }
    }

    /// Assign physical slots to a procedure's parameters.
    ///
    /// This is the single definition of the private `_proc_*` calling
    /// convention. Both the call site and the procedure prologue call it with
    /// the same input -- the declared parameter types, in order -- so the two
    /// sides cannot drift apart. They previously open-coded their own slot
    /// arithmetic and disagreed about strings: a string argument occupies two
    /// slots (pointer and length) but the callee bound only one register per
    /// parameter, so the string arrived empty and every parameter after it was
    /// read from the wrong register.
    ///
    /// Two rules keep the assignment simple enough to be obviously the same on
    /// both sides:
    ///
    /// * A parameter is never split between registers and the stack. Splitting
    ///   would buy at most one register, and `_proc_*` is a private symbol with
    ///   no external ABI obligation.
    /// * Spilling is monotone: once one parameter goes to the stack, so do all
    ///   later ones, which keeps stack slot indices contiguous in declaration
    ///   order.
    ///
    /// Returns the placements and the number of stack slots needed.
    fn classify_params(types: &[DataType]) -> (Vec<ParamPlace>, usize) {
        let n_regs = PlatformAbi::INT_ARG_REGS.len();
        let mut next_reg = 0usize;
        let mut next_stk = 0usize;
        let mut places = Vec::with_capacity(types.len());

        for &ty in types {
            let words = Self::words_for(ty) as usize;
            // If it does not fit entirely in the remaining registers, this
            // parameter and every later one go on the stack.
            if next_reg + words > n_regs {
                next_reg = n_regs;
            }
            let in_regs = next_reg < n_regs;

            let take = |counter: &mut usize| -> Slot {
                let slot = if in_regs {
                    Slot::Reg(*counter)
                } else {
                    Slot::Stk(*counter)
                };
                *counter += 1;
                slot
            };
            let counter = if in_regs {
                &mut next_reg
            } else {
                &mut next_stk
            };

            let ptr = take(counter);
            let len = if words == 2 {
                Some(take(counter))
            } else {
                None
            };
            places.push(ParamPlace { ty, ptr, len });
        }

        (places, next_stk)
    }

    /// Size in bytes of one element of an array, from the name's type suffix.
    ///
    /// Elements are stored at their declared width, the same way scalars are.
    /// Storing every numeric element as f64 regardless -- as this used to --
    /// meant `A%(0)` wrote 8 bytes of double but was *read* as an integer,
    /// because expr_type types an array access by its suffix. Typed numeric
    /// arrays returned garbage as a result.
    fn elem_size_for(&self, name: &str) -> i32 {
        // An array declared with `DIM ... AS T` has records for elements, so
        // its element size comes from the type rather than a name suffix.
        if let Some(words) = self.record_elem_words_of(name) {
            return words * 8;
        }
        Self::elem_size(name)
    }

    fn elem_size(name: &str) -> i32 {
        match DataType::from_suffix(name) {
            DataType::String => 16, // pointer + length
            DataType::Integer => 2,
            DataType::Long | DataType::Single => 4,
            DataType::Double => 8,
        }
    }

    /// Number of 8-byte words a scalar of this type occupies.
    fn words_for(data_type: DataType) -> i32 {
        if data_type == DataType::String { 2 } else { 1 }
    }

    /// Get just the storage location for a variable (convenience method)
    fn get_var_loc(&mut self, name: &str) -> Loc {
        self.get_var_info(name).loc
    }

    /// Determine the result type of an expression
    fn expr_type(&self, expr: &Expr) -> DataType {
        match expr {
            Expr::Literal(lit) => match lit {
                Literal::Integer(_) => DataType::Long, // Integer literals are Long
                Literal::Float(_) => DataType::Double,
                Literal::String(_) => DataType::String,
            },
            Expr::Variable(name) => match self.symbols.consts.get(&name.to_uppercase()) {
                Some(Literal::Integer(_)) => DataType::Long,
                Some(Literal::Float(_)) => DataType::Double,
                Some(Literal::String(_)) => DataType::String,
                // A variable declared with `AS` carries that type rather than
                // the one its name's suffix would imply.
                // An already-allocated slot knows its own type -- which for a
                // parameter declared `N AS INTEGER` is the declared one, not
                // what the name's suffix would imply.
                None => match self.lookup_var(name) {
                    Some(info) => info.data_type,
                    None => match self.typed_var(name) {
                        Some(t) => Self::data_type_of(&t),
                        None => DataType::from_suffix(name),
                    },
                },
            },
            Expr::ArrayAccess { name, .. } => DataType::from_suffix(name),
            Expr::FnCall { name, args } => self.call_return_type(name, args),
            Expr::Field { .. } => self.field_expr_type(expr),
            Expr::Unary { operand, .. } => self.expr_type(operand),
            Expr::Binary { left, right, op } => {
                let lt = self.expr_type(left);
                let rt = self.expr_type(right);
                self.promote_types(lt, rt, *op)
            }
        }
    }

    /// Get the return type of a function (built-in or user-defined)
    /// Result type of ABS, which preserves its argument's type.
    fn abs_result_type(arg: DataType) -> DataType {
        match arg {
            DataType::Integer | DataType::Long => DataType::Long,
            DataType::Single => DataType::Single,
            _ => DataType::Double,
        }
    }

    /// Return type of a call, including builtins whose result type depends on
    /// their argument.
    fn call_return_type(&self, name: &str, args: &[Expr]) -> DataType {
        if name.to_uppercase() == "ABS" && !args.is_empty() {
            return Self::abs_result_type(self.expr_type(&args[0]));
        }
        self.fn_return_type(name)
    }

    fn fn_return_type(&self, name: &str) -> DataType {
        // Built-in functions that return strings
        let upper = name.to_uppercase();
        if upper.ends_with('$') {
            return DataType::String;
        }
        // Built-in functions that return integers
        match upper.as_str() {
            "LEN" | "ASC" | "INSTR" | "CINT" | "CLNG" => DataType::Long,
            "EOF" | "LBOUND" | "UBOUND" => DataType::Long,
            // CSNG converts to SINGLE; saying Double here made its result print
            // with a Double's digits.
            "CSNG" => DataType::Single,
            // Most built-ins and user functions: check suffix, default to Double
            _ => DataType::from_suffix(name),
        }
    }

    /// Promote two types to a common type for binary operations
    fn promote_types(&self, left: DataType, right: DataType, op: BinaryOp) -> DataType {
        // Comparison operators always return Integer (0 or -1 for boolean)
        if matches!(
            op,
            BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge
        ) {
            return DataType::Long; // Boolean result as Long
        }

        // Division (/) always produces Double per GW-BASIC
        if op == BinaryOp::Div {
            return DataType::Double;
        }

        // Integer division (\) always produces Long
        if op == BinaryOp::IntDiv {
            return DataType::Long;
        }

        // MOD produces integer type
        if op == BinaryOp::Mod {
            return DataType::Long;
        }

        // Power (^) always produces Double (uses libm pow())
        if op == BinaryOp::Pow {
            return DataType::Double;
        }

        // String concatenation
        if left == DataType::String && right == DataType::String {
            return DataType::String;
        }

        // Numeric promotion: Integer < Long < Single < Double
        match (left, right) {
            (DataType::Double, _) | (_, DataType::Double) => DataType::Double,
            (DataType::Single, _) | (_, DataType::Single) => DataType::Single,
            (DataType::Long, _) | (_, DataType::Long) => DataType::Long,
            _ => DataType::Integer,
        }
    }

    /// Generate code to coerce a value from one type to another.
    /// Convention: integers in eax, floats in xmm0
    fn gen_coercion(&mut self, from: DataType, to: DataType) {
        if from == to {
            return;
        }

        match (from, to) {
            // Integer to Long (sign extension, but both in eax so just use movsxd conceptually)
            (DataType::Integer, DataType::Long) => {
                self.emit("    movsx eax, ax"); // sign-extend 16-bit to 32-bit
            }
            // Long to Integer (truncation - just use lower 16 bits)
            (DataType::Long, DataType::Integer) => {
                // No-op in eax, value is truncated when stored
            }
            // Integer/Long to Single
            (DataType::Integer | DataType::Long, DataType::Single) => {
                self.emit("    cvtsi2ss xmm0, eax");
            }
            // Integer/Long to Double
            (DataType::Integer | DataType::Long, DataType::Double) => {
                self.emit("    cvtsi2sd xmm0, eax");
            }
            // Single to Double
            (DataType::Single, DataType::Double) => {
                self.emit("    cvtss2sd xmm0, xmm0");
            }
            // Double to Single
            (DataType::Double, DataType::Single) => {
                self.emit("    cvtsd2ss xmm0, xmm0");
            }
            // Single to Integer/Long (truncate)
            (DataType::Single, DataType::Integer | DataType::Long) => {
                self.emit("    cvttss2si eax, xmm0");
            }
            // Double to Integer/Long (truncate)
            (DataType::Double, DataType::Integer | DataType::Long) => {
                self.emit("    cvttsd2si eax, xmm0");
            }
            // String conversions are not supported implicitly
            (DataType::String, _) | (_, DataType::String) => {
                panic!("Cannot implicitly convert to/from String");
            }
            // Same type - no conversion needed (shouldn't reach here due to early return)
            _ => {}
        }
    }

    pub fn generate(&mut self, program: &Program, symbols: Symbols, opts: Options) -> String {
        self.symbols = symbols;
        self.opts = opts;
        // First pass: collect DATA statements and check for GOSUB
        for stmt in &program.statements {
            self.preprocess(stmt);
        }

        // Render main into a scratch buffer *before* the procedures.
        //
        // Procedures used to be emitted first, which meant `vars` and `arrays`
        // were still empty while a procedure body was compiled: a reference to a
        // module-level variable allocated a fresh procedure local (so globals
        // read as 0 inside a SUB) and a reference to a module-level array hit
        // "Array not declared". Compiling main first means that by the time any
        // procedure is compiled, every module-level name is known and classified
        // as a global. The alternative -- a separate collect_globals walker --
        // would have to mirror every name-introducing statement forever.
        let main_asm = {
            let saved = std::mem::take(&mut self.output);
            self.gen_main(program);
            std::mem::replace(&mut self.output, saved)
        };

        // Emit assembly header
        self.emit(".intel_syntax noprefix");
        self.emit(".text");
        let p = PREFIX;
        self.emit(&format!(".globl {}main", p));
        self.emit("");

        // Procedures
        for stmt in &program.statements {
            if let StmtKind::Sub { name, params, body } = &stmt.kind {
                self.gen_procedure(name, params, body, false);
            } else if let StmtKind::Function { name, params, body } = &stmt.kind {
                self.gen_procedure(name, params, body, true);
            }
        }

        self.output.push_str(&main_asm);

        // Error trampolines, past every function body so the checked fast path
        // is only a compare and a never-taken branch.
        self.emit_error_trampolines();

        // Emit data section
        self.emit_data_section();

        self.output.clone()
    }

    /// Reserve descriptors for every array declared in `scope`.
    ///
    /// Done before any code is emitted, so that an array used earlier in
    /// program order than its `DIM` still has somewhere to read from. Its
    /// element pointer is null until the DIM runs, which the bounds check
    /// reports as "Array used before DIM" rather than crashing.
    fn reserve_array_descriptors(&mut self, scope: &SemaScope) {
        let mut decls: Vec<(String, usize)> = self
            .symbols
            .arrays
            .iter()
            .filter(|((s, _), _)| s == scope)
            .map(|((_, name), info)| (name.clone(), info.rank))
            .collect();
        decls.sort();

        for (name, rank) in decls {
            if self.lookup_array(&name).is_some() {
                continue;
            }
            let loc = if self.current_proc.is_some() {
                self.stack_offset -= 8 * (1 + rank as i32);
                Loc::Frame(self.stack_offset)
            } else {
                Loc::Global(format!("_arr_{}", mangle(&name)))
            };
            let info = ArrayInfo { loc, ndims: rank };
            if self.current_proc.is_some() {
                self.proc_arrays.insert(name, info);
            } else {
                self.arrays.insert(name, info);
            }
        }
    }

    /// Emit `main`: prologue, module-level statements, epilogue.
    fn gen_main(&mut self, program: &Program) {
        self.reserve_array_descriptors(&SemaScope::Module);
        let p = PREFIX;
        self.emit_label(&format!("{}main", p));
        self.emit("    push rbp");
        self.emit("    mov rbp, rsp");

        // Reserve stack space (will patch later)
        self.emit("    sub rsp, 0         # STACK_RESERVE");

        // Initialize GOSUB return stack if needed
        if self.gosub_used {
            self.emit("    # Initialize GOSUB return stack");
            self.emit(&format!(
                "    lea rax, [rip + _gosub_stack + {}]",
                GOSUB_STACK_SIZE
            )); // Point to end (stack grows down)
            self.emit("    mov QWORD PTR [rip + _gosub_sp], rax");
        }

        // Windows: Initialize console handles for Win32 API
        #[cfg(windows)]
        {
            self.emit("    # Initialize Windows console handles");
            self.emit("    call _rt_init_console");
            self.emit("    call _rt_init_input");
        }

        // Generate main body
        for stmt in &program.statements {
            match stmt.kind {
                StmtKind::Sub { .. } | StmtKind::Function { .. } => {}
                _ => self.gen_stmt(stmt),
            }
        }

        // Exit
        self.emit("    xor eax, eax");
        self.emit("    leave");
        self.emit("    ret");
        self.emit("");

        // Patch stack reserve
        // System V AMD64 ABI stack alignment rules:
        // - On function entry (after call pushed return addr): rsp % 16 == 8
        // - After push rbp: rsp % 16 == 0
        // - Before any call: rsp % 16 == 0
        //
        // Since we use 16-byte sub/add for all temporaries in expression evaluation,
        // we just need sub rsp, N where N is a multiple of 16 to maintain alignment.
        // Only main's own buffer is in `self.output` here, so this cannot reach
        // a procedure's placeholder -- note "# STACK_RESERVE" is a prefix of
        // "# STACK_RESERVE_PROC_<name>".
        let stack_needed = -self.stack_offset;
        let stack_size = (stack_needed + 15) & !15; // Round up to multiple of 16
        let old = "    sub rsp, 0         # STACK_RESERVE";
        let new = format!("    sub rsp, {}        # STACK_RESERVE", stack_size);
        self.output = self.output.replace(old, &new);
    }

    /// Preprocess statement: collect DATA items and check for GOSUB usage
    fn preprocess(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Data(values) => self.data_items.extend(values.clone()),
            StmtKind::Gosub(_) => self.gosub_used = true,
            // Record where each label sits in the DATA stream, so RESTORE can
            // resume from it.
            StmtKind::Label(n) => {
                self.data_line_index.insert(*n, self.data_items.len());
            }
            StmtKind::LabelName(name) => {
                self.data_label_index
                    .insert(name.to_uppercase(), self.data_items.len());
            }
            _ => {}
        }
        // Recurse into nested statements
        for body in child_bodies(stmt) {
            for s in body {
                self.preprocess(s);
            }
        }
    }

    /// Guard a math function whose argument must be in range: compare the
    /// value in `xmm0` against zero and raise "Illegal function call" when
    /// `cond` holds (`jb` for "negative", `jbe` for "not positive").
    fn emit_domain_check(&mut self, cond: &str) {
        if !self.opts.checks {
            return;
        }
        self.emit("    xorpd xmm1, xmm1");
        self.emit("    ucomisd xmm0, xmm1");
        self.emit_check(cond, RtError::Domain);
    }

    /// Guard an integer divide: `idiv` raises #DE (a SIGFPE crash) both when
    /// the divisor is zero and for INT_MIN / -1, which overflows the quotient.
    /// The divisor is expected in `ecx`.
    fn emit_integer_divide_checks(&mut self) {
        if !self.opts.checks {
            return;
        }
        self.emit("    test ecx, ecx");
        self.emit_check("je", RtError::DivideByZero);
        // INT_MIN / -1 overflows; both operands must match for it to trap.
        self.emit("    cmp ecx, -1");
        let skip = self.new_label("nodivovf");
        self.emit(&format!("    jne {}", skip));
        self.emit("    cmp eax, -2147483648");
        self.emit_check("je", RtError::Overflow);
        self.emit_label(&skip);
    }

    /// Label of the trampoline that raises `kind` at the current line,
    /// creating it if this is the first site to need it.
    fn error_label(&mut self, kind: RtError) -> String {
        let line = self.current_line;
        self.error_sites
            .entry((kind, line))
            .or_insert_with(|| format!(".Lerr_{}_{}", kind.tag(), line))
            .clone()
    }

    /// With `OPTION BASE 1`, subscript 0 is out of range too.
    ///
    /// Element storage is still allocated for it -- the descriptor keeps
    /// holding N+1 elements and slot 0 simply goes unused -- which leaves the
    /// linear index arithmetic completely unchanged.
    fn emit_lower_bound_check(&mut self, reg: &str) {
        if self.symbols.option_base == 0 {
            return;
        }
        self.emit(&format!("    cmp {}, {}", reg, self.symbols.option_base));
        self.emit_check("jb", RtError::Subscript);
    }

    /// Emit a check: branch to the error trampoline when `cond` holds.
    ///
    /// The fast path costs only the caller's compare plus this never-taken
    /// branch; everything else lives in the cold trampoline.
    fn emit_check(&mut self, cond: &str, kind: RtError) {
        if !self.opts.checks {
            return;
        }
        let label = self.error_label(kind);
        self.emit(&format!("    {} {}", cond, label));
    }

    /// Emit every error trampoline collected during code generation.
    fn emit_error_trampolines(&mut self) {
        let sites = std::mem::take(&mut self.error_sites);
        if sites.is_empty() {
            return;
        }
        self.emit("");
        self.emit("# Runtime error trampolines (cold; never fall through)");
        for ((kind, line), label) in sites {
            self.emit_label(&label);
            let sym = kind.symbol();
            self.emit(&format!("    lea {}, [rip + {}]", Self::arg_reg(0), sym));
            self.emit(&format!("    mov {}, {}", Self::arg_reg(1), line));
            self.emit("    call _rt_error");
        }
    }

    /// Zero the current frame, from `rsp` up to `rbp`.
    ///
    /// Emitted in a procedure prologue right after the stack reserve and before
    /// parameters are spilled. Uses `r10`/`r11`, which are caller-saved and are
    /// argument registers on neither System V nor Win64, so the incoming
    /// arguments stay live. (`rep stosq` would clobber `rdi`/`rcx`/`rax`, all of
    /// which carry arguments.)
    ///
    /// The loop is bottom-tested, so a zero-sized frame runs no iterations, and
    /// it derives its bounds from the registers rather than the reserve size, so
    /// it cannot drift out of step with the backpatched `sub rsp, N`.
    fn emit_zero_frame(&mut self) {
        let body = self.new_label("zero");
        let check = self.new_label("zchk");
        self.emit("    # zero locals: [rsp, rbp)");
        self.emit("    mov r11, rsp");
        self.emit("    mov r10, rbp");
        self.emit(&format!("    jmp {}", check));
        self.emit_label(&body);
        self.emit("    mov QWORD PTR [r11], 0");
        self.emit("    add r11, 8");
        self.emit_label(&check);
        self.emit("    cmp r11, r10");
        self.emit(&format!("    jb {}", body));
    }

    fn gen_procedure(&mut self, name: &str, params: &[Param], body: &[Stmt], is_function: bool) {
        self.current_proc = Some(name.to_string());
        self.proc_vars.clear();
        self.proc_arrays.clear();
        self.proc_types.clear();
        self.proc_record_vars.clear();
        self.proc_record_elem_words.clear();
        let old_stack_offset = self.stack_offset;
        self.stack_offset = 0;

        // Procedure label
        self.emit_label(&format!("_proc_{}", mangle(name)));
        self.emit("    push rbp");
        self.emit("    mov rbp, rsp");

        // Reserve stack space (will patch later with actual size)
        let placeholder = format!("    sub rsp, 0         # STACK_RESERVE_PROC_{}", name);
        self.emit(&placeholder);

        // Zero the frame before anything is spilled into it, so that procedure
        // locals start at 0 / "" on every call the way module-level variables
        // do. Must come before the parameter spill below, which would otherwise
        // be wiped.
        self.emit_zero_frame();

        self.reserve_array_descriptors(&SemaScope::Proc(name.to_string()));

        // Spill parameters into the frame, using the same placement the call
        // site computed from the same declared types.
        let int_regs = PlatformAbi::INT_ARG_REGS;
        let param_types: Vec<DataType> = params.iter().map(Self::param_data_type).collect();
        let (places, _) = Self::classify_params(&param_types);

        for (param_decl, place) in params.iter().zip(&places) {
            let param = &param_decl.name;

            // A record parameter arrives as a pointer to the caller's copy.
            // Copying it into a local slot here gives by-value semantics and
            // lets field access use ordinary frame addressing.
            if let Some(TypeRef::Record(_)) = &param_decl.ty {
                let ty = param_decl.ty.clone().expect("matched above");
                let words = self.symbols.type_words(&ty);
                self.stack_offset -= 8 * words;
                let loc = Loc::Frame(self.stack_offset);
                let src = match place.ptr {
                    Slot::Reg(i) => int_regs[i].to_string(),
                    Slot::Stk(i) => {
                        self.emit(&format!(
                            "    mov r11, QWORD PTR [rbp + {}]",
                            16 + 8 * i as i32
                        ));
                        "r11".to_string()
                    }
                };
                self.emit(&format!("    mov r10, {}", src));
                for w in 0..words {
                    self.emit(&format!("    mov rax, QWORD PTR [r10 + {}]", w * 8));
                    self.emit(&format!("    mov {}, rax", loc.q(w)));
                }
                self.proc_record_vars.insert(param.clone(), loc);
                self.proc_types.insert(param.clone(), ty);
                continue;
            }

            self.stack_offset -= 8 * Self::words_for(place.ty);
            let loc = Loc::Frame(self.stack_offset);
            self.proc_vars.insert(
                param.clone(),
                VarInfo {
                    loc: loc.clone(),
                    data_type: place.ty,
                },
            );

            // Bring a slot's value into a register we can store from. r11 is
            // caller-saved and an argument register on neither ABI, so using it
            // as scratch cannot clobber a parameter still to be spilled.
            let fetch = |s: &mut Self, slot: Slot| -> String {
                match slot {
                    Slot::Reg(i) => int_regs[i].to_string(),
                    Slot::Stk(i) => {
                        s.emit(&format!(
                            "    mov r11, QWORD PTR [rbp + {}]",
                            16 + 8 * i as i32
                        ));
                        "r11".to_string()
                    }
                }
            };

            match place.ty {
                DataType::String => {
                    let p = fetch(self, place.ptr);
                    self.emit(&format!("    mov {}, {}", loc.q(0), p));
                    let l = fetch(self, place.len.expect("string parameter has a length slot"));
                    self.emit(&format!("    mov {}, {}", loc.q(1), l));
                }
                DataType::Double => {
                    let p = fetch(self, place.ptr);
                    self.emit(&format!("    mov {}, {}", loc.q(0), p));
                }
                // Numeric arguments arrive as f64 bit patterns; narrow to the
                // declared type. rax/xmm0 are safe scratch: neither is an
                // argument register on either ABI.
                DataType::Single => {
                    let p = fetch(self, place.ptr);
                    self.emit(&format!("    movq xmm0, {}", p));
                    self.emit("    cvtsd2ss xmm0, xmm0");
                    self.emit(&format!("    movss {}, xmm0", loc.at("DWORD PTR", 0)));
                }
                DataType::Integer | DataType::Long => {
                    let p = fetch(self, place.ptr);
                    self.emit(&format!("    movq xmm0, {}", p));
                    self.emit("    cvttsd2si eax, xmm0");
                    let (size, reg) = if place.ty == DataType::Integer {
                        ("WORD PTR", "ax")
                    } else {
                        ("DWORD PTR", "eax")
                    };
                    self.emit(&format!("    mov {}, {}", loc.at(size, 0), reg));
                }
            }
        }

        // If function, allocate return value slot
        if is_function {
            let data_type = DataType::from_suffix(name);
            self.stack_offset -= 8 * Self::words_for(data_type);
            self.proc_vars.insert(
                name.to_string(),
                VarInfo {
                    loc: Loc::Frame(self.stack_offset),
                    data_type,
                },
            );
        }

        // Generate body
        let exit_label = format!(".Lproc_exit_{}", mangle(name));
        let saved_exit = self.proc_exit_label.replace(exit_label.clone());
        let saved_loops = std::mem::take(&mut self.loop_stack);
        for stmt in body {
            self.gen_stmt(stmt);
        }
        self.loop_stack = saved_loops;
        self.proc_exit_label = saved_exit;
        self.emit_label(&exit_label);

        // Return - load return value into appropriate register based on type
        if is_function {
            let ret_info = &self.proc_vars[name];
            let loc = ret_info.loc.clone();
            let data_type = ret_info.data_type;
            match data_type {
                DataType::Integer => {
                    self.emit(&format!("    movsx eax, {}", loc.at("WORD PTR", 0)));
                }
                DataType::Long => {
                    self.emit(&format!("    mov eax, {}", loc.at("DWORD PTR", 0)));
                }
                DataType::Single => {
                    self.emit(&format!("    movss xmm0, {}", loc.at("DWORD PTR", 0)));
                }
                DataType::Double => {
                    self.emit(&format!("    movsd xmm0, {}", loc.q(0)));
                }
                DataType::String => {
                    // Load string (ptr, len) into rax, rdx
                    self.emit(&format!("    mov rax, {}", loc.q(0)));
                    self.emit(&format!("    mov rdx, {}", loc.q(1)));
                }
            }
        }

        self.emit("    leave");
        self.emit("    ret");
        self.emit("");

        // Patch the stack reserve placeholder with actual size
        let stack_needed = -self.stack_offset;
        let stack_size = (stack_needed + 15) & !15; // Round up to multiple of 16
        let old_placeholder = format!("    sub rsp, 0         # STACK_RESERVE_PROC_{}", name);
        let new_instruction = format!(
            "    sub rsp, {}        # STACK_RESERVE_PROC_{}",
            stack_size, name
        );
        self.output = self.output.replace(&old_placeholder, &new_instruction);

        self.current_proc = None;
        self.stack_offset = old_stack_offset;
    }

    fn gen_stmt(&mut self, stmt: &Stmt) {
        if stmt.line != 0 {
            self.current_line = stmt.line;
        }
        match &stmt.kind {
            StmtKind::Label(n) => {
                self.emit_label(&format!("_line_{}", n));
            }

            StmtKind::LabelName(name) => {
                self.emit_label(&format!("_label_{}", mangle(name)));
            }

            // Assignment to a record field.
            StmtKind::Let {
                name,
                indices: None,
                value,
            } if self.typed_var(name).is_some() && !self.has_plain_slot(name) => {
                let Some((loc, ty)) = self.typed_storage(name) else {
                    unreachable!("guarded above")
                };
                // Assigning one whole record to another copies its words.
                if let TypeRef::Record(_) = &ty {
                    let words = self.symbols.type_words(&ty);
                    match value {
                        Expr::Variable(src) => {
                            let src_ty = self.typed_var(src).expect("sema checked the source");
                            let src_loc = self.get_record_loc(src, &src_ty);
                            for w in 0..words {
                                self.emit(&format!("    mov rax, {}", src_loc.q(w)));
                                self.emit(&format!("    mov {}, rax", loc.q(w)));
                            }
                            return;
                        }
                        // An array element's address is only known at run time.
                        Expr::ArrayAccess { name, indices }
                        | Expr::FnCall {
                            name,
                            args: indices,
                        } => {
                            let indices = indices.clone();
                            self.gen_array_addr(name, &indices);
                            self.emit("    mov r10, rax");
                            for w in 0..words {
                                self.emit(&format!("    mov rax, QWORD PTR [r10 + {}]", w * 8));
                                self.emit(&format!("    mov {}, rax", loc.q(w)));
                            }
                            return;
                        }
                        _ => {}
                    }
                }
                let vt = self.gen_expr(value);
                self.gen_store_typed(&loc, &ty, vt);
            }

            StmtKind::Let {
                name,
                indices,
                value,
            } => {
                if indices.is_some() {
                    // Array assignment
                    self.gen_array_store(name, indices.as_ref().unwrap(), value);
                } else if is_string_var(name) {
                    self.gen_string_assign(name, value);
                } else {
                    // Evaluate expression and get its type
                    let expr_type = self.gen_expr(value);
                    let var_info = self.get_var_info(name);

                    // Coerce to target type
                    self.gen_coercion(expr_type, var_info.data_type);

                    // Store based on target type
                    let loc = &var_info.loc;
                    match var_info.data_type {
                        DataType::Integer => {
                            self.emit(&format!("    mov {}, ax", loc.at("WORD PTR", 0)));
                        }
                        DataType::Long => {
                            self.emit(&format!("    mov {}, eax", loc.at("DWORD PTR", 0)));
                        }
                        DataType::Single => {
                            self.emit(&format!("    movss {}, xmm0", loc.at("DWORD PTR", 0)));
                        }
                        DataType::Double => {
                            self.emit(&format!("    movsd {}, xmm0", loc.q(0)));
                        }
                        DataType::String => {
                            // Should be handled by gen_string_assign above
                            unreachable!("String assignment should be handled separately");
                        }
                    }
                }
            }

            StmtKind::Print {
                items,
                newline,
                using: Some(fmt),
                ..
            } => {
                self.gen_print_using(fmt, items);
                if *newline {
                    self.emit("    call _rt_print_newline");
                }
            }

            StmtKind::Print {
                items,
                newline,
                write,
                ..
            } => {
                let mut first = true;
                for item in items {
                    match item {
                        PrintItem::Expr(expr) => {
                            // WRITE separates values with commas and quotes
                            // strings; PRINT emits them bare.
                            if *write {
                                if !first {
                                    self.emit_arg_imm(0, ASCII_COMMA);
                                    self.emit("    call _rt_print_char");
                                }
                                self.gen_write_expr(expr);
                            } else {
                                self.gen_print_expr(expr);
                            }
                            first = false;
                        }
                        PrintItem::Tab => {
                            if !*write {
                                self.emit_arg_imm(0, ASCII_TAB);
                                self.emit("    call _rt_print_char");
                            }
                        }
                        PrintItem::Empty => {}
                    }
                }
                if *newline {
                    self.emit("    call _rt_print_newline");
                }
            }

            StmtKind::Input { prompt, vars } => {
                if let Some(pstr) = prompt {
                    let idx = self.add_string_literal(pstr);
                    self.emit_arg_lea(0, &format!("[rip + _str_{}]", idx));
                    self.emit_arg_imm(1, pstr.len() as i64);
                    self.emit("    call _rt_print_string");
                }
                for var in vars {
                    if is_string_var(&var.name) {
                        self.emit("    call _rt_input_string");
                    } else {
                        self.emit("    call _rt_input_number");
                    }
                    self.gen_store_lvalue(var);
                }
            }

            StmtKind::LineInput {
                prompt,
                var,
                file_num,
            } => {
                if let Some(pstr) = prompt {
                    let idx = self.add_string_literal(pstr);
                    self.emit_arg_lea(0, &format!("[rip + _str_{}]", idx));
                    self.emit_arg_imm(1, pstr.len() as i64);
                    self.emit("    call _rt_print_string");
                }
                match file_num {
                    Some(e) => {
                        let fnum = self.gen_file_num(e);
                        self.emit_arg_file_num(0, &fnum);
                        self.emit("    call _rt_file_input_string");
                    }
                    None => self.emit("    call _rt_input_string"),
                }
                self.gen_store_lvalue(var);
            }

            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let else_label = self.new_label("else");
                let end_label = self.new_label("endif");

                let cond_type = self.gen_expr(condition);
                // Compare with 0 - conditions typically return Long (integer) now
                if cond_type.is_integer() {
                    self.emit("    test eax, eax");
                    self.emit(&format!("    je {}", else_label));
                } else {
                    self.emit("    xorpd xmm1, xmm1");
                    self.emit("    ucomisd xmm0, xmm1");
                    self.emit(&format!("    je {}", else_label));
                }

                for s in then_branch {
                    self.gen_stmt(s);
                }
                self.emit(&format!("    jmp {}", end_label));

                self.emit_label(&else_label);
                if let Some(eb) = else_branch {
                    for s in eb {
                        self.gen_stmt(s);
                    }
                }

                self.emit_label(&end_label);
            }

            StmtKind::For {
                var,
                start,
                end,
                step,
                body,
            } => {
                let start_label = self.new_label("for");
                let end_label = self.new_label("endfor");
                let var_loc = self.get_var_loc(var);

                // Initialize loop variable - coerce to double
                let start_type = self.gen_expr(start);
                self.gen_coercion(start_type, DataType::Double);
                self.emit(&format!("    movsd {}, xmm0", var_loc.q(0)));

                // Store end value - coerce to double
                self.stack_offset -= 8;
                let end_offset = self.stack_offset;
                let end_type = self.gen_expr(end);
                self.gen_coercion(end_type, DataType::Double);
                self.emit(&format!("    movsd QWORD PTR [rbp + {}], xmm0", end_offset));

                // Store step value - coerce to double
                self.stack_offset -= 8;
                let step_offset = self.stack_offset;
                if let Some(s) = step {
                    let step_type = self.gen_expr(s);
                    self.gen_coercion(step_type, DataType::Double);
                } else {
                    self.emit("    mov rax, 0x3FF0000000000000  # 1.0");
                    self.emit("    movq xmm0, rax");
                }
                self.emit(&format!(
                    "    movsd QWORD PTR [rbp + {}], xmm0",
                    step_offset
                ));

                self.emit_label(&start_label);

                // Check condition (var > end for positive step, var < end for negative)
                self.emit(&format!("    movsd xmm0, {}", var_loc.q(0)));
                self.emit(&format!("    movsd xmm1, QWORD PTR [rbp + {}]", end_offset));
                self.emit(&format!(
                    "    movsd xmm2, QWORD PTR [rbp + {}]",
                    step_offset
                ));
                self.emit("    xorpd xmm3, xmm3");
                self.emit("    ucomisd xmm2, xmm3");
                self.emit(&format!("    jb .Lfor_neg_{}", self.label_counter));

                // Positive step: exit if var > end
                self.emit("    ucomisd xmm0, xmm1");
                self.emit(&format!("    ja {}", end_label));
                self.emit(&format!("    jmp .Lfor_body_{}", self.label_counter));

                // Negative step: exit if var < end
                self.emit_label(&format!(".Lfor_neg_{}", self.label_counter));
                self.emit("    ucomisd xmm0, xmm1");
                self.emit(&format!("    jb {}", end_label));

                self.emit_label(&format!(".Lfor_body_{}", self.label_counter));
                self.label_counter += 1;

                // Body
                self.loop_stack.push((true, end_label.clone()));
                for s in body {
                    self.gen_stmt(s);
                }
                self.loop_stack.pop();

                // Increment
                self.emit(&format!("    movsd xmm0, {}", var_loc.q(0)));
                self.emit(&format!(
                    "    addsd xmm0, QWORD PTR [rbp + {}]",
                    step_offset
                ));
                self.emit(&format!("    movsd {}, xmm0", var_loc.q(0)));
                self.emit(&format!("    jmp {}", start_label));

                self.emit_label(&end_label);
            }

            StmtKind::While { condition, body } => {
                let start_label = self.new_label("while");
                let end_label = self.new_label("endwhile");

                self.emit_label(&start_label);
                let cond_type = self.gen_expr(condition);
                if cond_type.is_integer() {
                    self.emit("    test eax, eax");
                    self.emit(&format!("    je {}", end_label));
                } else {
                    self.emit("    xorpd xmm1, xmm1");
                    self.emit("    ucomisd xmm0, xmm1");
                    self.emit(&format!("    je {}", end_label));
                }

                // WHILE is a DO-family loop for EXIT DO purposes.
                self.loop_stack.push((false, end_label.clone()));
                for s in body {
                    self.gen_stmt(s);
                }
                self.loop_stack.pop();
                self.emit(&format!("    jmp {}", start_label));

                self.emit_label(&end_label);
            }

            StmtKind::DoLoop {
                condition,
                cond_at_start,
                is_until,
                body,
            } => {
                let start_label = self.new_label("do");
                let end_label = self.new_label("enddo");

                self.emit_label(&start_label);

                if *cond_at_start {
                    if let Some(cond) = condition {
                        let cond_type = self.gen_expr(cond);
                        if cond_type.is_integer() {
                            self.emit("    test eax, eax");
                            if *is_until {
                                self.emit(&format!("    jne {}", end_label));
                            } else {
                                self.emit(&format!("    je {}", end_label));
                            }
                        } else {
                            self.emit("    xorpd xmm1, xmm1");
                            self.emit("    ucomisd xmm0, xmm1");
                            if *is_until {
                                self.emit(&format!("    jne {}", end_label));
                            } else {
                                self.emit(&format!("    je {}", end_label));
                            }
                        }
                    }
                }

                self.loop_stack.push((false, end_label.clone()));
                for s in body {
                    self.gen_stmt(s);
                }
                self.loop_stack.pop();

                if !*cond_at_start {
                    if let Some(cond) = condition {
                        let cond_type = self.gen_expr(cond);
                        if cond_type.is_integer() {
                            self.emit("    test eax, eax");
                            if *is_until {
                                self.emit(&format!("    je {}", start_label));
                            } else {
                                self.emit(&format!("    jne {}", start_label));
                            }
                        } else {
                            self.emit("    xorpd xmm1, xmm1");
                            self.emit("    ucomisd xmm0, xmm1");
                            if *is_until {
                                self.emit(&format!("    je {}", start_label));
                            } else {
                                self.emit(&format!("    jne {}", start_label));
                            }
                        }
                    } else {
                        self.emit(&format!("    jmp {}", start_label));
                    }
                } else {
                    self.emit(&format!("    jmp {}", start_label));
                }

                self.emit_label(&end_label);
            }

            StmtKind::Goto(target) => {
                let label = match target {
                    GotoTarget::Line(n) => format!("_line_{}", n),
                    GotoTarget::Label(s) => format!("_label_{}", mangle(s)),
                };
                self.emit(&format!("    jmp {}", label));
            }

            StmtKind::Gosub(target) => {
                let label = match target {
                    GotoTarget::Line(n) => format!("_line_{}", n),
                    GotoTarget::Label(s) => format!("_label_{}", mangle(s)),
                };
                let ret_label = self.new_label("gosub_ret");
                // Check for stack overflow before push
                self.emit("    mov rcx, QWORD PTR [rip + _gosub_sp]");
                self.emit("    sub rcx, 8");
                self.emit("    lea rax, [rip + _gosub_stack]");
                self.emit("    cmp rcx, rax");
                self.emit("    jb _rt_gosub_overflow");
                // Push return address to GOSUB stack
                self.emit(&format!("    lea rax, [rip + {}]", ret_label));
                self.emit("    mov QWORD PTR [rcx], rax");
                self.emit("    mov QWORD PTR [rip + _gosub_sp], rcx");
                self.emit(&format!("    jmp {}", label));
                self.emit_label(&ret_label);
            }

            StmtKind::Return => {
                // Pop return address from GOSUB stack and jump (use rcx - caller-saved on both ABIs)
                self.emit("    mov rcx, QWORD PTR [rip + _gosub_sp]");
                self.emit("    mov rax, QWORD PTR [rcx]");
                self.emit("    add rcx, 8");
                self.emit("    mov QWORD PTR [rip + _gosub_sp], rcx");
                self.emit("    jmp rax");
            }

            StmtKind::OnGoto { expr, targets } => {
                let expr_type = self.gen_expr(expr);
                // Convert to integer in rax
                if expr_type.is_integer() {
                    self.emit("    movsxd rax, eax");
                } else {
                    self.emit("    cvttsd2si rax, xmm0");
                }
                // Create jump table
                for (i, target) in targets.iter().enumerate() {
                    let label = match target {
                        GotoTarget::Line(n) => format!("_line_{}", n),
                        GotoTarget::Label(s) => format!("_label_{}", mangle(s)),
                    };
                    self.emit(&format!("    cmp rax, {}", i + 1));
                    self.emit(&format!("    je {}", label));
                }
            }

            StmtKind::Dim { decls } => {
                for decl in decls {
                    self.gen_declarator(decl, false);
                }
            }

            StmtKind::Sub { .. } | StmtKind::Function { .. } => {
                // Already handled in first pass
            }

            StmtKind::Call { name, args } => {
                self.gen_call(name, args);
            }

            StmtKind::Data(_) => {
                // Data already collected in first pass
            }

            StmtKind::Read(vars) => {
                for var in vars {
                    if is_string_var(&var.name) {
                        // _rt_read_string returns (ptr, len); the length used to
                        // be discarded, leaving whatever was in the length word.
                        self.emit("    call _rt_read_string");
                    } else {
                        self.emit("    call _rt_read_number");
                    }
                    self.gen_store_lvalue(var);
                }
            }

            StmtKind::Restore(target) => {
                // RESTORE <line> resumes reading at the first DATA item at or
                // after that line. preprocess recorded how many items had been
                // seen when each label was reached; without that this always
                // restarted from the beginning, silently.
                let idx = match target {
                    Some(GotoTarget::Line(n)) => self.data_line_index.get(n).copied().unwrap_or(0),
                    Some(GotoTarget::Label(name)) => self
                        .data_label_index
                        .get(&name.to_uppercase())
                        .copied()
                        .unwrap_or(0),
                    None => 0,
                };
                self.emit_arg_imm(0, idx as i64);
                self.emit("    call _rt_restore");
            }

            // A TYPE definition emits nothing; its layout lives in Symbols.
            StmtKind::TypeDef { .. } => {}

            StmtKind::FieldAssign { target, value } => {
                if target.indices.is_some() {
                    self.gen_array_field(target, Some(value));
                } else {
                    let Some((loc, ty)) = self.resolve_field_path(&target.name, &target.fields)
                    else {
                        unreachable!("sema resolved this field path")
                    };
                    let vt = self.gen_expr(value);
                    self.gen_store_typed(&loc, &ty, vt);
                }
            }

            StmtKind::Redim { decls, preserve } => {
                for decl in decls {
                    self.gen_declarator(decl, *preserve);
                }
            }

            StmtKind::MidAssign {
                target,
                start,
                len,
                value,
            } => self.gen_mid_assign(target, start, len.as_ref(), value),

            StmtKind::Swap(a, b) => self.gen_swap(a, b),

            // CONST and OPTION BASE are resolved at compile time.
            StmtKind::Const { .. } | StmtKind::OptionBase(_) => {}

            StmtKind::ExitLoop { is_for } => {
                // Leave the innermost matching loop. Sema has already checked
                // that one exists.
                let target = self
                    .loop_stack
                    .iter()
                    .rev()
                    .find(|(f, _)| f == is_for)
                    .map(|(_, label)| label.clone());
                if let Some(label) = target {
                    self.emit(&format!("    jmp {}", label));
                }
            }

            StmtKind::ExitProc => {
                if let Some(label) = self.proc_exit_label.clone() {
                    self.emit(&format!("    jmp {}", label));
                }
            }

            StmtKind::Cls => {
                self.emit("    call _rt_cls");
            }

            StmtKind::SelectCase { expr, cases } => {
                let end_label = self.new_label("endselect");

                // Evaluate the selector once, into a frame slot. A string
                // needs two words, for its pointer and length.
                let is_string = self.expr_type(expr) == DataType::String;
                let expr_type = self.gen_expr(expr);
                let temp_offset;
                if is_string {
                    self.stack_offset -= 16;
                    temp_offset = self.stack_offset;
                    self.emit(&format!("    mov QWORD PTR [rbp + {}], rax", temp_offset));
                    self.emit(&format!(
                        "    mov QWORD PTR [rbp + {}], rdx",
                        temp_offset + 8
                    ));
                } else {
                    self.gen_coercion(expr_type, DataType::Double);
                    self.stack_offset -= 8;
                    temp_offset = self.stack_offset;
                    self.emit(&format!(
                        "    movsd QWORD PTR [rbp + {}], xmm0",
                        temp_offset
                    ));
                }

                // Generate code for each case
                for (i, (case_value, body)) in cases.iter().enumerate() {
                    let next_case_label = if i + 1 < cases.len() {
                        self.new_label("case")
                    } else {
                        end_label.clone()
                    };

                    if let Some(clauses) = case_value {
                        // Any alternative matching enters the body; all of them
                        // failing moves on to the next CASE.
                        let body_label = self.new_label("casebody");
                        for clause in clauses {
                            self.gen_case_clause(clause, temp_offset, is_string, &body_label);
                        }
                        self.emit(&format!("    jmp {}", next_case_label));
                        self.emit_label(&body_label);
                    }
                    // CASE ELSE (None) falls through without comparison

                    // Generate case body
                    for stmt in body {
                        self.gen_stmt(stmt);
                    }

                    // Jump to end (skip remaining cases)
                    if i + 1 < cases.len() {
                        self.emit(&format!("    jmp {}", end_label));
                        self.emit_label(&next_case_label);
                    }
                }

                self.emit_label(&end_label);
            }

            StmtKind::End | StmtKind::Stop => {
                // Terminate the program, not just the current frame. A plain
                // `leave; ret` only ends the program when END appears in main;
                // inside a SUB or FUNCTION it just returns to the caller.
                self.emit("    call _rt_end");
            }

            StmtKind::Open {
                filename,
                mode,
                file_num,
            } => {
                // _rt_file_open(filename_ptr, filename_len, mode, file_num)
                let fnum = self.gen_file_num(file_num);
                self.gen_expr(filename);
                self.emit_arg_reg(0, "rax"); // filename ptr
                self.emit_arg_reg(1, "rdx"); // filename len
                let mode_num = match mode {
                    FileMode::Input => 0,
                    FileMode::Output => 1,
                    FileMode::Append => 2,
                };
                self.emit_arg_imm(2, mode_num);
                self.emit_arg_file_num(3, &fnum);
                self.emit("    call _rt_file_open");
            }

            StmtKind::Close { file_num } => match file_num {
                Some(e) => {
                    let fnum = self.gen_file_num(e);
                    self.emit_arg_file_num(0, &fnum);
                    self.emit("    call _rt_file_close");
                }
                // Bare CLOSE closes every open file.
                None => self.emit("    call _rt_file_close_all"),
            },

            StmtKind::PrintFile {
                file_num,
                items,
                newline,
                using: Some(_),
                ..
            } => {
                // Sema rejects USING on file output for now, so this is only
                // reachable if that check is removed without adding support.
                let _ = (file_num, items, newline);
                unreachable!("PRINT # USING is rejected by semantic analysis")
            }

            StmtKind::PrintFile {
                file_num,
                items,
                newline,
                write,
                ..
            } => {
                let fnum = self.gen_file_num(file_num);
                let mut first = true;
                for item in items {
                    match item {
                        PrintItem::Expr(expr) => {
                            if *write && !first {
                                self.emit_arg_file_num(0, &fnum);
                                self.emit_arg_imm(1, ASCII_COMMA);
                                self.emit("    call _rt_file_print_char");
                            }
                            if *write {
                                self.gen_write_expr_to_file(expr, &fnum);
                            } else {
                                self.gen_print_expr_to_file(expr, &fnum);
                            }
                            first = false;
                        }
                        PrintItem::Tab => {
                            self.emit_arg_file_num(0, &fnum);
                            self.emit_arg_imm(1, ASCII_TAB);
                            self.emit("    call _rt_file_print_char");
                        }
                        PrintItem::Empty => {}
                    }
                }
                if *newline {
                    self.emit_arg_file_num(0, &fnum);
                    self.emit("    call _rt_file_print_newline");
                }
            }

            StmtKind::InputFile { file_num, vars } => {
                let fnum = self.gen_file_num(file_num);
                for var in vars {
                    self.emit_arg_file_num(0, &fnum);
                    if is_string_var(&var.name) {
                        self.emit("    call _rt_file_input_string");
                    } else {
                        self.emit("    call _rt_file_input_number");
                    }
                    self.gen_store_lvalue(var);
                }
            }
        }
    }

    /// Generate code for an expression.
    /// Returns the DataType of the result.
    /// Convention: integers in eax, floats in xmm0, strings in rax(ptr)/rdx(len)
    fn gen_expr(&mut self, expr: &Expr) -> DataType {
        match expr {
            Expr::Literal(lit) => match lit {
                Literal::Integer(n) => match i32::try_from(*n) {
                    Ok(v) => {
                        // Load as integer into eax
                        self.emit(&format!("    mov eax, {}", v));
                        DataType::Long
                    }
                    // Wider than LONG: emit as a Double rather than truncating
                    // to 32 bits, which silently turned 1000000000000001 into
                    // -1530494975. The lexer widens such literals already; this
                    // also covers values arriving from DATA.
                    Err(_) => {
                        let bits = (*n as f64).to_bits();
                        self.emit(&format!("    mov rax, 0x{:X}", bits));
                        self.emit("    movq xmm0, rax");
                        DataType::Double
                    }
                },
                Literal::Float(f) => {
                    // Load as double into xmm0
                    let bits = f.to_bits();
                    self.emit(&format!("    mov rax, 0x{:X}", bits));
                    self.emit("    movq xmm0, rax");
                    DataType::Double
                }
                Literal::String(s) => {
                    let idx = self.add_string_literal(s);
                    self.emit(&format!("    lea rax, [rip + _str_{}]", idx));
                    self.emit(&format!("    mov rdx, {}", s.len()));
                    DataType::String
                }
            },

            Expr::Variable(name) => {
                // A CONST is substituted with its folded value.
                if let Some(lit) = self.symbols.consts.get(&name.to_uppercase()).cloned() {
                    return self.gen_expr(&Expr::Literal(lit));
                }

                // A variable declared with `AS` lives in typed storage, which
                // is where its assignments went.
                if let Some((loc, ty)) = self.typed_storage(name) {
                    return self.gen_load_typed(&loc, &ty);
                }

                // A bare reference to a parameterless FUNCTION calls it, as in
                // QuickBASIC. Inside the function's own body the same name is
                // its return variable, so that case must not become infinite
                // recursion.
                let upper = name.to_uppercase();
                let is_own_name = self.current_proc.as_deref() == Some(upper.as_str());
                if !is_own_name
                    && self
                        .symbols
                        .procs
                        .get(&upper)
                        .is_some_and(|p| p.is_function && p.params.is_empty())
                {
                    self.gen_call(&upper, &[]);
                    return self.fn_return_type(&upper);
                }

                let info = self.get_var_info(name);
                let loc = &info.loc;
                match info.data_type {
                    DataType::Integer => {
                        self.emit(&format!("    movsx eax, {}", loc.at("WORD PTR", 0)));
                    }
                    DataType::Long => {
                        self.emit(&format!("    mov eax, {}", loc.at("DWORD PTR", 0)));
                    }
                    DataType::Single => {
                        self.emit(&format!("    movss xmm0, {}", loc.at("DWORD PTR", 0)));
                    }
                    DataType::Double => {
                        self.emit(&format!("    movsd xmm0, {}", loc.q(0)));
                    }
                    DataType::String => {
                        self.emit(&format!("    mov rax, {}", loc.q(0)));
                        self.emit(&format!("    mov rdx, {}", loc.q(1)));
                    }
                }
                info.data_type
            }

            Expr::ArrayAccess { name, indices } => {
                self.gen_array_load(name, indices);
                DataType::from_suffix(name)
            }

            Expr::Unary { op, operand } => {
                let operand_type = self.gen_expr(operand);
                match op {
                    UnaryOp::Neg => {
                        if operand_type.is_integer() {
                            self.emit("    neg eax");
                            operand_type
                        } else {
                            // Negate float by XORing sign bit
                            if operand_type == DataType::Single {
                                self.emit("    mov eax, 0x80000000");
                                self.emit("    movd xmm1, eax");
                                self.emit("    xorps xmm0, xmm1");
                            } else {
                                self.emit("    mov rax, 0x8000000000000000");
                                self.emit("    movq xmm1, rax");
                                self.emit("    xorpd xmm0, xmm1");
                            }
                            operand_type
                        }
                    }
                    UnaryOp::Not => {
                        // NOT: if 0 then -1, else 0 - result is always Long
                        if operand_type.is_integer() {
                            self.emit("    test eax, eax");
                        } else if operand_type == DataType::Single {
                            self.emit("    xorps xmm1, xmm1");
                            self.emit("    ucomiss xmm0, xmm1");
                        } else {
                            self.emit("    xorpd xmm1, xmm1");
                            self.emit("    ucomisd xmm0, xmm1");
                        }
                        self.emit("    sete al");
                        self.emit("    movzx eax, al");
                        self.emit("    neg eax");
                        DataType::Long
                    }
                }
            }

            Expr::Binary { op, left, right } => self.gen_binary_expr(*op, left, right),

            Expr::FnCall { name, args } => {
                self.gen_fn_call(name, args);
                self.call_return_type(name, args)
            }

            Expr::Field { .. } => {
                if let Some((name, indices, fields)) = Self::flatten_indexed_field_path(expr) {
                    let target = LValue {
                        name,
                        indices: Some(indices),
                        fields,
                    };
                    return self.gen_array_field(&target, None);
                }
                let Some((name, fields)) = Self::flatten_field_path(expr) else {
                    unreachable!("sema rejects a field access on a non-record")
                };
                let Some((loc, ty)) = self.resolve_field_path(&name, &fields) else {
                    unreachable!("sema resolved this field path")
                };
                self.gen_load_typed(&loc, &ty)
            }
        }
    }

    /// Generate code for a binary expression
    /// True for the six relational operators.
    fn is_comparison(op: BinaryOp) -> bool {
        matches!(
            op,
            BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge
        )
    }

    fn gen_binary_expr(&mut self, op: BinaryOp, left: &Expr, right: &Expr) -> DataType {
        // Track expression nesting depth and warn if too deep
        self.expr_depth += 1;
        if self.expr_depth == MAX_EXPR_DEPTH + 1 {
            eprintln!(
                "Warning: Expression nesting exceeds {} levels, stack overflow risk",
                MAX_EXPR_DEPTH
            );
        }

        let result_type = self.promote_types(self.expr_type(left), self.expr_type(right), op);

        // Handle string concatenation specially
        if result_type == DataType::String && op == BinaryOp::Add {
            // Evaluate left string (ptr in rax, len in rdx)
            self.gen_expr(left);
            // Save left string on stack using consistent sub rsp pattern (16-byte aligned)
            self.emit(&format!("    sub rsp, {}", STACK_TEMP_SPACE));
            self.emit("    mov QWORD PTR [rsp], rax"); // left ptr
            self.emit("    mov QWORD PTR [rsp + 8], rdx"); // left len

            // Evaluate right string (ptr in rax, len in rdx)
            self.gen_expr(right);
            // Now: right ptr in rax, right len in rdx

            // Call runtime string concat: rt_strcat(left_ptr, left_len, right_ptr, right_len)
            // Save right string temporarily
            self.emit("    mov r8, rax"); // right ptr
            self.emit("    mov r9, rdx"); // right len
            // Restore left string from stack
            self.emit("    mov rax, QWORD PTR [rsp]"); // left ptr
            self.emit("    mov rdx, QWORD PTR [rsp + 8]"); // left len
            self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));
            self.emit_arg_reg(0, "rax"); // left ptr
            self.emit_arg_reg(1, "rdx"); // left len
            self.emit_arg_reg(2, "r8"); // right ptr
            self.emit_arg_reg(3, "r9"); // right len
            self.emit("    call _rt_strcat");
            // Result: ptr in rax, len in rdx
            self.expr_depth -= 1;
            return DataType::String;
        }

        // Handle string comparison specially.
        //
        // Without this, comparing two strings fell through to the numeric path:
        // gen_coercion(String, String) is a no-op so nothing complained, and
        // emit_typed dropped through to the float branch, emitting
        // `ucomisd xmm0, xmm1` on registers that never held the operands. Every
        // relational operator on strings silently returned a constant.
        // (Guarded on the operand types, not `result_type`: a comparison always
        // promotes to Long, which is the type of its -1/0 result.)
        if Self::is_comparison(op)
            && self.expr_type(left) == DataType::String
            && self.expr_type(right) == DataType::String
        {
            // Evaluate left string (ptr in rax, len in rdx)
            self.gen_expr(left);
            self.emit(&format!("    sub rsp, {}", STACK_TEMP_SPACE));
            self.emit("    mov QWORD PTR [rsp], rax"); // left ptr
            self.emit("    mov QWORD PTR [rsp + 8], rdx"); // left len

            // Evaluate right string (ptr in rax, len in rdx)
            self.gen_expr(right);
            self.emit("    mov r8, rax"); // right ptr
            self.emit("    mov r9, rdx"); // right len
            self.emit("    mov rax, QWORD PTR [rsp]"); // left ptr
            self.emit("    mov rdx, QWORD PTR [rsp + 8]"); // left len
            self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));
            self.emit_arg_reg(0, "rax");
            self.emit_arg_reg(1, "rdx");
            self.emit_arg_reg(2, "r8");
            self.emit_arg_reg(3, "r9");
            self.emit("    call _rt_strcmp");

            // _rt_strcmp returns <0, 0 or >0 in eax, like memcmp. Turn that
            // into BASIC's -1 / 0 by testing it against zero with the signed
            // condition for this operator.
            let setcc = match op {
                BinaryOp::Eq => "sete",
                BinaryOp::Ne => "setne",
                BinaryOp::Lt => "setl",
                BinaryOp::Gt => "setg",
                BinaryOp::Le => "setle",
                BinaryOp::Ge => "setge",
                _ => unreachable!("guarded by is_comparison"),
            };
            self.emit("    test eax, eax");
            self.emit(&format!("    {} al", setcc));
            self.emit("    movzx eax, al");
            self.emit("    neg eax"); // BASIC true is -1
            self.expr_depth -= 1;
            return DataType::Long;
        }

        // For comparison/logical ops, we'll work in the promoted type but return Long
        let work_type = if matches!(
            op,
            BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::Lt
                | BinaryOp::Gt
                | BinaryOp::Le
                | BinaryOp::Ge
                | BinaryOp::And
                | BinaryOp::Or
                | BinaryOp::Xor
        ) {
            self.promote_types(self.expr_type(left), self.expr_type(right), BinaryOp::Add)
        } else {
            result_type
        };

        // Evaluate left operand and coerce to work type
        let left_type = self.gen_expr(left);
        self.gen_coercion(left_type, work_type);

        // Save left result - use 16 bytes to maintain 16-byte stack alignment
        // This ensures any function calls while evaluating right operand have aligned stack
        self.emit(&format!("    sub rsp, {}", STACK_TEMP_SPACE));
        if work_type.is_integer() {
            self.emit("    mov QWORD PTR [rsp], rax");
        } else if work_type == DataType::Single {
            self.emit("    movss DWORD PTR [rsp], xmm0");
        } else {
            self.emit("    movsd QWORD PTR [rsp], xmm0");
        }

        // Evaluate right operand and coerce to work type
        let right_type = self.gen_expr(right);
        self.gen_coercion(right_type, work_type);

        // Move right to secondary register/location and restore left
        if work_type.is_integer() {
            self.emit("    mov ecx, eax"); // right in ecx
            self.emit("    mov rax, QWORD PTR [rsp]"); // left in rax
        } else if work_type == DataType::Single {
            self.emit("    movss xmm1, xmm0"); // right in xmm1
            self.emit("    movss xmm0, DWORD PTR [rsp]"); // left in xmm0
        } else {
            self.emit("    movsd xmm1, xmm0"); // right in xmm1
            self.emit("    movsd xmm0, QWORD PTR [rsp]"); // left in xmm0
        }
        self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));

        // Generate operation
        match op {
            BinaryOp::Add => self.emit_typed(
                work_type,
                "    add eax, ecx",
                "    addss xmm0, xmm1",
                "    addsd xmm0, xmm1",
            ),
            BinaryOp::Sub => self.emit_typed(
                work_type,
                "    sub eax, ecx",
                "    subss xmm0, xmm1",
                "    subsd xmm0, xmm1",
            ),
            BinaryOp::Mul => self.emit_typed(
                work_type,
                "    imul eax, ecx",
                "    mulss xmm0, xmm1",
                "    mulsd xmm0, xmm1",
            ),
            BinaryOp::Div => {
                self.emit_cvt_to_double(work_type);
                if self.opts.checks {
                    // Test the bit pattern rather than comparing with ucomisd:
                    // doubling drops the sign bit, so the result is zero for
                    // both +0.0 and -0.0, and there is no unordered case to
                    // worry about.
                    self.emit("    movq r11, xmm1");
                    self.emit("    add r11, r11");
                    self.emit_check("jz", RtError::DivideByZero);
                }
                self.emit("    divsd xmm0, xmm1");
            }
            BinaryOp::IntDiv => {
                self.emit_cvt_float_to_int(work_type);
                self.emit_integer_divide_checks();
                self.emit("    cdq");
                self.emit("    idiv ecx");
            }
            BinaryOp::Mod => {
                self.emit_cvt_float_to_int(work_type);
                self.emit_integer_divide_checks();
                self.emit("    cdq");
                self.emit("    idiv ecx");
                self.emit("    mov eax, edx");
            }
            BinaryOp::Pow => {
                self.emit_cvt_to_double(work_type);
                self.emit_call_libc("pow");
            }
            BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::Lt
            | BinaryOp::Gt
            | BinaryOp::Le
            | BinaryOp::Ge => {
                // (signed_setcc, unsigned_setcc) - signed for integers, unsigned for floats
                let (signed, unsigned) = match op {
                    BinaryOp::Eq => ("sete", "sete"),
                    BinaryOp::Ne => ("setne", "setne"),
                    BinaryOp::Lt => ("setl", "setb"),
                    BinaryOp::Gt => ("setg", "seta"),
                    BinaryOp::Le => ("setle", "setbe"),
                    BinaryOp::Ge => ("setge", "setae"),
                    _ => unreachable!(),
                };
                self.emit_typed(
                    work_type,
                    "    cmp eax, ecx",
                    "    ucomiss xmm0, xmm1",
                    "    ucomisd xmm0, xmm1",
                );
                let setcc = if work_type.is_integer() {
                    signed
                } else {
                    unsigned
                };
                self.emit(&format!("    {} al", setcc));
                self.emit("    movzx eax, al");
                self.emit("    neg eax");
            }
            BinaryOp::And | BinaryOp::Or | BinaryOp::Xor => {
                self.emit_cvt_float_to_int(work_type);
                let instr = match op {
                    BinaryOp::And => "and",
                    BinaryOp::Or => "or",
                    BinaryOp::Xor => "xor",
                    _ => unreachable!(),
                };
                self.emit(&format!("    {} eax, ecx", instr));
            }
        }

        self.expr_depth -= 1;
        result_type
    }

    /// Evaluate a file-number expression ahead of a runtime call.
    ///
    /// Returns either an immediate (the common `#1` case) or a frame slot
    /// holding the value. Evaluating it *first*, into a slot, keeps a complex
    /// expression from clobbering argument registers that have already been
    /// loaded for the same call.
    fn gen_file_num(&mut self, e: &Expr) -> FileNum {
        if let Expr::Literal(Literal::Integer(n)) = e {
            return FileNum::Imm(*n);
        }
        let ty = self.gen_expr(e);
        self.gen_coercion(ty, DataType::Long);
        self.stack_offset -= 8;
        let loc = Loc::Frame(self.stack_offset);
        self.emit(&format!("    mov {}, eax", loc.at("DWORD PTR", 0)));
        FileNum::Slot(loc)
    }

    /// Place a previously evaluated file number in an argument register.
    fn emit_arg_file_num(&mut self, idx: usize, fnum: &FileNum) {
        match fnum {
            FileNum::Imm(n) => self.emit_arg_imm(idx, *n),
            FileNum::Slot(loc) => {
                let reg = Self::arg_reg(idx);
                self.emit(&format!("    movsxd {}, {}", reg, loc.at("DWORD PTR", 0)));
            }
        }
    }

    /// `MID$(s, start [, len]) = value` -- overwrite characters in place.
    ///
    /// The six arguments exceed Win64's four argument registers, so the last
    /// two go on the stack; `emit_arg_reg` handles only the register slots.
    fn gen_mid_assign(&mut self, target: &LValue, start: &Expr, len: Option<&Expr>, value: &Expr) {
        // Everything is evaluated into a temp block first, because each
        // evaluation clobbers the value registers.
        const SLOTS: i32 = 96; // 6 values, 16-byte aligned with room to spare
        self.emit(&format!("    sub rsp, {}", SLOTS));

        self.gen_read_lvalue(target);
        self.emit("    mov QWORD PTR [rsp], rax"); // target pointer
        self.emit("    mov QWORD PTR [rsp + 8], rdx"); // target length

        let t = self.gen_expr(start);
        self.gen_coercion(t, DataType::Long);
        self.emit("    movsxd rax, eax");
        self.emit("    mov QWORD PTR [rsp + 16], rax");

        match len {
            Some(e) => {
                let t = self.gen_expr(e);
                self.gen_coercion(t, DataType::Long);
                self.emit("    movsxd rax, eax");
            }
            // No length given: replace as much as the value provides.
            None => self.emit("    mov rax, 0x7FFFFFFF"),
        }
        self.emit("    mov QWORD PTR [rsp + 24], rax");

        self.gen_expr(value);
        self.emit("    mov QWORD PTR [rsp + 32], rax"); // source pointer
        self.emit("    mov QWORD PTR [rsp + 40], rdx"); // source length

        // Load the register arguments, then the stack ones on Win64.
        let regs = PlatformAbi::INT_ARG_REGS;
        for (i, off) in [0, 8, 16, 24].iter().enumerate() {
            if i < regs.len() {
                self.emit(&format!("    mov {}, QWORD PTR [rsp + {}]", regs[i], off));
            }
        }
        if regs.len() >= 6 {
            self.emit(&format!("    mov {}, QWORD PTR [rsp + 32]", regs[4]));
            self.emit(&format!("    mov {}, QWORD PTR [rsp + 40]", regs[5]));
            self.emit("    call _rt_mid_assign");
        } else {
            // Win64: the 5th and 6th arguments sit just above the 32-byte
            // shadow space, at [rsp+32] and [rsp+40] as seen by the caller.
            // The callee reads them at [rsp+40] and [rsp+48], since `call`
            // pushes a return address in between.
            self.emit("    mov r10, QWORD PTR [rsp + 32]");
            self.emit("    mov r11, QWORD PTR [rsp + 40]");
            self.emit("    sub rsp, 64");
            self.emit("    mov QWORD PTR [rsp + 32], r10");
            self.emit("    mov QWORD PTR [rsp + 40], r11");
            self.emit("    call _rt_mid_assign");
            self.emit("    add rsp, 64");
        }

        self.emit(&format!("    add rsp, {}", SLOTS));
    }

    /// Exchange two values, which sema has checked are the same type class.
    ///
    /// Both are read before either is written, so `SWAP A(I), A(J)` is correct
    /// even when the subscripts alias.
    fn gen_swap(&mut self, a: &LValue, b: &LValue) {
        let is_string = is_string_var(&a.name);

        // Read A into a temp.
        self.gen_read_lvalue(a);
        self.emit(&format!("    sub rsp, {}", STACK_TEMP_SPACE));
        if is_string {
            self.emit("    mov QWORD PTR [rsp], rax");
            self.emit("    mov QWORD PTR [rsp + 8], rdx");
        } else {
            self.emit("    movsd QWORD PTR [rsp], xmm0");
        }

        // Read B and store it into A.
        self.gen_read_lvalue(b);
        self.gen_store_lvalue(a);

        // Restore the saved A into B.
        if is_string {
            self.emit("    mov rax, QWORD PTR [rsp]");
            self.emit("    mov rdx, QWORD PTR [rsp + 8]");
        } else {
            self.emit("    movsd xmm0, QWORD PTR [rsp]");
        }
        self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));
        self.gen_store_lvalue(b);
    }

    /// Load an assignment target's current value, in the same registers
    /// `gen_expr` would leave it in.
    fn gen_read_lvalue(&mut self, target: &LValue) {
        let expr = match &target.indices {
            Some(indices) => Expr::ArrayAccess {
                name: target.name.clone(),
                indices: indices.clone(),
            },
            None => Expr::Variable(target.name.clone()),
        };
        let ty = self.gen_expr(&expr);
        if ty != DataType::String {
            // gen_store_lvalue expects a Double, as the runtime readers produce.
            self.gen_coercion(ty, DataType::Double);
        }
    }

    /// Copy the string in `rax`/`rdx` onto the heap.
    ///
    /// String assignment copies, so that mutating one variable is not visible
    /// through another, and so that a string constant's shared `.data` literal
    /// can never be written through.
    fn emit_string_copy(&mut self) {
        self.emit("    mov r10, rax");
        self.emit("    mov r11, rdx");
        self.emit_arg_reg(0, "r10");
        self.emit_arg_reg(1, "r11");
        self.emit("    call _rt_strdup");
    }

    /// Split `arr(i).f...` into its array name, subscripts and field path.
    fn flatten_indexed_field_path(expr: &Expr) -> Option<(String, Vec<Expr>, Vec<String>)> {
        let mut fields = Vec::new();
        let mut cur = expr;
        loop {
            match cur {
                Expr::Field { base, field } => {
                    fields.push(field.clone());
                    cur = base;
                }
                Expr::ArrayAccess { name, indices }
                | Expr::FnCall {
                    name,
                    args: indices,
                } => {
                    fields.reverse();
                    return Some((name.clone(), indices.clone(), fields));
                }
                _ => return None,
            }
        }
    }

    /// Split a field-access expression into its base variable and field path.
    fn flatten_field_path(expr: &Expr) -> Option<(String, Vec<String>)> {
        let mut fields = Vec::new();
        let mut cur = expr;
        loop {
            match cur {
                Expr::Field { base, field } => {
                    fields.push(field.clone());
                    cur = base;
                }
                Expr::Variable(name) => {
                    fields.reverse();
                    return Some((name.clone(), fields));
                }
                _ => return None,
            }
        }
    }

    /// Result type of a field access.
    fn field_expr_type(&self, expr: &Expr) -> DataType {
        let (name, fields) = match Self::flatten_indexed_field_path(expr) {
            Some((n, _, f)) => (n, f),
            None => match Self::flatten_field_path(expr) {
                Some(v) => v,
                None => return DataType::Double,
            },
        };
        let scope = match &self.current_proc {
            Some(p) => SemaScope::Proc(p.clone()),
            None => SemaScope::Module,
        };
        let Some(mut ty) = self.symbols.typed_var(&scope, &name).cloned() else {
            return DataType::Double;
        };
        for field in &fields {
            let TypeRef::Record(rec) = &ty else {
                return DataType::Double;
            };
            let Some(info) = self.symbols.records.get(&rec.to_uppercase()) else {
                return DataType::Double;
            };
            let Some(f) = info.field(field) else {
                return DataType::Double;
            };
            ty = f.ty.clone();
        }
        Self::data_type_of(&ty)
    }

    /// The value representation of a declared type.
    fn data_type_of(ty: &TypeRef) -> DataType {
        match ty {
            TypeRef::Integer => DataType::Integer,
            TypeRef::Long => DataType::Long,
            TypeRef::Single => DataType::Single,
            TypeRef::Double => DataType::Double,
            TypeRef::FixedString(_) => DataType::String,
            TypeRef::Record(_) => DataType::Double,
        }
    }

    /// Type of an array's elements, when it was declared `DIM a(n) AS T`.
    fn array_elem_type(&self, name: &str) -> Option<TypeRef> {
        let ty = self.typed_var(name)?;
        self.record_elem_words_of(name).is_some().then_some(ty)
    }

    /// Load or store a field of an array element.
    ///
    /// The element address is computed into `rax` and kept in `rcx`, and the
    /// field is reached at a fixed byte offset from it. Unlike a scalar record,
    /// this address is not known until run time, so it cannot go through `Loc`.
    fn gen_array_field(&mut self, target: &LValue, value: Option<&Expr>) -> DataType {
        let indices = target.indices.clone().unwrap_or_default();
        let Some(mut ty) = self.array_elem_type(&target.name) else {
            unreachable!("sema checked this is an array of records")
        };

        // Walk the field path for its byte offset and final type.
        let mut byte_offset = 0i32;
        for field in &target.fields {
            let TypeRef::Record(rec) = &ty else {
                unreachable!("sema checked the field path")
            };
            let info = self
                .symbols
                .records
                .get(&rec.to_uppercase())
                .expect("sema checked the type exists");
            let f = info.field(field).expect("sema checked the field exists");
            byte_offset += f.word * 8;
            ty = f.ty.clone();
        }

        match value {
            None => {
                self.gen_array_addr(&target.name, &indices);
                let loc = Loc::Frame(0); // placeholder, replaced below
                let _ = loc;
                self.emit(&format!("    add rax, {}", byte_offset));
                self.gen_load_indirect("rax", &ty)
            }
            Some(v) => {
                // Evaluate the value first, then the address, since computing
                // the address clobbers the value registers.
                let vt = self.gen_expr(v);
                if Self::data_type_of(&ty) == DataType::String {
                    self.emit_string_copy();
                    self.emit(&format!("    sub rsp, {}", STACK_TEMP_SPACE));
                    self.emit("    mov QWORD PTR [rsp], rax");
                    self.emit("    mov QWORD PTR [rsp + 8], rdx");
                } else {
                    self.gen_coercion(vt, DataType::Double);
                    self.emit(&format!("    sub rsp, {}", STACK_TEMP_SPACE));
                    self.emit("    movsd QWORD PTR [rsp], xmm0");
                }

                self.gen_array_addr(&target.name, &indices);
                self.emit(&format!("    add rax, {}", byte_offset));
                self.emit("    mov rcx, rax");

                if Self::data_type_of(&ty) == DataType::String {
                    self.emit("    mov rax, QWORD PTR [rsp]");
                    self.emit("    mov rdx, QWORD PTR [rsp + 8]");
                    self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));
                    self.emit("    mov QWORD PTR [rcx], rax");
                    self.emit("    mov QWORD PTR [rcx + 8], rdx");
                } else {
                    self.emit("    movsd xmm0, QWORD PTR [rsp]");
                    self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));
                    self.gen_coercion(DataType::Double, Self::data_type_of(&ty));
                    match ty {
                        TypeRef::Integer => self.emit("    mov WORD PTR [rcx], ax"),
                        TypeRef::Long => self.emit("    mov DWORD PTR [rcx], eax"),
                        TypeRef::Single => self.emit("    movss DWORD PTR [rcx], xmm0"),
                        _ => self.emit("    movsd QWORD PTR [rcx], xmm0"),
                    }
                }
                DataType::Double
            }
        }
    }

    /// Load a scalar of the given declared type from the address in `reg`.
    fn gen_load_indirect(&mut self, reg: &str, ty: &TypeRef) -> DataType {
        match ty {
            TypeRef::Integer => {
                self.emit(&format!("    movsx eax, WORD PTR [{}]", reg));
                DataType::Integer
            }
            TypeRef::Long => {
                self.emit(&format!("    mov eax, DWORD PTR [{}]", reg));
                DataType::Long
            }
            TypeRef::Single => {
                self.emit(&format!("    movss xmm0, DWORD PTR [{}]", reg));
                DataType::Single
            }
            TypeRef::Double => {
                self.emit(&format!("    movsd xmm0, QWORD PTR [{}]", reg));
                DataType::Double
            }
            TypeRef::FixedString(_) => {
                self.emit(&format!("    mov rcx, {}", reg));
                self.emit("    mov rax, QWORD PTR [rcx]");
                self.emit("    mov rdx, QWORD PTR [rcx + 8]");
                DataType::String
            }
            TypeRef::Record(_) => DataType::Double,
        }
    }

    /// Find an already-allocated variable slot, without creating one.
    fn lookup_var(&self, name: &str) -> Option<&VarInfo> {
        if self.current_proc.is_some() {
            if let Some(info) = self.proc_vars.get(name) {
                return Some(info);
            }
        }
        self.vars.get(name)
    }

    /// Whether `name` already has ordinary variable storage.
    ///
    /// A parameter declared `N AS INTEGER` is spilled into a normal frame slot
    /// by the prologue, so it must not also be treated as typed storage; a
    /// record parameter is the opposite, and is registered in `record_vars`.
    fn has_plain_slot(&self, name: &str) -> bool {
        (self.current_proc.is_some() && self.proc_vars.contains_key(name))
            || self.vars.contains_key(name)
    }

    /// Typed storage for a variable declared with `AS`, if that is where it
    /// actually lives.
    fn typed_storage(&mut self, name: &str) -> Option<(Loc, TypeRef)> {
        if self.has_plain_slot(name) && self.record_loc_of(name).is_none() {
            return None;
        }
        let ty = self.typed_var(name)?;
        let loc = self.get_record_loc(name, &ty);
        Some((loc, ty))
    }

    /// Declared type of a variable, when it was given one with `DIM ... AS`.
    fn typed_var(&self, name: &str) -> Option<TypeRef> {
        if let Some(t) = self.proc_types.get(name) {
            return Some(t.clone());
        }
        let scope = match &self.current_proc {
            Some(p) => SemaScope::Proc(p.clone()),
            None => SemaScope::Module,
        };
        self.symbols.typed_var(&scope, name).cloned()
    }

    /// Resolve `base.field...` to a storage location and the field's type.
    ///
    /// Records are laid out as consecutive 8-byte words, so a field is reached
    /// the same way a variable is: a base location plus a word offset. That
    /// keeps every existing load and store site working unchanged.
    fn resolve_field_path(&mut self, name: &str, fields: &[String]) -> Option<(Loc, TypeRef)> {
        let mut ty = self.typed_var(name)?;
        let mut loc = self.get_record_loc(name, &ty);

        for field in fields {
            let TypeRef::Record(rec) = &ty else {
                return None;
            };
            let info = self.symbols.records.get(&rec.to_uppercase())?;
            let f = info.field(field)?;
            loc = loc.offset_words(f.word);
            ty = f.ty.clone();
        }
        Some((loc, ty))
    }

    /// Storage for a variable declared with `DIM ... AS`, allocating it the
    /// first time it is seen.
    fn get_record_loc(&mut self, name: &str, ty: &TypeRef) -> Loc {
        if let Some(loc) = self.record_loc_of(name) {
            return loc;
        }
        let words = self.symbols.type_words(ty);

        // A record is local only when the current procedure actually declares
        // it. Deciding on `current_proc.is_some()` alone gave a frame slot to a
        // module-level record merely *referred to* from inside a procedure.
        if self.declared_in_current_proc(name) {
            self.stack_offset -= 8 * words;
            let loc = Loc::Frame(self.stack_offset);
            self.proc_record_vars.insert(name.to_string(), loc.clone());
            return loc;
        }

        let loc = Loc::Global(format!("_rec_{}", mangle(name)));
        self.record_vars.insert(name.to_string(), loc.clone());
        self.record_globals.insert(name.to_string(), words);
        loc
    }

    /// Where a typed variable already lives, preferring the current
    /// procedure's own declarations over module-level ones.
    fn record_loc_of(&self, name: &str) -> Option<Loc> {
        if self.current_proc.is_some() {
            if let Some(loc) = self.proc_record_vars.get(name) {
                return Some(loc.clone());
            }
        }
        self.record_vars.get(name).cloned()
    }

    /// Element size in words of an array of records, with the same preference.
    fn record_elem_words_of(&self, name: &str) -> Option<i32> {
        let upper = name.to_uppercase();
        if self.current_proc.is_some() {
            if let Some(w) = self.proc_record_elem_words.get(&upper) {
                return Some(*w);
            }
        }
        self.record_elem_words.get(&upper).copied()
    }

    /// Whether the current procedure declares `name` itself, as a typed
    /// parameter or a local `DIM ... AS`.
    fn declared_in_current_proc(&self, name: &str) -> bool {
        let Some(proc) = &self.current_proc else {
            return false;
        };
        self.proc_types.contains_key(name)
            || self
                .symbols
                .typed_var_in(&SemaScope::Proc(proc.clone()), name)
                .is_some()
    }

    /// Load a scalar of the given declared type from `loc`.
    fn gen_load_typed(&mut self, loc: &Loc, ty: &TypeRef) -> DataType {
        match ty {
            TypeRef::Integer => {
                self.emit(&format!("    movsx eax, {}", loc.at("WORD PTR", 0)));
                DataType::Integer
            }
            TypeRef::Long => {
                self.emit(&format!("    mov eax, {}", loc.at("DWORD PTR", 0)));
                DataType::Long
            }
            TypeRef::Single => {
                self.emit(&format!("    movss xmm0, {}", loc.at("DWORD PTR", 0)));
                DataType::Single
            }
            TypeRef::Double => {
                self.emit(&format!("    movsd xmm0, {}", loc.q(0)));
                DataType::Double
            }
            TypeRef::FixedString(_) => {
                self.emit(&format!("    mov rax, {}", loc.q(0)));
                self.emit(&format!("    mov rdx, {}", loc.q(1)));
                DataType::String
            }
            // A whole record has no scalar value; sema rejects using one here.
            TypeRef::Record(_) => DataType::Double,
        }
    }

    /// Store a scalar of the given declared type into `loc`.
    fn gen_store_typed(&mut self, loc: &Loc, ty: &TypeRef, value_type: DataType) {
        match ty {
            TypeRef::Integer => {
                self.gen_coercion(value_type, DataType::Integer);
                self.emit(&format!("    mov {}, ax", loc.at("WORD PTR", 0)));
            }
            TypeRef::Long => {
                self.gen_coercion(value_type, DataType::Long);
                self.emit(&format!("    mov {}, eax", loc.at("DWORD PTR", 0)));
            }
            TypeRef::Single => {
                self.gen_coercion(value_type, DataType::Single);
                self.emit(&format!("    movss {}, xmm0", loc.at("DWORD PTR", 0)));
            }
            TypeRef::Double => {
                self.gen_coercion(value_type, DataType::Double);
                self.emit(&format!("    movsd {}, xmm0", loc.q(0)));
            }
            TypeRef::FixedString(_) => {
                self.emit_string_copy();
                self.emit(&format!("    mov {}, rax", loc.q(0)));
                self.emit(&format!("    mov {}, rdx", loc.q(1)));
            }
            TypeRef::Record(_) => {}
        }
    }

    /// Store a freshly produced value into an assignment target.
    ///
    /// The value is expected where `gen_expr` leaves it: `rax`/`rdx` for a
    /// string, `xmm0` for a number. For an array element the address has to be
    /// computed *after* the value exists, since computing it clobbers `rax`, so
    /// the value is parked on the stack across the address calculation.
    fn gen_store_lvalue(&mut self, target: &LValue) {
        // A record field is addressed by a base location plus a word offset,
        // so it stores exactly like a scalar.
        if !target.fields.is_empty() {
            if let Some((loc, ty)) = self.resolve_field_path(&target.name, &target.fields) {
                self.gen_store_typed(&loc, &ty, DataType::Double);
                return;
            }
        }

        let is_string = is_string_var(&target.name);
        if is_string {
            self.emit_string_copy();
        }

        let Some(indices) = &target.indices else {
            let loc = self.get_var_loc(&target.name);
            if is_string {
                self.emit(&format!("    mov {}, rax", loc.q(0)));
                self.emit(&format!("    mov {}, rdx", loc.q(1)));
            } else {
                self.emit(&format!("    movsd {}, xmm0", loc.q(0)));
            }
            return;
        };

        // Park the value, compute the element address, then store.
        self.emit(&format!("    sub rsp, {}", STACK_TEMP_SPACE));
        if is_string {
            self.emit("    mov QWORD PTR [rsp], rax");
            self.emit("    mov QWORD PTR [rsp + 8], rdx");
        } else {
            self.emit("    movsd QWORD PTR [rsp], xmm0");
        }

        let indices = indices.clone();
        self.gen_array_addr(&target.name, &indices);
        self.emit("    mov rcx, rax");

        if is_string {
            self.emit("    mov rax, QWORD PTR [rsp]");
            self.emit("    mov rdx, QWORD PTR [rsp + 8]");
            self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));
            self.emit("    mov QWORD PTR [rcx], rax");
            self.emit("    mov QWORD PTR [rcx + 8], rdx");
        } else {
            self.emit("    movsd xmm0, QWORD PTR [rsp]");
            self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));
            // Narrow to the element's declared type, as a normal store does.
            let elem_type = DataType::from_suffix(&target.name);
            self.gen_coercion(DataType::Double, elem_type);
            match elem_type {
                DataType::Integer => self.emit("    mov WORD PTR [rcx], ax"),
                DataType::Long => self.emit("    mov DWORD PTR [rcx], eax"),
                DataType::Single => self.emit("    movss DWORD PTR [rcx], xmm0"),
                DataType::Double => self.emit("    movsd QWORD PTR [rcx], xmm0"),
                DataType::String => unreachable!("handled above"),
            }
        }
    }

    /// Emit `PRINT USING`.
    ///
    /// The format is parsed at compile time (see the `using` module), so this
    /// emits a straight-line sequence: literal runs go to the ordinary string
    /// printer, and each field calls one of two small runtime helpers. Values
    /// are consumed in order by the fields that take one; if the values run out
    /// the format stops there, and if values remain the format restarts, which
    /// is what GW-BASIC does.
    fn gen_print_using(&mut self, fmt: &Expr, items: &[PrintItem]) {
        let Expr::Literal(Literal::String(format)) = fmt else {
            unreachable!("sema requires a literal PRINT USING format")
        };
        let parts = using::parse(format);

        let values: Vec<&Expr> = items
            .iter()
            .filter_map(|i| match i {
                PrintItem::Expr(e) => Some(e),
                _ => None,
            })
            .collect();

        let fields = parts.iter().filter(|p| p.consumes_value()).count();
        if fields == 0 || values.is_empty() {
            // No fields to fill: the format is just text.
            for part in &parts {
                if let using::UsingPart::Literal(text) = part {
                    self.emit_literal_string(text);
                }
            }
            return;
        }

        let mut next = 0usize;
        while next < values.len() {
            let before = next;
            for part in &parts {
                match part {
                    using::UsingPart::Literal(text) => self.emit_literal_string(text),
                    using::UsingPart::Num {
                        width,
                        decimals,
                        flags,
                    } => {
                        if next >= values.len() {
                            return;
                        }
                        let value = values[next];
                        next += 1;
                        let ty = self.gen_expr(value);
                        self.gen_coercion(ty, DataType::Double);
                        self.emit_arg_imm(0, *width as i64);
                        self.emit_arg_imm(1, *decimals as i64);
                        self.emit_arg_imm(2, *flags);
                        self.emit("    call _rt_print_using_num");
                    }
                    using::UsingPart::Str { kind, width } => {
                        if next >= values.len() {
                            return;
                        }
                        let value = values[next];
                        next += 1;
                        self.gen_expr(value);
                        // rax = ptr, rdx = len from gen_expr
                        self.emit("    mov r10, rax");
                        self.emit("    mov r11, rdx");
                        self.emit_arg_reg(0, "r10");
                        self.emit_arg_reg(1, "r11");
                        let w = match kind {
                            using::StrFieldKind::Whole => 0,
                            using::StrFieldKind::First => 1,
                            using::StrFieldKind::Fixed => *width as i64,
                        };
                        self.emit_arg_imm(2, w);
                        self.emit("    call _rt_print_using_str");
                    }
                }
            }
            // A format with fields must consume at least one value per pass,
            // otherwise restarting would loop forever.
            if next == before {
                return;
            }
        }
    }

    /// Print a literal string constant.
    fn emit_literal_string(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let idx = self.add_string_literal(text);
        self.emit_arg_lea(0, &format!("[rip + _str_{}]", idx));
        self.emit_arg_imm(1, text.len() as i64);
        self.emit("    call _rt_print_string");
    }

    /// Emit one CASE alternative: jump to `body_label` when it matches.
    ///
    /// The selector was evaluated once, into the frame slot at `temp_offset`.
    fn gen_case_clause(
        &mut self,
        clause: &CaseClause,
        temp_offset: i32,
        is_string: bool,
        body_label: &str,
    ) {
        if is_string {
            return self.gen_case_clause_string(clause, temp_offset, body_label);
        }
        let sel = format!("QWORD PTR [rbp + {}]", temp_offset);
        match clause {
            CaseClause::Value(e) => {
                let t = self.gen_expr(e);
                self.gen_coercion(t, DataType::Double);
                self.emit(&format!("    movsd xmm1, {}", sel));
                self.emit("    ucomisd xmm1, xmm0");
                self.emit(&format!("    je {}", body_label));
            }
            CaseClause::Range(lo, hi) => {
                // Inclusive at both ends. The low bound is tested first, and a
                // failure skips the high test.
                let skip = self.new_label("caseskip");
                let t = self.gen_expr(lo);
                self.gen_coercion(t, DataType::Double);
                self.emit(&format!("    movsd xmm1, {}", sel));
                self.emit("    ucomisd xmm1, xmm0");
                self.emit(&format!("    jb {}", skip));
                let t = self.gen_expr(hi);
                self.gen_coercion(t, DataType::Double);
                self.emit(&format!("    movsd xmm1, {}", sel));
                self.emit("    ucomisd xmm1, xmm0");
                self.emit(&format!("    jbe {}", body_label));
                self.emit_label(&skip);
            }
            CaseClause::Compare(op, e) => {
                let t = self.gen_expr(e);
                self.gen_coercion(t, DataType::Double);
                self.emit(&format!("    movsd xmm1, {}", sel));
                self.emit("    ucomisd xmm1, xmm0");
                // Unsigned conditions, since ucomisd sets the carry flag.
                let cc = match op {
                    BinaryOp::Eq => "je",
                    BinaryOp::Ne => "jne",
                    BinaryOp::Lt => "jb",
                    BinaryOp::Gt => "ja",
                    BinaryOp::Le => "jbe",
                    BinaryOp::Ge => "jae",
                    _ => unreachable!("the parser only builds comparisons here"),
                };
                self.emit(&format!("    {} {}", cc, body_label));
            }
        }
    }

    /// A CASE alternative on a string selector.
    ///
    /// Comparison goes through `_rt_strcmp`, the same helper the relational
    /// operators use, so ordering is consistent between `CASE "a" TO "m"` and
    /// `IF S$ >= "a" AND S$ <= "m"`.
    fn gen_case_clause_string(&mut self, clause: &CaseClause, temp: i32, body_label: &str) {
        // Compare the selector against the expression, leaving memcmp-style
        // ordering in eax.
        let compare = |s: &mut Self, e: &Expr| {
            s.gen_expr(e);
            s.emit("    mov r10, rax");
            s.emit("    mov r11, rdx");
            s.emit(&format!("    mov rax, QWORD PTR [rbp + {}]", temp));
            s.emit(&format!("    mov rdx, QWORD PTR [rbp + {}]", temp + 8));
            s.emit_arg_reg(0, "rax");
            s.emit_arg_reg(1, "rdx");
            s.emit_arg_reg(2, "r10");
            s.emit_arg_reg(3, "r11");
            s.emit("    call _rt_strcmp");
            s.emit("    test eax, eax");
        };

        match clause {
            CaseClause::Value(e) => {
                compare(self, e);
                self.emit(&format!("    je {}", body_label));
            }
            CaseClause::Range(lo, hi) => {
                let skip = self.new_label("caseskip");
                compare(self, lo);
                self.emit(&format!("    jl {}", skip));
                compare(self, hi);
                self.emit(&format!("    jle {}", body_label));
                self.emit_label(&skip);
            }
            CaseClause::Compare(op, e) => {
                compare(self, e);
                let cc = match op {
                    BinaryOp::Eq => "je",
                    BinaryOp::Ne => "jne",
                    BinaryOp::Lt => "jl",
                    BinaryOp::Gt => "jg",
                    BinaryOp::Le => "jle",
                    BinaryOp::Ge => "jge",
                    _ => unreachable!("the parser only builds comparisons here"),
                };
                self.emit(&format!("    {} {}", cc, body_label));
            }
        }
    }

    /// Emit one WRITE value: strings are quoted, numbers printed as usual.
    fn gen_write_expr(&mut self, expr: &Expr) {
        if self.expr_type(expr) == DataType::String {
            self.emit_arg_imm(0, ASCII_QUOTE);
            self.emit("    call _rt_print_char");
            self.gen_print_expr(expr);
            self.emit_arg_imm(0, ASCII_QUOTE);
            self.emit("    call _rt_print_char");
        } else {
            self.gen_print_expr(expr);
        }
    }

    /// WRITE # equivalent of [`Self::gen_write_expr`].
    fn gen_write_expr_to_file(&mut self, expr: &Expr, file_num: &FileNum) {
        if self.expr_type(expr) == DataType::String {
            self.emit_arg_file_num(0, file_num);
            self.emit_arg_imm(1, ASCII_QUOTE);
            self.emit("    call _rt_file_print_char");
            self.gen_print_expr_to_file(expr, file_num);
            self.emit_arg_file_num(0, file_num);
            self.emit_arg_imm(1, ASCII_QUOTE);
            self.emit("    call _rt_file_print_char");
        } else {
            self.gen_print_expr_to_file(expr, file_num);
        }
    }

    fn gen_print_expr(&mut self, expr: &Expr) {
        // TAB() and SPC() position the cursor rather than producing a value,
        // so they are emitted for their effect and nothing is printed after.
        if let Expr::FnCall { name, args } = expr {
            let upper = name.to_uppercase();
            if upper == "TAB" || upper == "SPC" {
                self.gen_fn_call(&upper, args);
                return;
            }
        }

        // Check the expression type first
        let expected_type = self.expr_type(expr);

        if expected_type == DataType::String {
            // String expression - evaluate and print as string
            // gen_expr for strings puts ptr in rax, len in rdx
            self.gen_expr(expr);
            self.emit_arg_reg(0, "rax"); // ptr
            self.emit_arg_reg(1, "rdx"); // len
            self.emit("    call _rt_print_string");
        } else {
            // Numeric expression - evaluate and convert to double for printing.
            // A Single is printed via its own helper, which round-trips against
            // 32-bit precision: widening 3.14159! to a double and printing all
            // the digits that survive would show 3.141590118408203.
            let expr_type = self.gen_expr(expr);
            self.gen_coercion(expr_type, DataType::Double);
            if expr_type == DataType::Single {
                self.emit("    call _rt_print_single");
            } else {
                self.emit("    call _rt_print_float");
            }
        }
    }

    fn gen_print_expr_to_file(&mut self, expr: &Expr, file_num: &FileNum) {
        // Check the expression type first
        let expected_type = self.expr_type(expr);

        if expected_type == DataType::String {
            // String expression - evaluate and print as string
            // gen_expr for strings puts ptr in rax, len in rdx
            self.gen_expr(expr);
            // On Win64, arg1=rdx, arg2=r8. Must save rdx (len) to r8 BEFORE
            // clobbering rdx with ptr. Order matters to avoid register conflicts.
            self.emit_arg_reg(2, "rdx"); // len → r8 (on Win64) or rdx (on SysV, no-op)
            self.emit_arg_reg(1, "rax"); // ptr → rdx (on Win64) or rsi (on SysV)
            self.emit_arg_file_num(0, file_num); // file_num → rcx or rdi
            self.emit("    call _rt_file_print_string");
        } else {
            // Numeric expression - evaluate and convert to double for printing
            let expr_type = self.gen_expr(expr);
            self.gen_coercion(expr_type, DataType::Double);
            self.emit_arg_file_num(0, file_num);
            self.emit("    call _rt_file_print_float");
        }
    }

    fn gen_fn_call(&mut self, name: &str, args: &[Expr]) {
        let upper_name = name.to_uppercase();

        // Table-driven: libc math functions (SIN, COS, TAN, ATN, EXP, LOG)
        if let Some(libc_fn) = LIBC_MATH_FNS.get(upper_name.as_str()) {
            let arg_type = self.gen_expr(&args[0]);
            self.gen_coercion(arg_type, DataType::Double);
            // LOG is undefined at and below zero; libc would quietly return
            // -inf or NaN.
            if upper_name == "LOG" {
                self.emit_domain_check("jbe");
            }
            self.emit_call_libc(libc_fn);
            return;
        }

        // Table-driven: inline math functions (SQR, INT, FIX)
        if let Some(instr) = INLINE_MATH_FNS.get(upper_name.as_str()) {
            let arg_type = self.gen_expr(&args[0]);
            self.gen_coercion(arg_type, DataType::Double);
            // sqrtsd of a negative operand yields NaN, which then printed as a
            // huge meaningless integer.
            if upper_name == "SQR" {
                self.emit_domain_check("jb");
            }
            self.emit(&format!("    {}", instr));
            return;
        }

        // Complex built-in functions
        match upper_name.as_str() {
            "ABS" => {
                let arg_type = self.gen_expr(&args[0]);
                self.gen_coercion(arg_type, DataType::Double);
                self.emit("    mov rax, 0x7FFFFFFFFFFFFFFF");
                self.emit("    movq xmm1, rax");
                self.emit("    andpd xmm0, xmm1");
                // ABS preserves its argument's type: narrow back so the value
                // matches what call_return_type promises.
                self.gen_coercion(DataType::Double, Self::abs_result_type(arg_type));
            }
            "SGN" => {
                let arg_type = self.gen_expr(&args[0]);
                self.gen_coercion(arg_type, DataType::Double);
                self.emit("    xorpd xmm1, xmm1");
                self.emit("    ucomisd xmm0, xmm1");
                self.emit("    seta al");
                self.emit("    movzx eax, al");
                self.emit("    setb cl");
                self.emit("    movzx ecx, cl");
                self.emit("    sub eax, ecx");
                self.emit("    cvtsi2sd xmm0, eax");
            }
            "RND" => {
                if !args.is_empty() {
                    let arg_type = self.gen_expr(&args[0]);
                    self.gen_coercion(arg_type, DataType::Double);
                }
                self.emit("    call _rt_rnd");
            }
            "LEN" => {
                self.gen_expr(&args[0]);
                // String length is in rdx after gen_expr
                self.emit("    mov eax, edx"); // LEN returns Long (integer)
            }
            "LEFT$" => {
                // _rt_left(ptr, len, count)
                // Use callee-saved registers to preserve string across count evaluation
                self.emit("    push r12");
                self.emit("    push r13");
                self.gen_expr(&args[0]); // string: rax=ptr, rdx=len
                self.emit("    mov r12, rax"); // save ptr
                self.emit("    mov r13, rdx"); // save len
                let count_type = self.gen_expr(&args[1]); // count - safe now
                let arg2 = Self::arg_reg(2);
                if count_type.is_integer() {
                    self.emit(&format!("    movsxd {}, eax", arg2));
                } else {
                    self.emit(&format!("    cvttsd2si {}, xmm0", arg2));
                }
                self.emit_arg_reg(0, "r12"); // ptr
                self.emit_arg_reg(1, "r13"); // len
                self.emit("    call _rt_left");
                self.emit("    pop r13");
                self.emit("    pop r12");
            }
            "RIGHT$" => {
                // _rt_right(ptr, len, count)
                // Use callee-saved registers to preserve string across count evaluation
                self.emit("    push r12");
                self.emit("    push r13");
                self.gen_expr(&args[0]);
                self.emit("    mov r12, rax"); // save ptr
                self.emit("    mov r13, rdx"); // save len
                let count_type = self.gen_expr(&args[1]); // count - safe now
                let arg2 = Self::arg_reg(2);
                if count_type.is_integer() {
                    self.emit(&format!("    movsxd {}, eax", arg2));
                } else {
                    self.emit(&format!("    cvttsd2si {}, xmm0", arg2));
                }
                self.emit_arg_reg(0, "r12"); // ptr
                self.emit_arg_reg(1, "r13"); // len
                self.emit("    call _rt_right");
                self.emit("    pop r13");
                self.emit("    pop r12");
            }
            "MID$" => {
                // _rt_mid(ptr, len, start, count)
                // Use callee-saved registers to preserve string across position/count evaluation
                self.emit("    push r12");
                self.emit("    push r13");
                self.emit("    push r14");
                self.gen_expr(&args[0]);
                self.emit("    mov r12, rax"); // save ptr
                self.emit("    mov r13, rdx"); // save len
                let pos_type = self.gen_expr(&args[1]); // start position - safe now
                if pos_type.is_integer() {
                    self.emit("    movsxd r14, eax"); // save start
                } else {
                    self.emit("    cvttsd2si r14, xmm0"); // save start
                }
                let arg3 = Self::arg_reg(3);
                if args.len() > 2 {
                    let len_type = self.gen_expr(&args[2]); // count - safe now
                    if len_type.is_integer() {
                        self.emit(&format!("    movsxd {}, eax", arg3));
                    } else {
                        self.emit(&format!("    cvttsd2si {}, xmm0", arg3));
                    }
                } else {
                    self.emit(&format!("    mov {}, -1", arg3)); // rest of string
                }
                self.emit_arg_reg(0, "r12"); // ptr
                self.emit_arg_reg(1, "r13"); // len
                self.emit_arg_reg(2, "r14"); // start
                self.emit("    call _rt_mid");
                self.emit("    pop r14");
                self.emit("    pop r13");
                self.emit("    pop r12");
            }
            "INSTR" => {
                // INSTR([start,] haystack$, needle$)
                // Args: haystack_ptr, haystack_len, needle_ptr, needle_len, start
                let (start_arg, hay_arg, needle_arg) = if args.len() == 3 {
                    (Some(&args[0]), &args[1], &args[2])
                } else {
                    (None, &args[0], &args[1])
                };

                // Evaluate and save start position
                self.emit("    push rbx"); // save callee-saved reg
                if let Some(start) = start_arg {
                    let start_type = self.gen_expr(start);
                    if start_type.is_integer() {
                        self.emit("    movsxd rbx, eax");
                    } else {
                        self.emit("    cvttsd2si rbx, xmm0");
                    }
                } else {
                    self.emit("    mov rbx, 1");
                }

                // Evaluate haystack and save
                self.emit("    push r12");
                self.emit("    push r13");
                self.gen_expr(hay_arg);
                self.emit("    mov r12, rax"); // haystack ptr
                self.emit("    mov r13, rdx"); // haystack len

                // Evaluate needle
                self.gen_expr(needle_arg);
                // rax = needle ptr, rdx = needle len

                // Set up arguments based on ABI
                // SysV: rdi=hay_ptr, rsi=hay_len, rdx=needle_ptr, rcx=needle_len, r8=start
                // Win64: rcx=hay_ptr, rdx=hay_len, r8=needle_ptr, r9=needle_len, [rsp+32]=start
                #[cfg(windows)]
                {
                    self.emit(&format!("    sub rsp, {}", WIN64_5ARG_STACK_SPACE));
                    self.emit(&format!(
                        "    mov QWORD PTR [rsp + {}], rbx",
                        WIN64_5TH_ARG_OFFSET
                    )); // 5th arg: start
                    self.emit("    mov r9, rdx"); // needle len
                    self.emit("    mov r8, rax"); // needle ptr
                    self.emit("    mov rdx, r13"); // haystack len
                    self.emit("    mov rcx, r12"); // haystack ptr
                    self.emit("    call _rt_instr");
                    self.emit(&format!("    add rsp, {}", WIN64_5ARG_STACK_SPACE));
                }
                #[cfg(not(windows))]
                {
                    self.emit("    mov r8, rbx"); // start
                    self.emit("    mov rcx, rdx"); // needle len
                    self.emit("    mov rdx, rax"); // needle ptr
                    self.emit("    mov rsi, r13"); // haystack len
                    self.emit("    mov rdi, r12"); // haystack ptr
                    self.emit("    call _rt_instr");
                }

                self.emit("    pop r13");
                self.emit("    pop r12");
                self.emit("    pop rbx");
                // Result is in rax
                self.emit("    mov eax, eax"); // zero-extend/truncate to 32-bit
            }
            "ASC" => {
                self.gen_expr(&args[0]);
                self.emit("    movzx eax, BYTE PTR [rax]");
                // ASC returns integer in eax (Long type)
            }
            "CHR$" => {
                // _rt_chr(char_code)
                let arg_type = self.gen_expr(&args[0]);
                let arg0 = Self::arg_reg(0);
                if arg_type.is_integer() {
                    self.emit(&format!("    movsxd {}, eax", arg0));
                } else {
                    self.emit(&format!("    cvttsd2si {}, xmm0", arg0));
                }
                self.emit("    call _rt_chr");
            }
            "VAL" => {
                // _rt_val(ptr, len)
                self.gen_expr(&args[0]);
                self.emit_arg_reg(0, "rax"); // ptr
                self.emit_arg_reg(1, "rdx"); // len
                self.emit("    call _rt_val");
            }
            "STR$" => {
                let arg_type = self.gen_expr(&args[0]);
                // STR$ expects double in xmm0
                self.gen_coercion(arg_type, DataType::Double);
                self.emit("    call _rt_str");
            }
            "CINT" | "CLNG" => {
                let arg_type = self.gen_expr(&args[0]);
                // Convert to integer with rounding - result in eax
                // BASIC CINT/CLNG round to nearest integer (not truncate)
                if !arg_type.is_integer() {
                    // Coerce to Double first (handles Single -> Double conversion)
                    self.gen_coercion(arg_type, DataType::Double);
                    // Use cvtsd2si which rounds using MXCSR mode (default: round-to-nearest)
                    self.emit("    cvtsd2si eax, xmm0");
                }
                // Result is integer (Long) in eax
            }
            "CSNG" => {
                let arg_type = self.gen_expr(&args[0]);
                self.gen_coercion(arg_type, DataType::Single);
            }
            "CDBL" => {
                let arg_type = self.gen_expr(&args[0]);
                self.gen_coercion(arg_type, DataType::Double);
            }
            "TIMER" => {
                self.emit("    call _rt_timer");
            }
            // String builders. These allocate, so the result outlives the call.
            "SPACE$" => {
                let t = self.gen_expr(&args[0]);
                self.gen_coercion(t, DataType::Long);
                self.emit("    movsxd rax, eax");
                self.emit_arg_reg(0, "rax");
                self.emit("    call _rt_space");
            }
            "STRING$" => {
                // STRING$(n, ch) takes either a character code or a string
                // whose first character is used.
                let t = self.gen_expr(&args[0]);
                self.gen_coercion(t, DataType::Long);
                self.emit("    movsxd rax, eax");
                self.emit("    push rax");
                self.emit("    sub rsp, 8"); // keep rsp 16-byte aligned
                let ct = self.gen_expr(&args[1]);
                if ct == DataType::String {
                    self.emit("    movzx eax, BYTE PTR [rax]");
                } else {
                    self.gen_coercion(ct, DataType::Long);
                }
                self.emit("    mov r10d, eax");
                self.emit("    add rsp, 8");
                self.emit("    pop rax");
                self.emit_arg_reg(0, "rax");
                self.emit_arg_reg(1, "r10");
                self.emit("    call _rt_string_n");
            }
            // Trimming and case conversion.
            "LTRIM$" | "RTRIM$" | "UCASE$" | "LCASE$" => {
                self.gen_expr(&args[0]);
                self.emit("    mov r10, rax");
                self.emit("    mov r11, rdx");
                self.emit_arg_reg(0, "r10");
                self.emit_arg_reg(1, "r11");
                let rt = match upper_name.as_str() {
                    "LTRIM$" => "_rt_ltrim",
                    "RTRIM$" => "_rt_rtrim",
                    "UCASE$" => "_rt_ucase",
                    _ => "_rt_lcase",
                };
                self.emit(&format!("    call {}", rt));
            }
            // Radix conversions.
            "HEX$" | "OCT$" => {
                let t = self.gen_expr(&args[0]);
                self.gen_coercion(t, DataType::Long);
                self.emit("    movsxd rax, eax");
                self.emit_arg_reg(0, "rax");
                let rt = if upper_name == "HEX$" {
                    "_rt_hex"
                } else {
                    "_rt_oct"
                };
                self.emit(&format!("    call {}", rt));
            }
            // Array bounds. The descriptor stores each dimension's element
            // count, so UBOUND is that minus one and LBOUND is always 0.
            "LBOUND" | "UBOUND" => {
                let arr = match &args[0] {
                    Expr::Variable(n) => n.to_uppercase(),
                    Expr::FnCall { name, .. } => name.to_uppercase(),
                    Expr::ArrayAccess { name, .. } => name.to_uppercase(),
                    _ => unreachable!("sema requires an array name here"),
                };
                let dim = match args.get(1) {
                    Some(Expr::Literal(Literal::Integer(n))) => *n as i32,
                    _ => 1,
                };
                if upper_name == "LBOUND" {
                    let base = self.symbols.option_base;
                    self.emit(&format!("    mov eax, {}", base));
                } else {
                    let loc = self
                        .lookup_array(&arr)
                        .expect("sema checked the array exists")
                        .loc
                        .clone();
                    self.emit(&format!("    mov rax, {}", loc.q(dim)));
                    self.emit("    dec rax");
                }
            }
            // File status. Both take a file number and return a number.
            "EOF" | "LOF" => {
                let arg_type = self.gen_expr(&args[0]);
                self.gen_coercion(arg_type, DataType::Long);
                self.emit_arg_reg(0, "rax");
                let rt = if upper_name == "EOF" {
                    "_rt_file_eof"
                } else {
                    "_rt_file_lof"
                };
                self.emit(&format!("    call {}", rt));
            }
            // Print positioning. These emit output rather than yielding a
            // value, so they are only meaningful inside PRINT.
            "TAB" | "SPC" => {
                let arg_type = self.gen_expr(&args[0]);
                self.gen_coercion(arg_type, DataType::Long);
                self.emit_arg_reg(0, "rax");
                let rt = if upper_name == "TAB" {
                    "_rt_print_tab"
                } else {
                    "_rt_print_spc"
                };
                self.emit(&format!("    call {}", rt));
                // Leave a zero so PRINT has a well-defined value to render...
                // but PRINT special-cases these, so it is never printed.
                self.emit("    xorpd xmm0, xmm0");
            }
            _ => {
                // User-defined function or array access
                // Sema has already established that this name is a procedure
                // or an array, so no spelling heuristic is needed. Procedures
                // win: an array and a procedure cannot share a name.
                if self.symbols.procs.contains_key(&upper_name) {
                    self.gen_call(&upper_name, args);
                } else {
                    self.gen_array_load(&upper_name, args);
                }
            }
        }
    }

    /// Emit a call to a user SUB or FUNCTION.
    ///
    /// Argument placement comes from `classify_params`, driven by the callee's
    /// *declared* parameter types. Classifying by the argument expressions'
    /// types instead -- as this used to -- lets caller and callee disagree, and
    /// silently corrupts the callee's frame.
    fn gen_call(&mut self, name: &str, args: &[Expr]) {
        let upper = name.to_uppercase();
        let param_types: Vec<DataType> = self
            .symbols
            .procs
            .get(&upper)
            .map(|p| p.params.iter().map(Self::param_data_type).collect())
            .unwrap_or_default();

        let mangled = mangle(&upper);
        if args.is_empty() {
            self.emit(&format!("    call _proc_{}", mangled));
            return;
        }

        let (places, stack_slots) = Self::classify_params(&param_types);

        // Phase 1: evaluate every argument into a temp block, so that a nested
        // call inside a later argument cannot clobber an earlier one.
        let words: usize = param_types
            .iter()
            .map(|t| Self::words_for(*t) as usize)
            .sum();
        let temp_bytes = ((words * 8 + 15) & !15) as i32;
        self.emit(&format!("    sub rsp, {}", temp_bytes));

        let param_decls: Vec<Param> = self
            .symbols
            .procs
            .get(&upper)
            .map(|p| p.params.clone())
            .unwrap_or_default();

        let mut temp_of: Vec<i32> = Vec::with_capacity(args.len());
        let mut w = 0i32;
        for (i, (arg, ty)) in args.iter().zip(&param_types).enumerate() {
            // A record argument is passed as the address of the caller's copy.
            if let Some(Param {
                ty: Some(TypeRef::Record(_)),
                ..
            }) = param_decls.get(i)
            {
                if let Expr::Variable(src) = arg {
                    if let Some(src_ty) = self.typed_var(src) {
                        let src_loc = self.get_record_loc(src, &src_ty);
                        match &src_loc {
                            Loc::Global(sym) => self.emit(&format!("    lea rax, [rip + {}]", sym)),
                            Loc::Frame(off) => self.emit(&format!("    lea rax, [rbp + {}]", off)),
                        }
                        self.emit(&format!("    mov QWORD PTR [rsp + {}], rax", w * 8));
                        temp_of.push(w * 8);
                        w += 1;
                        continue;
                    }
                }
            }
            let arg_type = self.gen_expr(arg);
            if *ty == DataType::String {
                self.emit(&format!("    mov QWORD PTR [rsp + {}], rax", w * 8));
                self.emit(&format!("    mov QWORD PTR [rsp + {}], rdx", w * 8 + 8));
                temp_of.push(w * 8);
                w += 2;
            } else {
                // Numeric arguments travel as f64 bit patterns in integer
                // slots; the callee narrows to the declared type.
                self.gen_coercion(arg_type, DataType::Double);
                self.emit(&format!("    movsd QWORD PTR [rsp + {}], xmm0", w * 8));
                temp_of.push(w * 8);
                w += 1;
            }
        }

        // Phase 2: copy the stack-passed slots into place.
        let stack_bytes = ((stack_slots * 8 + 15) & !15) as i32;
        if stack_slots > 0 {
            self.emit(&format!("    sub rsp, {}", stack_bytes));
        }
        for (place, off) in places.iter().zip(&temp_of) {
            // r11 is caller-saved and an argument register on neither ABI.
            if let Slot::Stk(i) = place.ptr {
                self.emit(&format!(
                    "    mov r11, QWORD PTR [rsp + {}]",
                    stack_bytes + off
                ));
                self.emit(&format!("    mov QWORD PTR [rsp + {}], r11", i as i32 * 8));
            }
            if let Some(Slot::Stk(i)) = place.len {
                self.emit(&format!(
                    "    mov r11, QWORD PTR [rsp + {}]",
                    stack_bytes + off + 8
                ));
                self.emit(&format!("    mov QWORD PTR [rsp + {}], r11", i as i32 * 8));
            }
        }

        // Phase 3: load the register slots last, so nothing can clobber them.
        let regs = PlatformAbi::INT_ARG_REGS;
        for (place, off) in places.iter().zip(&temp_of) {
            if let Slot::Reg(i) = place.ptr {
                self.emit(&format!(
                    "    mov {}, QWORD PTR [rsp + {}]",
                    regs[i],
                    stack_bytes + off
                ));
            }
            if let Some(Slot::Reg(i)) = place.len {
                self.emit(&format!(
                    "    mov {}, QWORD PTR [rsp + {}]",
                    regs[i],
                    stack_bytes + off + 8
                ));
            }
        }

        self.emit(&format!("    call _proc_{}", mangled));
        self.emit(&format!("    add rsp, {}", stack_bytes + temp_bytes));
    }

    /// Emit storage for one DIM/REDIM declarator.
    ///
    /// A declarator is an array, a typed scalar, or a typed array, and one
    /// statement may mix them.
    fn gen_declarator(&mut self, decl: &Declarator, preserve: bool) {
        match (&decl.dimensions, &decl.ty) {
            // An array of records: element size comes from the type.
            (Some(dims), Some(ty)) => {
                let arr = ArrayDecl {
                    name: decl.name.clone(),
                    dimensions: dims.clone(),
                };
                let words = self.symbols.type_words(ty);
                let key = decl.name.to_uppercase();
                if self.current_proc.is_some() {
                    self.proc_record_elem_words.insert(key, words);
                } else {
                    self.record_elem_words.insert(key, words);
                }
                self.gen_array_alloc(&arr, preserve);
            }
            (Some(dims), None) => {
                let arr = ArrayDecl {
                    name: decl.name.clone(),
                    dimensions: dims.clone(),
                };
                if preserve {
                    self.gen_array_alloc(&arr, true);
                } else {
                    self.gen_dim_array(&arr);
                }
            }
            // A typed scalar: allocating its storage is all that is needed,
            // since .bss and the frame prologue already zero it.
            (None, Some(ty)) => {
                let ty = ty.clone();
                self.get_record_loc(&decl.name, &ty);
            }
            (None, None) => unreachable!("the parser rejects a bare declarator"),
        }
    }

    fn gen_dim_array(&mut self, arr: &ArrayDecl) {
        self.gen_array_alloc(arr, false);
    }

    /// Allocate or reallocate an array's storage.
    ///
    /// The descriptor -- word 0 the element pointer, word 1+i dimension i's
    /// element count -- is reused when the array already exists, so REDIM
    /// resizes in place rather than orphaning it, and a repeated DIM does not
    /// leak a second descriptor.
    ///
    /// With `preserve`, the block is grown with realloc and the new tail
    /// zeroed; otherwise fresh zeroed storage is allocated.
    fn gen_array_alloc(&mut self, arr: &ArrayDecl, preserve: bool) {
        let elem_size = self.elem_size_for(&arr.name);
        let ndims = arr.dimensions.len();

        // Reuse the existing descriptor, or reserve one. A module-level array
        // gets static storage so procedures can reach it; one declared inside a
        // procedure is local to it.
        let loc = match self.lookup_array(&arr.name).map(|i| i.loc.clone()) {
            Some(existing) => existing,
            None if self.current_proc.is_some() => {
                self.stack_offset -= 8 * (1 + ndims as i32);
                Loc::Frame(self.stack_offset)
            }
            None => Loc::Global(format!("_arr_{}", mangle(&arr.name))),
        };

        // With PRESERVE, the old element count is needed to know where the
        // newly added tail begins.
        if preserve {
            self.emit(&format!("    mov rax, {}", loc.q(1)));
            for i in 1..ndims {
                self.emit(&format!("    imul rax, {}", loc.q(1 + i as i32)));
            }
            self.emit(&format!("    imul rax, {}", elem_size));
            self.emit("    push rax"); // old size in bytes
            self.emit("    sub rsp, 8"); // keep rsp 16-byte aligned
        }

        // Evaluate and store all dimension bounds.
        // BASIC DIM A(N) means indices 0..N (N+1 elements), so add 1 to each bound
        for (i, dim) in arr.dimensions.iter().enumerate() {
            let dim_type = self.gen_expr(dim);
            if dim_type.is_integer() {
                // Value already in eax, sign-extend to rax
                self.emit("    movsxd rax, eax");
            } else {
                self.emit("    cvttsd2si rax, xmm0");
            }
            self.emit("    inc rax"); // DIM A(N) has N+1 elements (0 to N)
            self.emit(&format!("    mov {}, rax", loc.q(1 + i as i32)));
        }

        // Calculate total elements: dim0 * dim1 * dim2 * ...
        self.emit(&format!("    mov rax, {}", loc.q(1)));
        for i in 1..ndims {
            self.emit(&format!("    imul rax, {}", loc.q(1 + i as i32)));
        }
        self.emit(&format!("    imul rax, {}", elem_size));

        if preserve {
            // realloc(old_ptr, new_size)
            self.emit("    mov r10, rax"); // new size in bytes
            self.emit("    push r10");
            self.emit("    sub rsp, 8"); // keep rsp 16-byte aligned across the call
            self.emit(&format!("    mov {}, {}", Self::arg_reg(0), loc.q(0)));
            self.emit_arg_reg(1, "r10");
            self.emit_call_libc("realloc");
            self.emit("    add rsp, 8");
            self.emit("    pop r10"); // new size
        } else {
            // calloc(1, size): BASIC guarantees a fresh array reads as 0 / "",
            // which malloc alone does not.
            self.emit(&format!("    mov {}, 1", Self::arg_reg(0)));
            self.emit_arg_reg(1, "rax");
            self.emit_call_libc("calloc");
        }

        if self.opts.checks {
            // A null result would otherwise be written into the descriptor and
            // dereferenced on first use.
            self.emit("    test rax, rax");
            self.emit_check("jz", RtError::OutOfMemory);
        }

        // Store array pointer
        self.emit(&format!("    mov {}, rax", loc.q(0)));

        if preserve {
            // Zero the newly added tail, from the old size up to the new one.
            self.emit("    add rsp, 8");
            self.emit("    pop r11"); // old size in bytes
            self.emit("    mov rcx, r11");
            let loop_label = self.new_label("preserve_zero");
            let done_label = self.new_label("preserve_done");
            self.emit_label(&loop_label);
            self.emit("    cmp rcx, r10");
            self.emit(&format!("    jae {}", done_label));
            self.emit("    mov BYTE PTR [rax + rcx], 0");
            self.emit("    inc rcx");
            self.emit(&format!("    jmp {}", loop_label));
            self.emit_label(&done_label);
        }

        // Record array info
        let info = ArrayInfo { loc, ndims };
        if self.current_proc.is_some() {
            self.proc_arrays.insert(arr.name.clone(), info);
        } else {
            self.arrays.insert(arr.name.clone(), info);
        }
    }

    /// Compute the address of an array element, leaving it in `rax`.
    ///
    /// Shared by loads and stores so the index arithmetic -- and the bounds
    /// checks guarding it -- exist in exactly one place.
    fn gen_array_addr(&mut self, name: &str, indices: &[Expr]) {
        // Descriptors for every declared array are reserved before any code is
        // emitted, and sema has already rejected undeclared ones, so this
        // cannot fail for a program that reached code generation.
        let arr_info = self
            .lookup_array(name)
            .expect("sema checked the array is declared");
        let loc = arr_info.loc.clone();
        let elem_size = self.elem_size_for(name);

        // A module-level array's descriptor lives in .bss, so its element
        // pointer is null until the DIM executes. Catching that is what turns
        // "used before DIM at run time" into a diagnosable abort rather than a
        // null dereference.
        if self.opts.checks {
            self.emit(&format!("    cmp {}, 0", loc.q(0)));
            self.emit_check("je", RtError::Undim);
        }

        // Calculate linear index using row-major order:
        // For A(i, j, k): linear = ((i * dim1) + j) * dim2 + k
        // Start with first index
        let idx_type = self.gen_expr(&indices[0]);
        if idx_type.is_integer() {
            // Already in eax, sign-extend to rax
            self.emit("    movsxd rax, eax");
        } else {
            self.emit("    cvttsd2si rax, xmm0");
        }
        // One unsigned compare catches both a negative index and one past the
        // end, since a negative value wraps to a huge unsigned one. The stored
        // bound is already the element count (declared bound + 1).
        if self.opts.checks {
            self.emit(&format!("    cmp rax, {}", loc.q(1)));
            self.emit_check("jae", RtError::Subscript);
            self.emit_lower_bound_check("rax");
        }

        // For each subsequent index, multiply by dimension bound and add
        for (i, idx_expr) in indices.iter().enumerate().skip(1) {
            // Save current accumulated index - use 16 bytes for alignment
            self.emit(&format!("    sub rsp, {}", STACK_TEMP_SPACE));
            self.emit("    mov QWORD PTR [rsp], rax");
            // Evaluate next index
            let idx_type = self.gen_expr(idx_expr);
            if idx_type.is_integer() {
                self.emit("    movsxd rcx, eax");
            } else {
                self.emit("    cvttsd2si rcx, xmm0");
            }
            if self.opts.checks {
                self.emit(&format!("    cmp rcx, {}", loc.q(1 + i as i32)));
                self.emit_check("jae", RtError::Subscript);
                self.emit_lower_bound_check("rcx");
            }
            self.emit("    mov rax, QWORD PTR [rsp]");
            self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));
            // rax = rax * dim[i] + indices[i]
            self.emit(&format!("    imul rax, {}", loc.q(1 + i as i32)));
            self.emit("    add rax, rcx");
        }

        // Multiply by element size and add to base pointer
        self.emit(&format!("    imul rax, {}", elem_size));
        self.emit(&format!("    add rax, {}", loc.q(0)));
    }

    fn gen_array_load(&mut self, name: &str, indices: &[Expr]) {
        self.gen_array_addr(name, indices);

        // Load value from computed address
        match DataType::from_suffix(name) {
            DataType::String => {
                self.emit("    mov rcx, rax");
                self.emit("    mov rax, QWORD PTR [rcx]");
                self.emit("    mov rdx, QWORD PTR [rcx + 8]");
            }
            DataType::Integer => self.emit("    movsx eax, WORD PTR [rax]"),
            DataType::Long => self.emit("    mov eax, DWORD PTR [rax]"),
            DataType::Single => self.emit("    movss xmm0, DWORD PTR [rax]"),
            DataType::Double => self.emit("    movsd xmm0, QWORD PTR [rax]"),
        }
    }

    fn gen_array_store(&mut self, name: &str, indices: &[Expr], value: &Expr) {
        self.gen_array_addr(name, indices);

        // Save the address while the value is evaluated - 16 bytes for alignment
        self.emit(&format!("    sub rsp, {}", STACK_TEMP_SPACE));
        self.emit("    mov QWORD PTR [rsp], rax");

        let val_type = self.gen_expr(value);
        if val_type == DataType::String {
            self.emit_string_copy();
        }

        self.emit("    mov rcx, QWORD PTR [rsp]");
        self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));

        let elem_type = DataType::from_suffix(name);
        if elem_type == DataType::String {
            self.emit("    mov QWORD PTR [rcx], rax");
            self.emit("    mov QWORD PTR [rcx + 8], rdx");
        } else {
            // Coerce to the element's declared type, then store at its width,
            // exactly as a scalar assignment does.
            self.gen_coercion(val_type, elem_type);
            match elem_type {
                DataType::Integer => self.emit("    mov WORD PTR [rcx], ax"),
                DataType::Long => self.emit("    mov DWORD PTR [rcx], eax"),
                DataType::Single => self.emit("    movss DWORD PTR [rcx], xmm0"),
                DataType::Double => self.emit("    movsd QWORD PTR [rcx], xmm0"),
                DataType::String => unreachable!("handled above"),
            }
        }
    }

    fn gen_string_assign(&mut self, name: &str, value: &Expr) {
        self.gen_expr(value);
        self.emit_string_copy();
        // Both words were reserved when the variable was first seen, so this no
        // longer has to scavenge a slot per assignment.
        let loc = self.get_var_loc(name);
        self.emit(&format!("    mov {}, rax", loc.q(0)));
        self.emit(&format!("    mov {}, rdx", loc.q(1)));
    }

    fn emit_data_section(&mut self) {
        // Pass 1: intern every DATA string and build the table rows as text.
        //
        // Nothing is emitted yet, so `string_literals` may still grow. Emitting
        // the `_str_N` labels first and interning DATA strings afterwards -- as
        // this used to -- left the trailing labels referenced by the table but
        // never defined, so any program with a string in a DATA statement
        // failed to link.
        let data_items = std::mem::take(&mut self.data_items);
        let mut rows: Vec<(u8, String)> = Vec::with_capacity(data_items.len());
        for item in &data_items {
            rows.push(match item {
                Literal::Integer(n) => (0, n.to_string()),
                Literal::Float(f) => (1, format!("0x{:X}", f.to_bits())),
                Literal::String(s) => {
                    let idx = self.add_string_literal(s);
                    (2, format!("_str_{}", idx))
                }
            });
        }

        // Pass 2: emit. `string_literals` is final from here on.
        self.output.push_str("\n.data\n");

        let strings = self.string_literals.clone();
        for (i, s) in strings.iter().enumerate() {
            self.output.push_str(&format!("_str_{}:\n", i));
            let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
            // .asciz, not .ascii: _rt_read_string measures DATA strings with
            // strlen/lstrlenA, so without a terminator a string READ runs past
            // its own literal into the next one. The trailing NUL is invisible
            // to everything that uses the (ptr, len) representation.
            self.output
                .push_str(&format!("    .asciz \"{}\"\n", escaped));
        }

        // DATA table - always define it (even if empty) to avoid linker errors
        self.output.push_str(".p2align 3\n");
        self.output.push_str("_data_table:\n");
        for (tag, value) in &rows {
            let kind = match tag {
                0 => "int",
                1 => "float",
                _ => "string",
            };
            self.output
                .push_str(&format!("    .quad {}  # type {}\n", tag, kind));
            self.output.push_str(&format!("    .quad {}\n", value));
        }
        self.output
            .push_str(&format!("_data_count: .quad {}\n", rows.len()));

        // DATA pointer
        self.emit("_data_ptr: .quad 0");

        // GOSUB return stack pointer
        if self.gosub_used {
            self.emit("_gosub_sp: .quad 0");
        }

        self.emit("");
        self.emit(".bss");
        self.emit(".p2align 3");

        // Module-level variables and arrays. .bss is zero-filled by the loader,
        // which is what gives BASIC's "unassigned is 0 / empty string" for free.
        // Emitted in sorted order so the generated assembly is reproducible.
        let mut vars: Vec<(&String, &VarInfo)> = self.vars.iter().collect();
        vars.sort_by(|a, b| a.0.cmp(b.0));
        for (name, info) in vars {
            if let Loc::Global(sym) = &info.loc {
                let bytes = 8 * Self::words_for(info.data_type);
                self.output
                    .push_str(&format!("{}: .zero {}  # {}\n", sym, bytes, name));
            }
        }

        let record_globals = std::mem::take(&mut self.record_globals);
        for (name, words) in &record_globals {
            self.output.push_str(&format!(
                "_rec_{}: .zero {}  # {} (record)\n",
                mangle(name),
                words * 8,
                name
            ));
        }

        let mut arrays: Vec<(&String, &ArrayInfo)> = self.arrays.iter().collect();
        arrays.sort_by(|a, b| a.0.cmp(b.0));
        for (name, info) in arrays {
            if let Loc::Global(sym) = &info.loc {
                // Word 0 is the element pointer (null until DIM runs), then one
                // word per dimension bound.
                let bytes = 8 * (1 + info.ndims);
                self.output.push_str(&format!(
                    "{}: .zero {}  # {} ptr + {} dim(s)\n",
                    sym, bytes, name, info.ndims
                ));
            }
        }

        // GOSUB stack (if needed)
        if self.gosub_used {
            self.emit(&format!(
                "_gosub_stack: .skip {}  # GOSUB return stack (64K entries)",
                GOSUB_STACK_SIZE
            ));
        }
    }
}
