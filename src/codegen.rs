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
use crate::lexer::normalized;
use crate::parser::TypeRef;
use crate::parser::*;
use crate::sema::{Scope as SemaScope, Symbols};
use crate::using;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::sync::LazyLock;

/// Emit one formatted instruction straight into the output buffer.
///
/// The spelling this replaces was `self.emit(&format!(...))`, which built a
/// throwaway `String` for every instruction only to copy it into `output` and
/// drop it. A 200k-line BASIC program assembles to five million lines, so that
/// was five million allocations spent handing text to `push_str`.
///
/// `writeln!` into a `String` cannot fail -- `fmt::Write` for `String` is
/// infallible -- so the result is discarded rather than unwrapped.
macro_rules! emit {
    ($self:expr, $($arg:tt)*) => {{
        let _ = writeln!($self.output, $($arg)*);
    }};
}

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

/// How a builtin that is just "evaluate, coerce, call" reaches its helper.
///
/// The irregular builtins stay written out in [`CodeGen::gen_fn_call`]; these
/// differ only in the coercion and the symbol, which is what a table is for.
enum Builtin {
    /// Takes no argument.
    Call0(&'static str),
    /// One string argument, passed as (pointer, length).
    CallStr(&'static str),
    /// One numeric argument, widened to Double in xmm0.
    /// One numeric argument, narrowed to Long and sign-extended.
    CallLong(&'static str),
    /// No helper at all: the conversion *is* the coercion.
    Coerce(DataType),
}

static RT_BUILTINS: LazyLock<HashMap<&'static str, Builtin>> = LazyLock::new(|| {
    HashMap::from([
        ("TIMER", Builtin::Call0("_rt_timer")),
        ("VAL", Builtin::CallStr("_rt_val")),
        ("LTRIM$", Builtin::CallStr("_rt_ltrim")),
        ("RTRIM$", Builtin::CallStr("_rt_rtrim")),
        ("UCASE$", Builtin::CallStr("_rt_ucase")),
        ("LCASE$", Builtin::CallStr("_rt_lcase")),
        // STR$ is not here: it picks its formatter from the argument's static
        // type, which this table has no way to express. It was the only user of
        // a "coerce to double and call" form, which is why none remains.
        ("CHR$", Builtin::CallLong("_rt_chr")),
        ("SPACE$", Builtin::CallLong("_rt_space")),
        ("HEX$", Builtin::CallLong("_rt_hex")),
        ("OCT$", Builtin::CallLong("_rt_oct")),
        ("CSNG", Builtin::Coerce(DataType::Single)),
        ("CDBL", Builtin::Coerce(DataType::Double)),
    ])
});

/// Stack space for temporary values (must be 16-byte aligned)
const STACK_TEMP_SPACE: i32 = 16;

/// Maximum expression nesting depth before warning (each level uses 16 bytes of stack)
const MAX_EXPR_DEPTH: u32 = 256;

/// GOSUB stack size in bytes (64K entries * 8 bytes = 512KB)
const GOSUB_STACK_SIZE: i32 = 524288;

/// ASCII character codes
const ASCII_TAB: i64 = 9;
const ASCII_COMMA: i64 = 44;
/// The console's file number. The runtime seeds handle 0 with standard
/// output, so PRINT and PRINT # are the same helpers with a different handle.
const CONSOLE: i64 = 0;

/// Record length of a `FOR RANDOM` open with no `LEN =` clause, as in GW-BASIC.
const DEFAULT_RECLEN: i64 = 128;

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
    GosubOverflow,
    BadFileNum,
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
            RtError::GosubOverflow => "_err_gosub",
            RtError::BadFileNum => "_err_badfile",
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
            RtError::GosubOverflow => "gosub",
            RtError::BadFileNum => "badfile",
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
#[derive(Clone)]
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

/// A FOR loop's limit or step, held wherever it is cheapest to reach.
///
/// A constant is written into the compare and the add themselves; anything
/// else is evaluated once, before the loop, into a frame slot.
enum ForOperand {
    /// An integer constant, as an instruction immediate.
    Imm(i32),
    /// A Double constant, named in the constant pool.
    Pool(String),
    /// A frame slot, at the control type's own width.
    Slot(i32),
}

impl ForOperand {
    /// The operand as it is written in an instruction.
    fn text(&self, ct: DataType) -> String {
        match self {
            ForOperand::Imm(n) => n.to_string(),
            ForOperand::Pool(sym) => sym.clone(),
            ForOperand::Slot(off) => {
                format!("{} [rbp + {}]", CodeGen::for_ptr(ct), off)
            }
        }
    }
}

/// A variable a loop is holding in a register rather than in its storage.
///
/// Both a FOR control variable and an accumulator the body assigns to: the
/// difference is only where the register's initial value comes from, and both
/// are written back at the loop's single exit.
#[derive(Clone)]
struct Promoted {
    /// The name as the AST spells it, which is how a reference is matched.
    name: String,
    reg: String,
    ty: DataType,
    /// The variable's storage, for the write-back.
    loc: Loc,
}

/// An array whose descriptor a loop is holding in registers.
///
/// The element pointer and the per-dimension element counts do not change
/// while the loop runs -- DIM and REDIM are outside the allowlist that makes a
/// loop promotable -- but every subscript re-read them: two loads for the
/// pointer, since the null check and the address each fetched it, and one per
/// bound compare.
#[derive(Clone)]
struct HoistedArray {
    /// The name as the AST spells it, which is how a subscript is matched.
    name: String,
    /// Register holding the element pointer, if one was free.
    base: Option<String>,
    /// Register per dimension holding its element count, where one was free.
    bounds: Vec<Option<String>>,
}

/// Registers this compiler never allocates for anything else, so a loop may
/// keep a variable in one for its whole duration.
///
/// The integer ones belong to the calling function and are saved around the
/// loop. The XMM ones do not need saving -- System V has no callee-saved XMM
/// register at all, which is the same fact that stops a promoted loop from
/// containing a call.
const PROMO_GPRS: [&str; 4] = ["r12", "r13", "r14", "r15"];
const PROMO_XMMS: [&str; 8] = [
    "xmm4", "xmm5", "xmm6", "xmm7", "xmm8", "xmm9", "xmm10", "xmm11",
];

/// How many variables besides the control variable one loop may promote.
///
/// Past a handful the registers are gone and each further one is a save and a
/// restore for a diminishing return; a loop with more accumulators than this
/// simply keeps the rest in memory.
const MAX_PROMOTED_ACCUMULATORS: usize = 3;

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
    /// Double constants, as bit patterns, in emission order. Deduplicated
    /// through `f64_index`; keyed on the bits rather than the value so that
    /// 0.0 and -0.0 stay distinct, as do NaNs with different payloads.
    f64_pool: Vec<u64>,
    f64_index: HashMap<u64, usize>,
    data_items: Vec<Literal>,                // DATA values
    current_proc: Option<String>,            // current SUB/FUNCTION name
    proc_vars: HashMap<String, VarInfo>,     // local variables for current proc
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
    /// Variables currently held in a register instead of in their storage,
    /// innermost loop last. A read or a write of one of these names has to go
    /// to the register: the storage is stale until the loop ends.
    promoted: Vec<Promoted>,
    /// Array descriptors currently held in registers, innermost loop last.
    hoisted_arrays: Vec<HoistedArray>,
    gosub_used: bool, // whether GOSUB is used (need return stack)
    expr_depth: u32,  // current expression nesting depth
}

impl CodeGen {
    fn emit(&mut self, s: &str) {
        self.output.push_str(s);
        self.output.push('\n');
    }

    /// Whether the instruction just emitted begins with `prefix`.
    ///
    /// A peephole over the text, which is all there is to look at without an
    /// IR. It is deliberately literal: anything at all between -- a label, a
    /// comment, a branch target -- means no match, so the caller keeps
    /// whatever it was about to elide.
    fn last_emitted_is(&self, prefix: &str) -> bool {
        self.output
            .rsplit('\n')
            .nth(1)
            .is_some_and(|line| line.trim_start().starts_with(prefix))
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
            emit!(self, "    mov {}, {}", dst, src_reg);
        }
    }

    /// The 32-bit name of an integer register, for [`emit_arg_imm`].
    ///
    /// [`emit_arg_imm`]: Self::emit_arg_imm
    fn reg32(reg: &str) -> &'static str {
        match reg {
            "rax" => "eax",
            "rbx" => "ebx",
            "rcx" => "ecx",
            "rdx" => "edx",
            "rsi" => "esi",
            "rdi" => "edi",
            "r8" => "r8d",
            "r9" => "r9d",
            other => panic!("no 32-bit name recorded for {}", other),
        }
    }

    /// Emit a mov instruction to set up an integer argument from an immediate
    ///
    /// Writing a 32-bit register zeroes the upper half, so for a value that
    /// fits unsigned in 32 bits the narrow form is equivalent and two bytes
    /// shorter -- `mov edi, 1` rather than `mov rdi, 1`. Every caller today
    /// passes a small non-negative constant, so this is the form that is
    /// actually emitted; the wide form remains for anything that needs it.
    fn emit_arg_imm(&mut self, arg_n: usize, value: i64) {
        let dst = Self::arg_reg(arg_n);
        if (0..=u32::MAX as i64).contains(&value) {
            emit!(self, "    mov {}, {}", Self::reg32(dst), value);
        } else {
            emit!(self, "    mov {}, {}", dst, value);
        }
    }

    /// Emit a lea instruction to set up an integer argument from a memory reference
    fn emit_arg_lea(&mut self, arg_n: usize, mem: &str) {
        let dst = Self::arg_reg(arg_n);
        emit!(self, "    lea {}, {}", dst, mem);
    }

    /// Call a libc function, whose arguments are already in place.
    fn emit_call_libc(&mut self, func: &str) {
        self.emit_call_with_args(func, &[]);
    }

    /// Call `sym` with `srcs` as its integer arguments, in order.
    ///
    /// The ABIs differ in how much of a call they can carry in registers --
    /// six arguments on System V, four on Win64 -- and in whether the caller
    /// owes the callee shadow space. Both follow from the register list, so
    /// this computes the split instead of spelling out a platform each time.
    ///
    /// Arguments are assigned last-first, so a register that is both a source
    /// and a destination is read before it is overwritten. Callers choose
    /// sources with that order in mind.
    fn emit_call_with_args(&mut self, sym: &str, srcs: &[&str]) {
        let regs = PlatformAbi::INT_ARG_REGS;
        let shadow = PlatformAbi::SHADOW_SPACE;
        let on_stack = srcs.len().saturating_sub(regs.len()) as i32;

        // Whatever is reserved has to leave rsp 16-byte aligned at the call.
        let reserve = match shadow + on_stack * 8 {
            0 => 0,
            bytes => (bytes + 15) / 16 * 16,
        };
        if reserve > 0 {
            emit!(self, "    sub rsp, {}", reserve);
        }

        for (i, src) in srcs.iter().enumerate().rev() {
            match regs.get(i) {
                Some(reg) if reg == src => {} // already there
                Some(reg) => emit!(self, "    mov {}, {}", reg, src),
                None => {
                    let off = shadow + (i - regs.len()) as i32 * 8;
                    emit!(self, "    mov QWORD PTR [rsp + {}], {}", off, src);
                }
            }
        }

        emit!(self, "    call {}", sym);
        if reserve > 0 {
            emit!(self, "    add rsp, {}", reserve);
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

    /// A memory operand naming `value` in the Double constant pool.
    ///
    /// Materializing a double as an immediate costs `mov rax, <imm64>` plus
    /// `movq xmm0, rax`: fifteen bytes, a scratch register, and a register
    /// round trip. Naming it in memory costs eight bytes and no register, and
    /// every SSE instruction that wants a second operand will take it from
    /// memory directly, which is what makes the literal fast paths below
    /// possible at all.
    fn f64_operand(&mut self, value: f64) -> String {
        let bits = value.to_bits();
        let next = self.f64_pool.len();
        let idx = *self.f64_index.entry(bits).or_insert(next);
        if idx == next {
            self.f64_pool.push(bits);
        }
        format!("QWORD PTR [rip + _f64_{}]", idx)
    }

    /// Load a Double constant into xmm0.
    fn gen_double_const(&mut self, value: f64) {
        // Positive zero is the one value with a shorter encoding still: three
        // bytes, no memory reference, and it breaks rather than creates a
        // dependency on xmm0's previous contents. Negative zero is a different
        // bit pattern and does not qualify.
        if value == 0.0 && value.is_sign_positive() {
            self.emit("    xorpd xmm0, xmm0");
            return;
        }
        let operand = self.f64_operand(value);
        emit!(self, "    movsd xmm0, {}", operand);
    }

    /// The constant value of a numeric literal, for the fast paths that need
    /// to know one at compile time. `None` for anything else, including
    /// strings.
    ///
    /// A `CONST` name counts: sema has already folded it to a literal, and
    /// `gen_expr` substitutes it, so leaving it out here would mean `X * 2`
    /// took the fast path while `X * TWO` did not.
    ///
    /// So does a negated one. The parser gives `-1` as a negation applied to
    /// the literal 1 rather than as a literal, and without this `STEP -1` --
    /// the commonest negative step there is -- would count as unknown.
    fn const_double(&self, expr: &Expr) -> Option<f64> {
        let lit = match expr {
            Expr::Literal(lit) => lit,
            Expr::Variable(name) => self.symbols.consts.get(normalized(name))?,
            Expr::Unary {
                op: UnaryOp::Neg,
                operand,
            } => return self.const_double(operand).map(|v| -v),
            _ => return None,
        };
        match lit {
            Literal::Integer(n) => Some(*n as f64),
            Literal::Float(f) => Some(*f),
            Literal::String(_) => None,
        }
    }

    /// The value of an integer constant that fits an instruction's imm32.
    ///
    /// Read from the literal rather than through [`Self::const_double`]: past
    /// 2^53 an f64 no longer holds every integer, and the point of this is to
    /// be exact.
    fn const_i32(&self, expr: &Expr) -> Option<i32> {
        let lit = match expr {
            Expr::Literal(lit) => lit,
            Expr::Variable(name) => self.symbols.consts.get(normalized(name))?,
            // `checked_neg` rather than `-`: negating i32::MIN is not an i32,
            // and the general path handles it correctly.
            Expr::Unary {
                op: UnaryOp::Neg,
                operand,
            } => return self.const_i32(operand).and_then(i32::checked_neg),
            _ => return None,
        };
        match lit {
            Literal::Integer(n) => i32::try_from(*n).ok(),
            _ => None,
        }
    }

    /// Evaluate `expr` into xmm0 as a Double.
    ///
    /// A numeric constant is loaded straight from the pool. Going through
    /// `gen_expr` would put an integer literal in eax and then pay a
    /// `cvtsi2sd` to widen it -- an instruction whose destination register is
    /// also an input, so it carries a dependency on whatever xmm0 last held.
    fn gen_expr_to_double(&mut self, expr: &Expr) {
        if let Some(value) = self.const_double(expr) {
            self.gen_double_const(value);
            return;
        }
        let ty = self.gen_expr(expr);
        self.gen_coercion(ty, DataType::Double);
    }

    /// Evaluate `expr` into the working register for `ct`, coerced.
    ///
    /// Routes a Double target through the pooled path and everything else
    /// through the ordinary one, so a caller working in a type it only knows
    /// at compile time does not have to choose.
    fn gen_expr_coerced(&mut self, expr: &Expr, ct: DataType) {
        if ct == DataType::Double {
            self.gen_expr_to_double(expr);
            return;
        }
        let ty = self.gen_expr(expr);
        self.gen_coercion(ty, ct);
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
            Some(t) => DataType::from_type_ref(t),
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
            Expr::Variable(name) if crate::sema::is_zero_arg_builtin(name) => {
                self.fn_return_type(name)
            }
            Expr::Variable(name) => match self.symbols.consts.get(normalized(name)) {
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
                        Some(t) => DataType::from_type_ref(&t),
                        None => DataType::from_suffix(name),
                    },
                },
            },
            Expr::ArrayAccess { name, .. } => DataType::from_suffix(name),
            Expr::FnCall { name, args } => self.call_return_type(name, args),
            Expr::Field { .. } => self.field_expr_type(expr),
            // Unary minus keeps its operand's type; NOT is a bitwise operator
            // and yields an integer, exactly as AND, OR and XOR do.
            Expr::Unary {
                op: UnaryOp::Not, ..
            } => DataType::Long,
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

    /// Result type of a user-defined FUNCTION.
    ///
    /// `FUNCTION F(X) AS INTEGER` used to be parsed and then thrown away, so
    /// the result fell back to the name's suffix -- Double for an unsuffixed
    /// name, which printed 3.5 where the program asked for 3.
    fn proc_return_type(&self, name: &str) -> DataType {
        match self
            .symbols
            .procs
            .get(normalized(name))
            .and_then(|p| p.ret_ty.as_ref())
        {
            Some(ty) => DataType::from_type_ref(ty),
            None => DataType::from_suffix(name),
        }
    }

    fn fn_return_type(&self, name: &str) -> DataType {
        // Built-in functions that return strings
        let upper = name.to_uppercase();

        // A user-defined FUNCTION's declared type wins over any name-based
        // guess, including the built-in table below.
        if let Some(proc) = self.symbols.procs.get(&upper) {
            if let Some(ty) = &proc.ret_ty {
                return DataType::from_type_ref(ty);
            }
        }

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

        // The bitwise operators convert both operands to integers and produce
        // an integer, exactly as `\` and MOD above do.
        //
        // Without this they fell through to ordinary numeric promotion and came
        // out Double whenever either operand was -- which is what an unsuffixed
        // variable is. Codegen still emitted `and eax, ecx`, so the answer sat
        // in EAX while every consumer read xmm0 and found the *left operand*
        // still there: `A = 12 : B = 10 : PRINT A AND B` printed 12. Literal
        // operands are folded before reaching here, which is why it hid.
        if matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Xor) {
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
            //
            // Skipped when eax was just loaded by a word-sized `movsx`, which
            // has already sign-extended it -- the common case, since that is
            // how every INTEGER variable, array element and record field is
            // read. It cannot be skipped on the strength of the static type
            // alone: Integer is also what `promote_types` returns for
            // INTEGER+INTEGER, whose 32-bit `add` genuinely has to be
            // truncated back to 16 bits here, and `gen_coercion(Long,
            // Integer)` is a no-op that leaves an untruncated 32-bit value
            // behind a value typed Integer.
            (DataType::Integer, DataType::Long) => {
                if !self.last_emitted_is("movsx eax, WORD PTR") {
                    self.emit("    movsx eax, ax"); // sign-extend 16-bit to 32-bit
                }
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
        self.emit(".globl main");
        self.emit("");

        // Procedures
        for stmt in &program.statements {
            if let StmtKind::Sub { name, params, body } = &stmt.kind {
                self.gen_procedure(name, params, body, false);
            } else if let StmtKind::Function {
                name, params, body, ..
            } = &stmt.kind
            {
                self.gen_procedure(name, params, body, true);
            }
        }

        self.output.push_str(&main_asm);

        // Error trampolines, past every function body so the checked fast path
        // is only a compare and a never-taken branch.
        self.emit_error_trampolines();

        // Emit data section
        self.emit_data_section();

        // Handed over rather than cloned. The buffer is the whole program's
        // assembly -- 145 MB for a 200k-line source -- so copying it to return
        // it doubled the compiler's peak memory for nothing.
        std::mem::take(&mut self.output)
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
        self.emit_label("main");
        self.emit("    push rbp");
        self.emit("    mov rbp, rsp");

        // Reserve stack space (will patch later)
        self.emit("    sub rsp, 0         # STACK_RESERVE");

        // Initialize GOSUB return stack if needed
        if self.gosub_used {
            self.emit("    # Initialize GOSUB return stack");
            emit!(
                self,
                "    lea rax, [rip + _gosub_stack + {}]",
                GOSUB_STACK_SIZE
            ); // Point to end (stack grows down)
            self.emit("    mov QWORD PTR [rip + _gosub_sp], rax");
        }

        // The console is file handle 0, and each platform's runtime seeds that
        // slot its own way -- a libc stream on System V, a handle from the OS
        // on Windows. Both answer to the same call.
        self.emit("    call _rt_platform_init");

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
            // Both forms push a return address, so both need the GOSUB stack
            // emitted; without this, ON ... GOSUB alone failed to link.
            StmtKind::Gosub(_) | StmtKind::OnGosub { .. } => self.gosub_used = true,
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
        emit!(self, "    jne {}", skip);
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
        emit!(self, "    cmp {}, {}", reg, self.symbols.option_base);
        self.emit_check("jb", RtError::Subscript);
    }

    /// A LBOUND/UBOUND dimension known at compile time.
    ///
    /// The descriptor's word 0 holds the element pointer and word i the i'th
    /// dimension's count, so a constant dimension is a fixed offset.
    fn const_dim(&self, e: &Expr) -> Option<i32> {
        let lit = match e {
            Expr::Literal(l) => l.clone(),
            Expr::Variable(n) => self.symbols.consts.get(normalized(n))?.clone(),
            _ => return None,
        };
        match lit {
            Literal::Integer(n) => i32::try_from(n).ok(),
            _ => None,
        }
    }

    /// Whether an expression can be evaluated without calling anything.
    ///
    /// A call is what makes a promoted counter unsafe: System V has no
    /// callee-saved XMM register at all, so a Double counter would not survive
    /// one, and a called procedure can reach a module-level counter through
    /// its own name. Strings are rejected wholesale -- every string operation
    /// goes through the runtime -- as is `^`, which calls libc's pow.
    fn expr_is_call_free(&self, expr: &Expr) -> bool {
        if self.expr_type(expr) == DataType::String {
            return false;
        }
        match expr {
            Expr::Literal(_) => true,
            // A bare name is not always a variable: a parameterless FUNCTION
            // and a zero-argument builtin are both spelled this way, and both
            // compile to a call.
            Expr::Variable(name) => {
                let upper = name.to_uppercase();
                !crate::sema::is_zero_arg_builtin(&upper)
                    && !self.symbols.procs.contains_key(&upper)
            }
            Expr::ArrayAccess { name, indices } => {
                self.array_elem_type(name).is_none()
                    && indices.iter().all(|i| self.expr_is_call_free(i))
            }
            Expr::Unary { operand, .. } => self.expr_is_call_free(operand),
            Expr::Binary { op, left, right } => {
                *op != BinaryOp::Pow
                    && self.expr_is_call_free(left)
                    && self.expr_is_call_free(right)
            }
            Expr::FnCall { .. } | Expr::Field { .. } => false,
        }
    }

    /// Whether a statement can run with `counter` held only in a register.
    ///
    /// An allowlist, not a denylist: the set of statements that neither call,
    /// nor leave the loop by a route that skips its exit, nor write the
    /// counter behind the register's back. Everything else -- PRINT, INPUT,
    /// CALL, GOSUB, GOTO, READ, SWAP, file I/O -- says no, which costs those
    /// loops nothing they had before.
    fn stmt_allows_register_counter(&self, counter: &str, stmt: &Stmt) -> bool {
        let all = |s: &[Stmt]| {
            s.iter()
                .all(|s| self.stmt_allows_register_counter(counter, s))
        };
        match &stmt.kind {
            StmtKind::Let {
                name,
                indices,
                value,
            } => {
                name != counter
                    && !is_string_var(name)
                    && self.typed_var(name).is_none()
                    && indices
                        .as_ref()
                        .is_none_or(|ix| ix.iter().all(|e| self.expr_is_call_free(e)))
                    && self.expr_is_call_free(value)
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr_is_call_free(condition)
                    && all(then_branch)
                    && else_branch.as_ref().is_none_or(|b| all(b))
            }
            StmtKind::For {
                var,
                start,
                end,
                step,
                body,
            } => {
                var != counter
                    && self.expr_is_call_free(start)
                    && self.expr_is_call_free(end)
                    && step.as_ref().is_none_or(|s| self.expr_is_call_free(s))
                    && all(body)
            }
            StmtKind::While { condition, body } => self.expr_is_call_free(condition) && all(body),
            StmtKind::DoLoop {
                condition, body, ..
            } => condition.as_ref().is_none_or(|c| self.expr_is_call_free(c)) && all(body),
            // Leaves by way of a loop's own exit label, which is where the
            // register is written back.
            StmtKind::ExitLoop { .. } => true,
            StmtKind::Const { .. } => true,
            _ => false,
        }
    }

    /// Whether this whole loop can run with its counter in a register.
    ///
    /// The bounds are checked as well as the body: they are evaluated before
    /// the register is loaded, so a call in them would be harmless -- but a
    /// bound that is not call-free is a sign the loop is not the kind this
    /// helps, and checking them keeps the rule one sentence long.
    fn body_allows_register_counter(
        &self,
        counter: &str,
        body: &[Stmt],
        start: &Expr,
        end: &Expr,
        step: Option<&Expr>,
    ) -> bool {
        self.expr_is_call_free(start)
            && self.expr_is_call_free(end)
            && step.is_none_or(|s| self.expr_is_call_free(s))
            && body
                .iter()
                .all(|s| self.stmt_allows_register_counter(counter, s))
    }

    /// A register of the right class that no enclosing loop is already using.
    ///
    /// Nested loops promote too, so the pool is checked against what is live
    /// rather than indexed by depth; when it runs out the variable simply
    /// stays in memory.
    fn free_promotion_register(&self, ty: DataType) -> Option<&'static str> {
        let pool: &[&'static str] = if ty.is_integer() {
            &PROMO_GPRS
        } else {
            &PROMO_XMMS
        };
        pool.iter().copied().find(|r| !self.register_in_use(r))
    }

    /// Whether an enclosing loop is already holding something in `reg`.
    fn register_in_use(&self, reg: &str) -> bool {
        self.promoted.iter().any(|p| p.reg == reg)
            || self.hoisted_arrays.iter().any(|h| {
                h.base.as_deref() == Some(reg) || h.bounds.iter().any(|b| b.as_deref() == Some(reg))
            })
    }

    /// The register holding `name`'s element pointer, if a loop hoisted it.
    fn hoisted_base(&self, name: &str) -> Option<String> {
        self.hoisted_arrays
            .iter()
            .rev()
            .find(|h| h.name == name)
            .and_then(|h| h.base.clone())
    }

    /// The register holding dimension `dim`'s element count, if hoisted.
    fn hoisted_bound(&self, name: &str, dim: usize) -> Option<String> {
        self.hoisted_arrays
            .iter()
            .rev()
            .find(|h| h.name == name)
            .and_then(|h| h.bounds.get(dim).cloned().flatten())
    }

    /// The operand naming an array's element pointer.
    fn array_base_operand(&self, name: &str, loc: &Loc) -> String {
        self.hoisted_base(name).unwrap_or_else(|| loc.q(0))
    }

    /// Pick registers to hold the descriptors of the arrays `body` subscripts.
    ///
    /// Element pointers first, then bounds: a pointer is read twice per
    /// subscript and a bound once, so it is worth more per register. Whatever
    /// is left over keeps reading memory, which is what it did before.
    fn hoist_array_descriptors(&mut self, body: &[Stmt], claimed: &[String]) -> Vec<HoistedArray> {
        let names = Self::arrays_used_in(body);
        let mut taken: Vec<String> = claimed.to_vec();
        let mut out: Vec<HoistedArray> = Vec::new();

        for name in &names {
            let Some(info) = self.lookup_array(name) else {
                continue;
            };
            let ndims = info.ndims;
            let base = self.free_gpr(&taken);
            if let Some(reg) = &base {
                taken.push(reg.clone());
            }
            out.push(HoistedArray {
                name: name.clone(),
                base,
                bounds: vec![None; ndims],
            });
        }

        for h in out.iter_mut() {
            for slot in h.bounds.iter_mut() {
                match self.free_gpr(&taken) {
                    Some(reg) => {
                        taken.push(reg.clone());
                        *slot = Some(reg);
                    }
                    None => break,
                }
            }
        }

        out.retain(|h| h.base.is_some() || h.bounds.iter().any(|b| b.is_some()));
        out
    }

    /// A callee-saved register neither an enclosing loop nor `taken` is using.
    fn free_gpr(&self, taken: &[String]) -> Option<String> {
        PROMO_GPRS
            .iter()
            .find(|r| !self.register_in_use(r) && !taken.iter().any(|t| t == *r))
            .map(|r| r.to_string())
    }

    /// Save the registers a hoist claimed and load the descriptor into them.
    fn emit_hoist_loads(&mut self, h: &HoistedArray) {
        let Some(info) = self.lookup_array(&h.name) else {
            return;
        };
        let loc = info.loc.clone();
        let mut regs: Vec<(String, String)> = Vec::new();
        if let Some(reg) = &h.base {
            regs.push((reg.clone(), loc.q(0)));
        }
        for (i, slot) in h.bounds.iter().enumerate() {
            if let Some(reg) = slot {
                regs.push((reg.clone(), loc.q(1 + i as i32)));
            }
        }
        for (reg, mem) in regs {
            self.emit("    sub rsp, 16        # save a descriptor register");
            emit!(self, "    mov QWORD PTR [rsp], {}", reg);
            emit!(self, "    mov {}, {}", reg, mem);
        }
    }

    /// Give those registers back, in the order that unwinds the saves.
    fn emit_hoist_restores(&mut self, h: &HoistedArray) {
        let mut regs: Vec<String> = Vec::new();
        if let Some(reg) = &h.base {
            regs.push(reg.clone());
        }
        for reg in h.bounds.iter().flatten() {
            regs.push(reg.clone());
        }
        for reg in regs.into_iter().rev() {
            emit!(self, "    mov {}, QWORD PTR [rsp]", reg);
            self.emit("    add rsp, 16");
        }
    }

    /// An array's element pointer in a register, ready for a scaled index.
    ///
    /// Free when a loop has hoisted it; otherwise loaded into r10 here, after
    /// the last subscript has been evaluated -- evaluating one runs arbitrary
    /// code, which is free to clobber r10.
    fn array_base_into_register(&mut self, name: &str, loc: &Loc) -> String {
        if let Some(reg) = self.hoisted_base(name) {
            return reg;
        }
        emit!(self, "    mov r10, {}", loc.q(0));
        "r10".to_string()
    }

    /// The operand naming dimension `dim`'s element count.
    fn array_bound_operand(&self, name: &str, loc: &Loc, dim: usize) -> String {
        self.hoisted_bound(name, dim)
            .unwrap_or_else(|| loc.q(1 + dim as i32))
    }

    /// Every array subscripted anywhere in `body`, in first-use order.
    fn arrays_used_in(body: &[Stmt]) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        Self::walk_body(body, &mut |stmt| match &stmt.kind {
            StmtKind::Let {
                name,
                indices,
                value,
            } => {
                if let Some(ix) = indices {
                    if !names.iter().any(|n| n == name) {
                        names.push(name.clone());
                    }
                    for e in ix {
                        Self::walk_array_uses(e, &mut names);
                    }
                }
                Self::walk_array_uses(value, &mut names);
            }
            StmtKind::If { condition, .. } | StmtKind::While { condition, .. } => {
                Self::walk_array_uses(condition, &mut names)
            }
            StmtKind::DoLoop {
                condition: Some(c), ..
            } => Self::walk_array_uses(c, &mut names),
            StmtKind::For {
                start, end, step, ..
            } => {
                Self::walk_array_uses(start, &mut names);
                Self::walk_array_uses(end, &mut names);
                if let Some(st) = step {
                    Self::walk_array_uses(st, &mut names);
                }
            }
            _ => {}
        });
        names
    }

    /// Collect array names subscripted anywhere in `expr`.
    fn walk_array_uses(expr: &Expr, names: &mut Vec<String>) {
        match expr {
            Expr::ArrayAccess { name, indices } => {
                if !names.iter().any(|n| n == name) {
                    names.push(name.clone());
                }
                for i in indices {
                    Self::walk_array_uses(i, names);
                }
            }
            Expr::Unary { operand, .. } => Self::walk_array_uses(operand, names),
            Expr::Binary { left, right, .. } => {
                Self::walk_array_uses(left, names);
                Self::walk_array_uses(right, names);
            }
            Expr::FnCall { args, .. } => {
                for a in args {
                    Self::walk_array_uses(a, names);
                }
            }
            _ => {}
        }
    }

    /// The scalars a loop body assigns to, in first-assignment order.
    ///
    /// These are the ones worth a register: an accumulator's store and the
    /// next iteration's load of the same address are the loop-carried
    /// dependency, and removing it is worth far more than the instructions.
    /// A variable the body only reads costs a load per read and carries no
    /// dependency, so it is left alone and its register given to an
    /// accumulator instead.
    ///
    /// Only plain numeric scalars. A string would need its two words and its
    /// ownership rules; an `AS`-typed variable lives in different storage and
    /// is written through a different path; an array is not a scalar at all.
    fn promotable_accumulators(&self, counter: &str, body: &[Stmt]) -> Vec<String> {
        // A nested loop's control variable is written by that loop's own
        // machinery, which reads and writes its storage directly. Holding it
        // here as well would mean this loop's write-back put a stale value
        // over whatever the inner loop had left there.
        let mut inner_counters: Vec<String> = Vec::new();
        Self::walk_nested_counters(body, &mut |name| inner_counters.push(name.to_string()));

        let mut found: Vec<String> = Vec::new();
        Self::walk_assigned_scalars(body, &mut |name| {
            if name == counter
                || is_string_var(name)
                || found.iter().any(|n| n == name)
                || inner_counters.iter().any(|n| n == name)
                // Already held by an enclosing loop, which will write it back.
                || self.promoted.iter().any(|p| p.name == name)
            {
                return;
            }
            if self.typed_var(name).is_some() || self.lookup_array(name).is_some() {
                return;
            }
            // A name that is really a procedure, or a FUNCTION's own result
            // variable, is not an ordinary scalar.
            let upper = name.to_uppercase();
            if self.symbols.procs.contains_key(&upper) {
                return;
            }
            found.push(name.to_string());
        });
        found
    }

    /// Call `visit` with every scalar name assigned by a `LET` in `body`.
    fn walk_assigned_scalars(body: &[Stmt], visit: &mut impl FnMut(&str)) {
        Self::walk_body(body, &mut |stmt| {
            if let StmtKind::Let {
                name,
                indices: None,
                ..
            } = &stmt.kind
            {
                visit(name);
            }
        });
    }

    /// Call `visit` with the control variable of every loop nested in `body`.
    fn walk_nested_counters(body: &[Stmt], visit: &mut impl FnMut(&str)) {
        Self::walk_body(body, &mut |stmt| {
            if let StmtKind::For { var, .. } = &stmt.kind {
                visit(var);
            }
        });
    }

    /// Visit every statement in `body`, including nested ones.
    ///
    /// Only the statement kinds that can appear in a promotable loop are
    /// descended into; everything else is outside the allowlist that makes
    /// the loop promotable at all, so it cannot be there.
    fn walk_body(body: &[Stmt], visit: &mut impl FnMut(&Stmt)) {
        for stmt in body {
            visit(stmt);
            match &stmt.kind {
                StmtKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    Self::walk_body(then_branch, visit);
                    if let Some(eb) = else_branch {
                        Self::walk_body(eb, visit);
                    }
                }
                StmtKind::For { body, .. }
                | StmtKind::While { body, .. }
                | StmtKind::DoLoop { body, .. } => Self::walk_body(body, visit),
                _ => {}
            }
        }
    }

    /// Whether `name` is held in a register by some enclosing loop.
    fn promotion_of(&self, name: &str) -> Option<Promoted> {
        self.promoted.iter().rev().find(|p| p.name == name).cloned()
    }

    /// Where a FOR control variable lives, and the type it counts in.
    ///
    /// Resolved the same way a read of the name resolves, so that the loop
    /// writes where every other reference looks: a `DIM I AS INTEGER` counter
    /// lives in typed storage, and allocating a plain slot for it instead
    /// would leave the loop counting somewhere nothing reads.
    fn for_control_var(&mut self, name: &str) -> (Loc, DataType) {
        if let Some((loc, ty)) = self.typed_storage(name) {
            return (loc, DataType::from_type_ref(&ty));
        }
        let info = self.get_var_info(name);
        (info.loc, info.data_type)
    }

    /// The operand size a FOR control value is held at.
    fn for_ptr(ct: DataType) -> &'static str {
        match ct {
            DataType::Integer => "WORD PTR",
            DataType::Long | DataType::Single => "DWORD PTR",
            _ => "QWORD PTR",
        }
    }

    /// Load a FOR control value from `mem` into the working register.
    fn emit_for_load(&mut self, ct: DataType, mem: &str, second: bool) {
        match ct {
            DataType::Integer => {
                let r = if second { "ecx" } else { "eax" };
                emit!(self, "    movsx {}, {}", r, mem);
            }
            DataType::Long => {
                let r = if second { "ecx" } else { "eax" };
                emit!(self, "    mov {}, {}", r, mem);
            }
            DataType::Single => {
                let r = if second { "xmm1" } else { "xmm0" };
                emit!(self, "    movss {}, {}", r, mem);
            }
            _ => {
                let r = if second { "xmm1" } else { "xmm0" };
                emit!(self, "    movsd {}, {}", r, mem);
            }
        }
    }

    /// Store the primary working register into a FOR control value's slot.
    ///
    /// The narrow store is what gives an INTEGER counter its wraparound, the
    /// same as every other INTEGER assignment in the language.
    fn emit_for_store(&mut self, ct: DataType, mem: &str) {
        match ct {
            DataType::Integer => emit!(self, "    mov {}, ax", mem),
            DataType::Long => emit!(self, "    mov {}, eax", mem),
            DataType::Single => emit!(self, "    movss {}, xmm0", mem),
            _ => emit!(self, "    movsd {}, xmm0", mem),
        }
    }

    /// Evaluate a FOR limit or step into something usable every iteration.
    ///
    /// `None` is the implicit `STEP 1`. A constant costs no frame slot and no
    /// setup instruction; anything else is evaluated once into a slot, since
    /// re-evaluating it per iteration would be both slower and wrong.
    fn gen_for_bound(&mut self, expr: Option<&Expr>, ct: DataType) -> ForOperand {
        let constant = match expr {
            None => Some(1.0),
            Some(e) => self.const_double(e),
        };

        if let Some(value) = constant {
            match ct {
                // Narrowed to the counter's own type first, so a limit too
                // wide to hold behaves the way assigning it would.
                DataType::Integer => return ForOperand::Imm(i32::from(value as i32 as i16)),
                DataType::Long => return ForOperand::Imm(value as i32),
                DataType::Double => return ForOperand::Pool(self.f64_operand(value)),
                // SINGLE is the exception: the pool holds doubles and `movss`
                // cannot read one, so it takes a slot like anything else.
                _ => {}
            }
        }

        self.stack_offset -= 8;
        let offset = self.stack_offset;
        match expr {
            Some(e) => self.gen_expr_coerced(e, ct),
            None => {
                self.gen_double_const(1.0);
                self.gen_coercion(DataType::Double, ct);
            }
        }
        let mem = format!("{} [rbp + {}]", Self::for_ptr(ct), offset);
        self.emit_for_store(ct, &mem);
        ForOperand::Slot(offset)
    }

    /// Compare the counter in the primary register against `operand`.
    fn emit_for_compare(&mut self, ct: DataType, operand: &ForOperand) {
        match (ct, operand) {
            (DataType::Integer, ForOperand::Slot(off)) => {
                // The slot holds sixteen bits; the counter in eax is already
                // sign-extended, so the limit has to be too before they meet.
                emit!(self, "    movsx ecx, WORD PTR [rbp + {}]", off);
                self.emit("    cmp eax, ecx");
            }
            (DataType::Integer | DataType::Long, _) => {
                emit!(self, "    cmp eax, {}", operand.text(ct));
            }
            (DataType::Single, _) => {
                emit!(self, "    ucomiss xmm0, {}", operand.text(ct));
            }
            _ => emit!(self, "    ucomisd xmm0, {}", operand.text(ct)),
        }
    }

    /// Add `operand` to the counter in the primary register.
    ///
    /// The add is done at the counter's own width, so that a step past the
    /// type's range sets the overflow flag. Without that check the counter
    /// simply wraps and never passes its limit: `FOR I% = 32760 TO 32767`
    /// would run forever rather than reporting the Overflow that GW-BASIC
    /// reports. `--unsafe` drops the check and takes the wraparound.
    fn emit_for_add(&mut self, ct: DataType, operand: &ForOperand) {
        match ct {
            DataType::Integer => {
                emit!(self, "    add ax, {}", operand.text(ct));
                self.emit_check("jo", RtError::Overflow);
            }
            DataType::Long => {
                emit!(self, "    add eax, {}", operand.text(ct));
                self.emit_check("jo", RtError::Overflow);
            }
            DataType::Single => {
                emit!(self, "    addss xmm0, {}", operand.text(ct));
            }
            _ => emit!(self, "    addsd xmm0, {}", operand.text(ct)),
        }
    }

    /// The 32-bit name of a promoted counter's register.
    fn counter_reg32(reg: &str) -> String {
        format!("{}d", reg)
    }

    /// Move the freshly computed start value into the counter's register.
    fn emit_counter_init(&mut self, ct: DataType, reg: &str) {
        match ct {
            // Narrowed on the way in, exactly as the store to a 16-bit slot
            // would have narrowed it.
            DataType::Integer => emit!(self, "    movsx {}, ax", Self::counter_reg32(reg)),
            DataType::Long => emit!(self, "    mov {}, eax", Self::counter_reg32(reg)),
            // movaps rather than movss: a register-to-register movss merges
            // into the destination, so it would depend on what was there.
            DataType::Single => emit!(self, "    movaps {}, xmm0", reg),
            _ => emit!(self, "    movapd {}, xmm0", reg),
        }
    }

    /// Write a promoted counter back to the variable's storage.
    fn emit_counter_writeback(&mut self, ct: DataType, reg: &str, mem: &str) {
        match ct {
            DataType::Integer => emit!(self, "    mov {}, {}w", mem, reg),
            DataType::Long => emit!(self, "    mov {}, {}", mem, Self::counter_reg32(reg)),
            DataType::Single => emit!(self, "    movss {}, {}", mem, reg),
            _ => emit!(self, "    movsd {}, {}", mem, reg),
        }
    }

    /// Compare a promoted counter against `operand`.
    fn emit_counter_compare(&mut self, ct: DataType, reg: &str, operand: &ForOperand) {
        match (ct, operand) {
            (DataType::Integer, ForOperand::Slot(off)) => {
                emit!(self, "    movsx ecx, WORD PTR [rbp + {}]", off);
                emit!(self, "    cmp {}, ecx", Self::counter_reg32(reg));
            }
            (DataType::Integer | DataType::Long, _) => {
                let text = operand.text(ct);
                emit!(self, "    cmp {}, {}", Self::counter_reg32(reg), text);
            }
            (DataType::Single, _) => {
                let text = operand.text(ct);
                emit!(self, "    ucomiss {}, {}", reg, text);
            }
            _ => {
                let text = operand.text(ct);
                emit!(self, "    ucomisd {}, {}", reg, text);
            }
        }
    }

    /// Step a promoted counter. See [`Self::emit_for_add`] on the width.
    fn emit_counter_add(&mut self, ct: DataType, reg: &str, operand: &ForOperand) {
        let r32 = Self::counter_reg32(reg);
        let text = operand.text(ct);
        match ct {
            DataType::Integer => {
                emit!(self, "    add {}w, {}", reg, text);
                self.emit_check("jo", RtError::Overflow);
                // The narrowing store used to keep an INTEGER counter within
                // sixteen bits for free; in a register it has to be said.
                emit!(self, "    movsx {}, {}w", r32, reg);
            }
            DataType::Long => {
                emit!(self, "    add {}, {}", r32, text);
                self.emit_check("jo", RtError::Overflow);
            }
            DataType::Single => emit!(self, "    addss {}, {}", reg, text),
            _ => emit!(self, "    addsd {}, {}", reg, text),
        }
    }

    /// Load a promoted variable's storage into its register, entering a loop.
    fn emit_promotion_load(&mut self, p: &Promoted) {
        match p.ty {
            DataType::Integer => emit!(
                self,
                "    movsx {}, {}",
                Self::counter_reg32(&p.reg),
                p.loc.at("WORD PTR", 0)
            ),
            DataType::Long => emit!(
                self,
                "    mov {}, {}",
                Self::counter_reg32(&p.reg),
                p.loc.at("DWORD PTR", 0)
            ),
            DataType::Single => emit!(self, "    movss {}, {}", p.reg, p.loc.at("DWORD PTR", 0)),
            _ => emit!(self, "    movsd {}, {}", p.reg, p.loc.q(0)),
        }
    }

    /// Write a promoted variable's register back to its storage, leaving.
    fn emit_promotion_store(&mut self, p: &Promoted) {
        let mem = p.loc.at(Self::for_ptr(p.ty), 0);
        self.emit_counter_writeback(p.ty, &p.reg.clone(), &mem);
    }

    /// Move a freshly computed value from the working register into a
    /// promoted variable's register.
    fn emit_promotion_assign(&mut self, p: &Promoted) {
        // Narrowed on the way in, exactly as the store to its slot would have
        // narrowed it, so an INTEGER still wraps at sixteen bits.
        self.emit_counter_init(p.ty, &p.reg.clone());
    }

    /// Read a promoted counter into the register an expression expects.
    fn emit_counter_read(&mut self, ct: DataType, reg: &str) {
        match ct {
            DataType::Integer | DataType::Long => {
                emit!(self, "    mov eax, {}", Self::counter_reg32(reg))
            }
            DataType::Single => emit!(self, "    movaps xmm0, {}", reg),
            _ => emit!(self, "    movapd xmm0, {}", reg),
        }
    }

    /// Branch to `target` when a step held only at run time is negative.
    ///
    /// Only reached for a step that is not a constant, which is also the only
    /// case where `operand` is a slot. Clobbers the secondary registers, so
    /// the counter already loaded in the primary one survives.
    fn emit_for_test_step_sign(&mut self, ct: DataType, operand: &ForOperand, target: &str) {
        let mem = operand.text(ct);
        match ct {
            DataType::Integer | DataType::Long => {
                emit!(self, "    cmp {}, 0", mem);
                emit!(self, "    jl {}", target);
            }
            DataType::Single => {
                emit!(self, "    movss xmm1, {}", mem);
                self.emit("    xorps xmm2, xmm2");
                self.emit("    ucomiss xmm1, xmm2");
                emit!(self, "    jb {}", target);
            }
            _ => {
                emit!(self, "    movsd xmm1, {}", mem);
                self.emit("    xorpd xmm2, xmm2");
                self.emit("    ucomisd xmm1, xmm2");
                emit!(self, "    jb {}", target);
            }
        }
    }

    /// The branch that leaves the loop: past the limit going up, or below it
    /// going down. Integers compare signed; ucomis* reports through CF and ZF,
    /// so a floating counter reads unsigned.
    fn for_exit_branch(ct: DataType, ascending: bool) -> &'static str {
        match (ct.is_integer(), ascending) {
            (true, true) => "jg",
            (true, false) => "jl",
            (false, true) => "ja",
            (false, false) => "jb",
        }
    }

    /// Evaluate a computed dimension into `rax`, checked against `rank`.
    ///
    /// Dimensions are 1-based, so an unsigned compare of `dim - 1` against the
    /// rank catches zero and negative values in the same branch.
    fn gen_dim_index(&mut self, dim: &Expr, rank: i64) {
        let ty = self.gen_expr(dim);
        self.gen_coercion(ty, DataType::Long);
        self.emit("    movsxd rax, eax");
        self.emit("    dec rax");
        emit!(self, "    cmp rax, {}", rank);
        self.emit_check("jae", RtError::Subscript);
        self.emit("    inc rax");
    }

    /// The scope semantic analysis would name for the code being generated.
    fn sema_scope(&self) -> SemaScope {
        match &self.current_proc {
            Some(p) => SemaScope::Proc(p.clone()),
            None => SemaScope::Module,
        }
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
        emit!(self, "    {} {}", cond, label);
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
            emit!(self, "    lea {}, [rip + {}]", Self::arg_reg(0), sym);
            emit!(self, "    mov {}, {}", Self::arg_reg(1), line);
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
        emit!(self, "    jmp {}", check);
        self.emit_label(&body);
        self.emit("    mov QWORD PTR [r11], 0");
        self.emit("    add r11, 8");
        self.emit_label(&check);
        self.emit("    cmp r11, r10");
        emit!(self, "    jb {}", body);
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
                        emit!(self, "    mov r11, QWORD PTR [rbp + {}]", 16 + 8 * i as i32);
                        "r11".to_string()
                    }
                };
                emit!(self, "    mov r10, {}", src);
                for w in 0..words {
                    emit!(self, "    mov rax, QWORD PTR [r10 + {}]", w * 8);
                    emit!(self, "    mov {}, rax", loc.q(w));
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
                    emit!(self, "    mov {}, {}", loc.q(0), p);
                    let l = fetch(self, place.len.expect("string parameter has a length slot"));
                    emit!(self, "    mov {}, {}", loc.q(1), l);
                }
                DataType::Double => {
                    let p = fetch(self, place.ptr);
                    emit!(self, "    mov {}, {}", loc.q(0), p);
                }
                // Numeric arguments arrive as f64 bit patterns; narrow to the
                // declared type. rax/xmm0 are safe scratch: neither is an
                // argument register on either ABI.
                DataType::Single => {
                    let p = fetch(self, place.ptr);
                    emit!(self, "    movq xmm0, {}", p);
                    self.emit("    cvtsd2ss xmm0, xmm0");
                    emit!(self, "    movss {}, xmm0", loc.at("DWORD PTR", 0));
                }
                DataType::Integer | DataType::Long => {
                    let p = fetch(self, place.ptr);
                    emit!(self, "    movq xmm0, {}", p);
                    self.emit("    cvttsd2si eax, xmm0");
                    let (size, reg) = if place.ty == DataType::Integer {
                        ("WORD PTR", "ax")
                    } else {
                        ("DWORD PTR", "eax")
                    };
                    emit!(self, "    mov {}, {}", loc.at(size, 0), reg);
                }
            }
        }

        // If function, allocate return value slot
        if is_function {
            let data_type = self.proc_return_type(name);
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
                    emit!(self, "    movsx eax, {}", loc.at("WORD PTR", 0));
                }
                DataType::Long => {
                    emit!(self, "    mov eax, {}", loc.at("DWORD PTR", 0));
                }
                DataType::Single => {
                    emit!(self, "    movss xmm0, {}", loc.at("DWORD PTR", 0));
                }
                DataType::Double => {
                    emit!(self, "    movsd xmm0, {}", loc.q(0));
                }
                DataType::String => {
                    // Load string (ptr, len) into rax, rdx
                    emit!(self, "    mov rax, {}", loc.q(0));
                    emit!(self, "    mov rdx, {}", loc.q(1));
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
                                emit!(self, "    mov rax, {}", src_loc.q(w));
                                emit!(self, "    mov {}, rax", loc.q(w));
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
                                emit!(self, "    mov rax, QWORD PTR [r10 + {}]", w * 8);
                                emit!(self, "    mov {}, rax", loc.q(w));
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
                    // Evaluate expression and get its type. A Double target is
                    // worth knowing about first: `X# = 2` is common, and going
                    // through gen_expr would put 2 in eax only to widen it.
                    let target_type = self.get_var_info(name).data_type;
                    let expr_type = if target_type == DataType::Double {
                        self.gen_expr_to_double(value);
                        DataType::Double
                    } else {
                        self.gen_expr(value)
                    };
                    let var_info = self.get_var_info(name);

                    // Coerce to target type
                    self.gen_coercion(expr_type, var_info.data_type);

                    // An enclosing loop may be holding this name in a
                    // register. Writing its storage instead would be lost:
                    // the register is where every read of it now looks, and
                    // it is what gets written back when the loop ends.
                    if let Some(p) = self.promotion_of(name) {
                        self.emit_promotion_assign(&p);
                        return;
                    }

                    // Store based on target type
                    let loc = &var_info.loc;
                    match var_info.data_type {
                        DataType::Integer => {
                            emit!(self, "    mov {}, ax", loc.at("WORD PTR", 0));
                        }
                        DataType::Long => {
                            emit!(self, "    mov {}, eax", loc.at("DWORD PTR", 0));
                        }
                        DataType::Single => {
                            emit!(self, "    movss {}, xmm0", loc.at("DWORD PTR", 0));
                        }
                        DataType::Double => {
                            emit!(self, "    movsd {}, xmm0", loc.q(0));
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
                    self.emit_arg_imm(0, CONSOLE);
                    self.emit("    call _rt_file_print_newline");
                }
            }

            StmtKind::Print {
                file_num,
                items,
                newline,
                write,
                ..
            } => {
                let sink = self.gen_sink(file_num.as_ref());
                let mut first = true;
                for item in items {
                    match item {
                        PrintItem::Expr(expr) => {
                            // WRITE separates values with commas and quotes
                            // strings; PRINT emits them bare.
                            if *write {
                                if !first {
                                    self.emit_arg_file_num(0, &sink);
                                    self.emit_arg_imm(1, ASCII_COMMA);
                                    self.emit("    call _rt_file_print_char");
                                }
                                self.gen_write_expr(expr, &sink);
                            } else {
                                self.gen_print_expr(expr, &sink);
                            }
                            first = false;
                        }
                        PrintItem::Tab => {
                            // For WRITE the comma is only a separator, and one
                            // is already emitted before each value; emitting a
                            // tab as well wrote "10\t,20".
                            if !*write {
                                self.emit_arg_file_num(0, &sink);
                                self.emit_arg_imm(1, ASCII_TAB);
                                self.emit("    call _rt_file_print_char");
                            }
                        }
                        PrintItem::Empty => {}
                    }
                }
                if *newline {
                    self.emit_arg_file_num(0, &sink);
                    self.emit("    call _rt_file_print_newline");
                }
            }

            StmtKind::Input {
                prompt,
                query,
                vars,
                file_num,
            } => {
                // The question mark is part of the prompt text, so it costs
                // nothing at run time and needs no runtime support.
                let text = match (prompt.as_deref(), query) {
                    (Some(p), true) => Some(format!("{}? ", p)),
                    (Some(p), false) => Some(p.to_string()),
                    (None, true) => Some("? ".to_string()),
                    (None, false) => None,
                };
                if let Some(text) = text {
                    self.gen_console_prompt(&text);
                }
                let fnum = file_num.as_ref().map(|e| self.gen_file_num(e));
                for var in vars {
                    let rt = match (&fnum, is_string_var(&var.name)) {
                        (Some(f), string) => {
                            self.emit_arg_file_num(0, f);
                            if string {
                                "_rt_file_input_string"
                            } else {
                                "_rt_file_input_number"
                            }
                        }
                        (None, true) => "_rt_input_string",
                        (None, false) => "_rt_input_number",
                    };
                    emit!(self, "    call {}", rt);
                    self.gen_store_lvalue(var);
                }
            }

            StmtKind::LineInput {
                prompt,
                var,
                file_num,
            } => {
                if let Some(pstr) = prompt {
                    self.gen_console_prompt(pstr);
                }
                match file_num {
                    Some(e) => {
                        // LINE INPUT # takes the whole line; INPUT # takes one
                        // comma-delimited field, which is a different reader.
                        let fnum = self.gen_file_num(e);
                        self.emit_arg_file_num(0, &fnum);
                        self.emit("    call _rt_file_line_input");
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

                self.gen_condition(condition, &else_label, false);

                for s in then_branch {
                    self.gen_stmt(s);
                }
                emit!(self, "    jmp {}", end_label);

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
                // The control variable counts in its own declared type, and is
                // read back through that type's own load by everything else in
                // the program -- so it has to be written through the matching
                // store. Counting in Double regardless, as this used to, meant
                // `FOR I% = 1 TO 3` wrote the bit pattern of 1.0 and every read
                // of I% took the low sixteen bits of it, which are zero.
                let (var_loc, ct) = self.for_control_var(var);
                let var_mem = var_loc.at(Self::for_ptr(ct), 0);

                // A counter kept in memory makes the loop-carried dependency a
                // store followed by the next iteration's load of the same
                // address -- around ten cycles of store-to-load forwarding that
                // nothing else in the loop can hide. Held in a register it is
                // one add. Only loops that call nothing qualify, since System V
                // has no callee-saved XMM register for a Double counter to
                // survive a call in, and a called procedure could reach a
                // module-level counter by name.
                let promotable =
                    self.body_allows_register_counter(var, body, start, end, step.as_ref());
                let counter_reg = if promotable {
                    self.free_promotion_register(ct).map(str::to_string)
                } else {
                    None
                };

                // r12-r15 belong to this function's caller, so they are saved
                // across the loop. Sixteen bytes rather than eight because a
                // bounds check inside the loop can still reach a trampoline
                // that calls the runtime, and it must find rsp aligned.
                if let Some(reg) = &counter_reg {
                    if ct.is_integer() {
                        self.emit("    sub rsp, 16        # save a counter register");
                        emit!(self, "    mov QWORD PTR [rsp], {}", reg);
                    }
                }

                // Initialize the control variable.
                self.gen_expr_coerced(start, ct);
                match &counter_reg {
                    Some(reg) => {
                        let reg = reg.clone();
                        self.emit_counter_init(ct, &reg)
                    }
                    None => self.emit_for_store(ct, &var_mem),
                }

                // The limit and the step are evaluated once. A constant one
                // needs no slot at all: it becomes the compare's or the add's
                // own operand.
                let end_operand = self.gen_for_bound(Some(end), ct);
                let step_operand = self.gen_for_bound(step.as_ref(), ct);

                // The same argument that puts the counter in a register puts
                // the body's accumulators there: `T = T + ...` has exactly the
                // same store-then-reload dependency, and usually a longer one,
                // since the add is on the critical path too.
                //
                // Loaded after the bounds, which are evaluated in memory, and
                // before the loop label, so a variable the loop never reaches
                // is still written back unchanged.
                let mut accumulators: Vec<Promoted> = Vec::new();
                if promotable {
                    for name in self.promotable_accumulators(var, body) {
                        if accumulators.len() == MAX_PROMOTED_ACCUMULATORS {
                            break;
                        }
                        let info = self.get_var_info(&name);
                        // Ask with the counter already accounted for, so the
                        // two cannot be given the same register.
                        let taken: Vec<String> = counter_reg.iter().cloned().collect();
                        let reg = {
                            let pool: &[&'static str] = if info.data_type.is_integer() {
                                &PROMO_GPRS
                            } else {
                                &PROMO_XMMS
                            };
                            pool.iter().copied().find(|r| {
                                !taken.iter().any(|t| t == r)
                                    && !accumulators.iter().any(|a| a.reg == *r)
                                    && !self.promoted.iter().any(|p| p.reg == *r)
                            })
                        };
                        let Some(reg) = reg else { break };
                        accumulators.push(Promoted {
                            name,
                            reg: reg.to_string(),
                            ty: info.data_type,
                            loc: info.loc,
                        });
                    }
                }
                for p in &accumulators.clone() {
                    if p.ty.is_integer() {
                        self.emit("    sub rsp, 16        # save an accumulator register");
                        emit!(self, "    mov QWORD PTR [rsp], {}", p.reg);
                    }
                    self.emit_promotion_load(p);
                }

                // An array's element pointer and bounds cannot change while
                // the loop runs -- DIM and REDIM are outside the allowlist --
                // yet every subscript re-read them: two loads for the pointer,
                // since the null check and the address each fetched it, and
                // one per bound compare. Hoisting those was worth 1.45x on a
                // loop doing four subscripts, which is most of what the bounds
                // checking costs in the first place.
                let hoisted = if promotable {
                    // The counter and the accumulators have registers but are
                    // not on the promoted stack yet -- that happens as the
                    // body is entered -- so they are named explicitly here.
                    let mut claimed: Vec<String> = counter_reg.iter().cloned().collect();
                    claimed.extend(accumulators.iter().map(|a| a.reg.clone()));
                    self.hoist_array_descriptors(body, &claimed)
                } else {
                    Vec::new()
                };
                for h in &hoisted.clone() {
                    self.emit_hoist_loads(h);
                }

                // Which way the loop runs is a property of the step's sign. It
                // is almost always written into the program, so it is almost
                // always decided here rather than re-tested on every iteration.
                let direction = match step {
                    None => Some(true),
                    Some(s) => self.const_double(s).map(|v| v >= 0.0),
                };

                self.emit_label(&start_label);
                let compare = |s: &mut Self| match &counter_reg {
                    Some(reg) => {
                        let reg = reg.clone();
                        s.emit_counter_compare(ct, &reg, &end_operand)
                    }
                    None => {
                        s.emit_for_load(ct, &var_mem, false);
                        s.emit_for_compare(ct, &end_operand);
                    }
                };

                match direction {
                    Some(ascending) => {
                        compare(self);
                        emit!(
                            self,
                            "    {} {}",
                            Self::for_exit_branch(ct, ascending),
                            end_label
                        );
                    }
                    None => {
                        // Step known only at run time, so both directions have
                        // to be present. Only this shape pays for the test.
                        let neg = format!(".Lfor_neg_{}", self.label_counter);
                        let body_label = format!(".Lfor_body_{}", self.label_counter);
                        self.label_counter += 1;

                        self.emit_for_test_step_sign(ct, &step_operand, &neg);
                        compare(self);
                        emit!(
                            self,
                            "    {} {}",
                            Self::for_exit_branch(ct, true),
                            end_label
                        );
                        emit!(self, "    jmp {}", body_label);

                        self.emit_label(&neg);
                        compare(self);
                        emit!(
                            self,
                            "    {} {}",
                            Self::for_exit_branch(ct, false),
                            end_label
                        );
                        self.emit_label(&body_label);
                    }
                }

                // Body. While it runs, a read or a write of any promoted name
                // resolves to its register rather than to its storage.
                let promoted_here = accumulators.len() + usize::from(counter_reg.is_some());
                let hoisted_here = hoisted.len();
                for h in &hoisted {
                    self.hoisted_arrays.push(h.clone());
                }
                if let Some(reg) = &counter_reg {
                    self.promoted.push(Promoted {
                        name: var.clone(),
                        reg: reg.clone(),
                        ty: ct,
                        loc: var_loc.clone(),
                    });
                }
                for p in &accumulators {
                    self.promoted.push(p.clone());
                }
                self.loop_stack.push((true, end_label.clone()));
                for s in body {
                    self.gen_stmt(s);
                }
                self.loop_stack.pop();
                self.promoted.truncate(self.promoted.len() - promoted_here);
                self.hoisted_arrays
                    .truncate(self.hoisted_arrays.len() - hoisted_here);

                // Increment. When the counter is in memory it is reloaded
                // rather than carried over from the compare above, because the
                // body is allowed to assign to it and BASIC programs do -- the
                // register form is only reached for bodies that provably do not.
                match &counter_reg {
                    Some(reg) => {
                        let reg = reg.clone();
                        self.emit_counter_add(ct, &reg, &step_operand);
                    }
                    None => {
                        self.emit_for_load(ct, &var_mem, false);
                        self.emit_for_add(ct, &step_operand);
                        self.emit_for_store(ct, &var_mem);
                    }
                }
                emit!(self, "    jmp {}", start_label);

                self.emit_label(&end_label);

                // Every route out of the loop passes here, EXIT FOR included,
                // so this is where the variables become visible again. In
                // reverse order, so the saved registers come off the stack in
                // the order they went on.
                for h in hoisted.iter().rev() {
                    self.emit_hoist_restores(h);
                }
                for p in accumulators.iter().rev() {
                    self.emit_promotion_store(p);
                    if p.ty.is_integer() {
                        emit!(self, "    mov {}, QWORD PTR [rsp]", p.reg);
                        self.emit("    add rsp, 16");
                    }
                }
                if let Some(reg) = &counter_reg {
                    let reg = reg.clone();
                    self.emit_counter_writeback(ct, &reg, &var_mem);
                    if ct.is_integer() {
                        emit!(self, "    mov {}, QWORD PTR [rsp]", reg);
                        self.emit("    add rsp, 16");
                    }
                }
            }

            StmtKind::While { condition, body } => {
                let start_label = self.new_label("while");
                let end_label = self.new_label("endwhile");

                self.emit_label(&start_label);
                self.gen_condition(condition, &end_label, false);

                // WHILE is a DO-family loop for EXIT DO purposes.
                self.loop_stack.push((false, end_label.clone()));
                for s in body {
                    self.gen_stmt(s);
                }
                self.loop_stack.pop();
                emit!(self, "    jmp {}", start_label);

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
                        // DO WHILE leaves when the condition is false;
                        // DO UNTIL leaves when it is true.
                        self.gen_condition(cond, &end_label, *is_until);
                    }
                }

                self.loop_stack.push((false, end_label.clone()));
                for s in body {
                    self.gen_stmt(s);
                }
                self.loop_stack.pop();

                if !*cond_at_start {
                    if let Some(cond) = condition {
                        // The senses invert against the pre-test form: this
                        // branch goes back into the loop rather than out of
                        // it. LOOP WHILE repeats while true, LOOP UNTIL
                        // repeats while false.
                        self.gen_condition(cond, &start_label, !*is_until);
                    } else {
                        emit!(self, "    jmp {}", start_label);
                    }
                } else {
                    emit!(self, "    jmp {}", start_label);
                }

                self.emit_label(&end_label);
            }

            StmtKind::Goto(target) => {
                let label = match target {
                    GotoTarget::Line(n) => format!("_line_{}", n),
                    GotoTarget::Label(s) => format!("_label_{}", mangle(s)),
                };
                emit!(self, "    jmp {}", label);
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
                self.emit_check("jb", RtError::GosubOverflow);
                // Push return address to GOSUB stack
                emit!(self, "    lea rax, [rip + {}]", ret_label);
                self.emit("    mov QWORD PTR [rcx], rax");
                self.emit("    mov QWORD PTR [rip + _gosub_sp], rcx");
                emit!(self, "    jmp {}", label);
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
                    emit!(self, "    cmp rax, {}", i + 1);
                    emit!(self, "    je {}", label);
                }
            }

            StmtKind::OnGosub { expr, targets } => {
                // One return address serves the whole statement: whichever
                // subroutine runs, RETURN comes back to the same place, which
                // is the statement after this one.
                //
                // That address must not be pushed when the selector picks
                // nothing, or a GOSUB that never happened would leave a frame
                // behind and the next RETURN would jump to it. So the range is
                // checked first, and out-of-range branches past the push --
                // to the very label the return address points at, since
                // "matched nothing" and "came back" continue identically.
                let expr_type = self.gen_expr(expr);
                if expr_type.is_integer() {
                    self.emit("    movsxd rax, eax");
                } else {
                    self.emit("    cvttsd2si rax, xmm0");
                }
                // The push sequence below needs rax and rcx, so the selector
                // is parked in r8: caller-saved on both ABIs, and nothing is
                // called between here and the dispatch.
                self.emit("    mov r8, rax");

                let after = self.new_label("on_gosub_ret");
                self.emit("    cmp r8, 1");
                emit!(self, "    jl {}", after);
                emit!(self, "    cmp r8, {}", targets.len());
                emit!(self, "    jg {}", after);

                self.emit("    mov rcx, QWORD PTR [rip + _gosub_sp]");
                self.emit("    sub rcx, 8");
                self.emit("    lea rax, [rip + _gosub_stack]");
                self.emit("    cmp rcx, rax");
                self.emit_check("jb", RtError::GosubOverflow);
                emit!(self, "    lea rax, [rip + {}]", after);
                self.emit("    mov QWORD PTR [rcx], rax");
                self.emit("    mov QWORD PTR [rip + _gosub_sp], rcx");

                for (i, target) in targets.iter().enumerate() {
                    let label = match target {
                        GotoTarget::Line(n) => format!("_line_{}", n),
                        GotoTarget::Label(s) => format!("_label_{}", mangle(s)),
                    };
                    emit!(self, "    cmp r8, {}", i + 1);
                    emit!(self, "    je {}", label);
                }
                self.emit_label(&after);
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
                        .get(normalized(name))
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
                    emit!(self, "    jmp {}", label);
                }
            }

            StmtKind::ExitProc => {
                if let Some(label) = self.proc_exit_label.clone() {
                    emit!(self, "    jmp {}", label);
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
                    emit!(self, "    mov QWORD PTR [rbp + {}], rax", temp_offset);
                    emit!(self, "    mov QWORD PTR [rbp + {}], rdx", temp_offset + 8);
                } else {
                    self.gen_coercion(expr_type, DataType::Double);
                    self.stack_offset -= 8;
                    temp_offset = self.stack_offset;
                    emit!(self, "    movsd QWORD PTR [rbp + {}], xmm0", temp_offset);
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
                        emit!(self, "    jmp {}", next_case_label);
                        self.emit_label(&body_label);
                    }
                    // CASE ELSE (None) falls through without comparison

                    // Generate case body
                    for stmt in body {
                        self.gen_stmt(stmt);
                    }

                    // Jump to end (skip remaining cases)
                    if i + 1 < cases.len() {
                        emit!(self, "    jmp {}", end_label);
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
                reclen,
            } => {
                // _rt_file_open(filename_ptr, filename_len, mode, file_num), or
                // _rt_file_open_random(filename_ptr, filename_len, file_num,
                // record_length) -- which carries no mode, because being the
                // random entry point is the mode.
                //
                // Four arguments each, and that is a hard limit rather than a
                // preference: Win64 passes only four in registers, so a fifth
                // would have to go on the stack. Folding the mode away is what
                // keeps the record length in a register.
                let fnum = self.gen_file_num(file_num);
                let rec = match (mode, reclen) {
                    // GW-BASIC's default record length.
                    (FileMode::Random, None) => Some(FileNum::Imm(DEFAULT_RECLEN)),
                    (FileMode::Random, Some(e)) => Some(self.gen_file_num_like(e)),
                    _ => None,
                };
                self.gen_expr(filename);
                self.emit_arg_reg(0, "rax"); // filename ptr
                self.emit_arg_reg(1, "rdx"); // filename len
                match rec {
                    Some(r) => {
                        self.emit_arg_file_num(2, &fnum);
                        self.emit_arg_file_num(3, &r);
                        self.emit("    call _rt_file_open_random");
                    }
                    None => {
                        let mode_num = match mode {
                            FileMode::Input => 0,
                            FileMode::Output => 1,
                            FileMode::Append => 2,
                            FileMode::Random => unreachable!("a random open took the other path"),
                        };
                        self.emit_arg_imm(2, mode_num);
                        self.emit_arg_file_num(3, &fnum);
                        self.emit("    call _rt_file_open");
                    }
                }
            }

            StmtKind::Field { file_num, fields } => {
                // Each clause binds its variable to (buffer + offset, width).
                // The offsets accumulate across the statement, so they are
                // tracked by the runtime rather than recomputed here: a width
                // may be any expression.
                let fnum = self.gen_file_num(file_num);
                self.emit_arg_file_num(0, &fnum);
                self.emit("    call _rt_field_reset");
                for f in fields {
                    let width = self.gen_file_num_like(&f.width);
                    self.emit_arg_file_num(0, &fnum);
                    self.emit_arg_file_num(1, &width);
                    self.emit("    call _rt_field_next");
                    // rax/rdx now hold the slice; bind without copying, which
                    // is what makes the variable alias the record buffer.
                    self.gen_bind_alias(&f.target);
                }
            }

            StmtKind::SetField {
                target,
                value,
                right,
            } => {
                // The destination is read first: LSET writes *through* the
                // field's existing pointer, so the target keeps its address and
                // width while the bytes underneath change.
                self.gen_expr(value);
                emit!(self, "    sub rsp, {}", STACK_TEMP_SPACE);
                self.emit("    mov QWORD PTR [rsp], rax");
                self.emit("    mov QWORD PTR [rsp + 8], rdx");
                self.gen_load_lvalue_string(target);
                self.emit_arg_reg(0, "rax"); // dest ptr
                self.emit_arg_reg(1, "rdx"); // dest len
                // The source is loaded straight into its argument registers
                // rather than staged through rax/rdx: on System V argument 2
                // *is* rdx, so moving the pointer there first would destroy the
                // length that argument 3 has yet to read.
                let src_ptr = Self::arg_reg(2);
                let src_len = Self::arg_reg(3);
                emit!(self, "    mov {}, QWORD PTR [rsp]", src_ptr);
                emit!(self, "    mov {}, QWORD PTR [rsp + 8]", src_len);
                emit!(self, "    add rsp, {}", STACK_TEMP_SPACE);
                if *right {
                    self.emit("    call _rt_rset");
                } else {
                    self.emit("    call _rt_lset");
                }
            }

            StmtKind::GetPut {
                file_num,
                record,
                is_put,
            } => {
                let fnum = self.gen_file_num(file_num);
                // Record 0 means "the one after the last", which is how the
                // runtime reads a missing record number.
                let rec = match record {
                    Some(e) => self.gen_file_num_like(e),
                    None => FileNum::Imm(0),
                };
                self.emit_arg_file_num(0, &fnum);
                self.emit_arg_file_num(1, &rec);
                self.emit_arg_imm(2, self.current_line as i64);
                if *is_put {
                    self.emit("    call _rt_file_put");
                } else {
                    self.emit("    call _rt_file_get");
                }
            }

            StmtKind::Lock {
                file_num,
                range,
                is_unlock,
            } => {
                let fnum = self.gen_file_num(file_num);
                // A missing range is the whole file, which the runtime reads as
                // first record 0.
                let (start, end) = match range {
                    Some((s, e)) => {
                        let s = self.gen_file_num_like(s);
                        let e = match e {
                            Some(e) => self.gen_file_num_like(e),
                            // `LOCK #1, 5` locks exactly record 5.
                            None => s.clone(),
                        };
                        (s, e)
                    }
                    None => (FileNum::Imm(0), FileNum::Imm(0)),
                };
                self.emit_arg_file_num(0, &fnum);
                self.emit_arg_file_num(1, &start);
                self.emit_arg_file_num(2, &end);
                self.emit_arg_imm(3, self.current_line as i64);
                if *is_unlock {
                    self.emit("    call _rt_unlock");
                } else {
                    self.emit("    call _rt_lock");
                }
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
                        emit!(self, "    mov eax, {}", v);
                        DataType::Long
                    }
                    // Wider than LONG: emit as a Double rather than truncating
                    // to 32 bits, which silently turned 1000000000000001 into
                    // -1530494975. The lexer widens such literals already; this
                    // also covers values arriving from DATA.
                    Err(_) => {
                        self.gen_double_const(*n as f64);
                        DataType::Double
                    }
                },
                Literal::Float(f) => {
                    // Load as double into xmm0
                    self.gen_double_const(*f);
                    DataType::Double
                }
                Literal::String(s) => {
                    let idx = self.add_string_literal(s);
                    emit!(self, "    lea rax, [rip + _str_{}]", idx);
                    // A literal's length cannot approach 2^32, and writing edx
                    // zeroes the upper half, so the narrow form is equivalent.
                    emit!(self, "    mov edx, {}", s.len());
                    DataType::String
                }
            },

            Expr::Variable(name) => {
                // A CONST is substituted with its folded value.
                if let Some(lit) = self.symbols.consts.get(normalized(name)).cloned() {
                    return self.gen_expr(&Expr::Literal(lit));
                }

                // An enclosing FOR may be holding this name in a register
                // instead of in its storage, in which case that register is
                // where its current value is. Checked before anything else,
                // because the storage is stale until the loop ends.
                if let Some(p) = self.promotion_of(name) {
                    self.emit_counter_read(p.ty, &p.reg);
                    return p.ty;
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

                // The same applies to the builtins that take no argument.
                if crate::sema::is_zero_arg_builtin(&upper) {
                    self.gen_fn_call(&upper, &[]);
                    return self.fn_return_type(&upper);
                }

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
                        emit!(self, "    movsx eax, {}", loc.at("WORD PTR", 0));
                    }
                    DataType::Long => {
                        emit!(self, "    mov eax, {}", loc.at("DWORD PTR", 0));
                    }
                    DataType::Single => {
                        emit!(self, "    movss xmm0, {}", loc.at("DWORD PTR", 0));
                    }
                    DataType::Double => {
                        emit!(self, "    movsd xmm0, {}", loc.q(0));
                    }
                    DataType::String => {
                        emit!(self, "    mov rax, {}", loc.q(0));
                        emit!(self, "    mov rdx, {}", loc.q(1));
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
                                // The mask comes from the pool rather than a
                                // 64-bit immediate, so rax is left alone.
                                //
                                // It is loaded into a register rather than
                                // named as xorpd's operand: the memory form of
                                // a packed SSE instruction reads sixteen bytes
                                // and requires them aligned, and pool entries
                                // are eight bytes on an eight-byte boundary.
                                let mask = self.f64_operand(-0.0);
                                emit!(self, "    movsd xmm1, {}", mask);
                                self.emit("    xorpd xmm0, xmm1");
                            }
                            operand_type
                        }
                    }
                    UnaryOp::Not => {
                        // NOT is a bitwise complement, like AND, OR and XOR
                        // beside it: `NOT 12` is -13, not 0.
                        //
                        // It used to emit `sete al / movzx / neg`, i.e. "0 gives
                        // -1, anything else gives 0" -- a *logical* not. That
                        // agrees with the bitwise answer only when the operand
                        // is already 0 or -1, which every test used, so the
                        // difference stayed invisible while `12 AND 10` was
                        // correctly bitwise and `NOT 12` was not.
                        //
                        // GW-BASIC complements a 16-bit two's-complement
                        // integer; we use 32-bit, as the other three do.
                        if !operand_type.is_integer() {
                            self.emit_typed(
                                operand_type,
                                "",
                                "    cvttss2si eax, xmm0",
                                "    cvttsd2si eax, xmm0",
                            );
                        }
                        self.emit("    not eax");
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
            emit!(self, "    sub rsp, {}", STACK_TEMP_SPACE);
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
            emit!(self, "    add rsp, {}", STACK_TEMP_SPACE);
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
            self.gen_string_compare(left, right);

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
            emit!(self, "    {} al", setcc);
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

        // A constant on the right needs no register of its own, and evaluating
        // it cannot disturb the left operand, so the spill below is pure
        // overhead for what is far and away the commonest shape in BASIC.
        if self.gen_binary_const_rhs(op, left, right, work_type) {
            self.expr_depth -= 1;
            return result_type;
        }

        self.gen_binary_operands(left, right, work_type);

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
                emit!(self, "    {} al", setcc);
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
                emit!(self, "    {} eax, ecx", instr);
            }
        }

        self.expr_depth -= 1;
        result_type
    }

    /// Branch to `target` on the truth of `cond`.
    ///
    /// A comparison already sets exactly the flags a conditional jump reads,
    /// so when one is used *as* a condition -- which is nearly always -- there
    /// is no reason to turn the flags into a -1/0 word only to test that word
    /// against zero. This turns
    ///
    /// ```text
    ///     ucomisd xmm0, xmm1 / setb al / movzx eax, al / neg eax
    ///     test eax, eax / je .Lelse
    /// ```
    ///
    /// into `ucomisd xmm0, xmm1 / jae .Lelse`.
    ///
    /// It has to be requested per site rather than done inside
    /// `gen_binary_expr`, because a comparison is an ordinary Long-valued
    /// expression everywhere else: `X = (A > B)` stores it, `PRINT (A > B)`
    /// prints it, and `A > B AND C > D` needs both halves as -1/0 words,
    /// since BASIC's AND is bitwise.
    ///
    /// Anything that is not a comparison falls back to evaluating the
    /// condition as a value and testing it, exactly as before.
    fn gen_condition(&mut self, cond: &Expr, target: &str, jump_if_true: bool) {
        // Complement rather than negate the mnemonic by hand: jae is exactly
        // not-jb, jle is not-jg, and so on, including for the unordered case
        // that ucomisd signals through CF -- so inverting the operator and
        // inverting the branch agree on NaN, as they must to preserve what the
        // setcc form did.
        let effective = |op: BinaryOp| -> BinaryOp {
            if jump_if_true {
                op
            } else {
                match op {
                    BinaryOp::Eq => BinaryOp::Ne,
                    BinaryOp::Ne => BinaryOp::Eq,
                    BinaryOp::Lt => BinaryOp::Ge,
                    BinaryOp::Ge => BinaryOp::Lt,
                    BinaryOp::Gt => BinaryOp::Le,
                    BinaryOp::Le => BinaryOp::Gt,
                    other => other,
                }
            }
        };

        // Integers and memcmp results read signed; ucomis* reports through CF
        // and ZF, so a float comparison reads unsigned.
        let jcc = |op: BinaryOp, signed: bool| -> &'static str {
            match (op, signed) {
                (BinaryOp::Eq, _) => "je",
                (BinaryOp::Ne, _) => "jne",
                (BinaryOp::Lt, true) => "jl",
                (BinaryOp::Lt, false) => "jb",
                (BinaryOp::Gt, true) => "jg",
                (BinaryOp::Gt, false) => "ja",
                (BinaryOp::Le, true) => "jle",
                (BinaryOp::Le, false) => "jbe",
                (BinaryOp::Ge, true) => "jge",
                (BinaryOp::Ge, false) => "jae",
                _ => unreachable!("guarded by is_comparison"),
            }
        };

        if let Expr::Binary { op, left, right } = cond {
            if Self::is_comparison(*op) {
                let left_type = self.expr_type(left);
                let right_type = self.expr_type(right);

                if left_type == DataType::String && right_type == DataType::String {
                    self.gen_string_compare(left, right);
                    emit!(self, "    {} {}", jcc(effective(*op), true), target);
                    return;
                }

                if left_type != DataType::String && right_type != DataType::String {
                    let work_type = self.promote_types(left_type, right_type, BinaryOp::Add);
                    let signed = work_type.is_integer();

                    // Same constant-operand shortcut the value form takes.
                    if signed {
                        if let Some(n) = self.const_i32(right) {
                            let ty = self.gen_expr(left);
                            self.gen_coercion(ty, work_type);
                            emit!(self, "    cmp eax, {}", n);
                            emit!(self, "    {} {}", jcc(effective(*op), true), target);
                            return;
                        }
                    } else if work_type == DataType::Double {
                        if let Some(value) = self.const_double(right) {
                            self.gen_expr_to_double(left);
                            let operand = self.f64_operand(value);
                            emit!(self, "    ucomisd xmm0, {}", operand);
                            emit!(self, "    {} {}", jcc(effective(*op), false), target);
                            return;
                        }
                    }

                    self.gen_binary_operands(left, right, work_type);
                    self.emit_typed(
                        work_type,
                        "    cmp eax, ecx",
                        "    ucomiss xmm0, xmm1",
                        "    ucomisd xmm0, xmm1",
                    );
                    emit!(self, "    {} {}", jcc(effective(*op), signed), target);
                    return;
                }
            }
        }

        // Not a comparison: evaluate it as a value and test that.
        let cond_type = self.gen_expr(cond);
        if cond_type.is_integer() {
            self.emit("    test eax, eax");
        } else {
            self.emit("    xorpd xmm1, xmm1");
            self.emit("    ucomisd xmm0, xmm1");
        }
        emit!(
            self,
            "    {} {}",
            if jump_if_true { "jne" } else { "je" },
            target
        );
    }

    /// Compare two strings, leaving memcmp-style flags set.
    ///
    /// `_rt_strcmp` returns a negative, zero or positive int in eax like
    /// memcmp, and the `test` at the end sets the flags a *signed* condition
    /// reads -- so both the value form and the branch form below can just pick
    /// a mnemonic.
    fn gen_string_compare(&mut self, left: &Expr, right: &Expr) {
        // Evaluate left string (ptr in rax, len in rdx)
        self.gen_expr(left);
        emit!(self, "    sub rsp, {}", STACK_TEMP_SPACE);
        self.emit("    mov QWORD PTR [rsp], rax"); // left ptr
        self.emit("    mov QWORD PTR [rsp + 8], rdx"); // left len

        // Evaluate right string (ptr in rax, len in rdx)
        self.gen_expr(right);
        self.emit("    mov r8, rax"); // right ptr
        self.emit("    mov r9, rdx"); // right len
        self.emit("    mov rax, QWORD PTR [rsp]"); // left ptr
        self.emit("    mov rdx, QWORD PTR [rsp + 8]"); // left len
        emit!(self, "    add rsp, {}", STACK_TEMP_SPACE);
        self.emit_arg_reg(0, "rax");
        self.emit_arg_reg(1, "rdx");
        self.emit_arg_reg(2, "r8");
        self.emit_arg_reg(3, "r9");
        self.emit("    call _rt_strcmp");
        self.emit("    test eax, eax");
    }

    /// Evaluate both operands of a numeric binary operator into the registers
    /// the operator instruction expects: left in eax/xmm0, right in ecx/xmm1.
    ///
    /// The left operand is parked on the stack while the right is evaluated,
    /// because evaluating the right can call a function and clobber every
    /// scratch register. Sixteen bytes rather than eight so that such a call
    /// still finds rsp aligned.
    fn gen_binary_operands(&mut self, left: &Expr, right: &Expr, work_type: DataType) {
        let left_type = self.gen_expr(left);
        self.gen_coercion(left_type, work_type);

        emit!(self, "    sub rsp, {}", STACK_TEMP_SPACE);
        if work_type.is_integer() {
            self.emit("    mov QWORD PTR [rsp], rax");
        } else if work_type == DataType::Single {
            self.emit("    movss DWORD PTR [rsp], xmm0");
        } else {
            self.emit("    movsd QWORD PTR [rsp], xmm0");
        }

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
        emit!(self, "    add rsp, {}", STACK_TEMP_SPACE);
    }

    /// Apply `op` with a compile-time constant on the right, or report that it
    /// could not be done and leave nothing emitted.
    ///
    /// The general path spills the left operand to the stack while the right
    /// one is evaluated, because evaluating the right operand can call a
    /// function and clobber every scratch register. A constant cannot: it has
    /// no side effects, needs no registers, and every instruction here takes
    /// it as an immediate or straight out of the constant pool. That removes
    /// four instructions and a store-to-load round trip from what is the
    /// commonest shape in BASIC arithmetic.
    ///
    /// `Single` is left to the general path -- it would want a pool of its own
    /// and there is very little SINGLE arithmetic to reward one.
    fn gen_binary_const_rhs(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        work_type: DataType,
    ) -> bool {
        // (signed, unsigned): integers compare signed, but ucomisd reports
        // through CF and ZF, so a float comparison reads as unsigned.
        let setcc = |op: BinaryOp, signed: bool| -> &'static str {
            match (op, signed) {
                (BinaryOp::Eq, _) => "sete",
                (BinaryOp::Ne, _) => "setne",
                (BinaryOp::Lt, true) => "setl",
                (BinaryOp::Lt, false) => "setb",
                (BinaryOp::Gt, true) => "setg",
                (BinaryOp::Gt, false) => "seta",
                (BinaryOp::Le, true) => "setle",
                (BinaryOp::Le, false) => "setbe",
                (BinaryOp::Ge, true) => "setge",
                (BinaryOp::Ge, false) => "setae",
                _ => unreachable!("guarded by is_comparison"),
            }
        };

        match work_type {
            DataType::Integer | DataType::Long => {
                let Some(n) = self.const_i32(right) else {
                    return false;
                };
                // Div and Pow always promote to Double, so they never reach
                // the integer arm; anything else unexpected declines.
                let arith = match op {
                    BinaryOp::Add => Some("add"),
                    BinaryOp::Sub => Some("sub"),
                    BinaryOp::Mul => Some("imul"),
                    BinaryOp::And => Some("and"),
                    BinaryOp::Or => Some("or"),
                    BinaryOp::Xor => Some("xor"),
                    _ => None,
                };
                if arith.is_none()
                    && !Self::is_comparison(op)
                    && !matches!(op, BinaryOp::IntDiv | BinaryOp::Mod)
                {
                    return false;
                }

                let left_type = self.gen_expr(left);
                self.gen_coercion(left_type, work_type);

                if let Some(instr) = arith {
                    emit!(self, "    {} eax, {}", instr, n);
                } else if Self::is_comparison(op) {
                    emit!(self, "    cmp eax, {}", n);
                    emit!(self, "    {} al", setcc(op, true));
                    self.emit("    movzx eax, al");
                    self.emit("    neg eax"); // BASIC true is -1
                } else {
                    // idiv has no immediate form, so the divisor still has to
                    // reach ecx -- but it gets there without the spill, and
                    // the existing checks apply to it unchanged.
                    emit!(self, "    mov ecx, {}", n);
                    self.emit_integer_divide_checks();
                    self.emit("    cdq");
                    self.emit("    idiv ecx");
                    if op == BinaryOp::Mod {
                        self.emit("    mov eax, edx");
                    }
                }
                true
            }

            DataType::Double => {
                let Some(value) = self.const_double(right) else {
                    return false;
                };
                // IntDiv and Mod promote to Long, so they are handled above
                // and never arrive here; decline rather than assume it.
                if matches!(op, BinaryOp::IntDiv | BinaryOp::Mod) {
                    return false;
                }
                self.gen_expr_to_double(left);
                let operand = self.f64_operand(value);

                match op {
                    BinaryOp::Add => emit!(self, "    addsd xmm0, {}", operand),
                    BinaryOp::Sub => emit!(self, "    subsd xmm0, {}", operand),
                    BinaryOp::Mul => emit!(self, "    mulsd xmm0, {}", operand),
                    BinaryOp::Div => {
                        // The divisor is known here, so the check is decided
                        // here too rather than being re-tested at run time.
                        // `value == 0.0` is true of -0.0 as well, which is
                        // exactly the set the runtime test catches.
                        if value == 0.0 {
                            self.emit_check("jmp", RtError::DivideByZero);
                        }
                        emit!(self, "    divsd xmm0, {}", operand);
                    }
                    BinaryOp::Pow => {
                        emit!(self, "    movsd xmm1, {}", operand);
                        self.emit_call_libc("pow");
                    }
                    BinaryOp::And | BinaryOp::Or | BinaryOp::Xor => {
                        // Truncation to integer is left to the same conversion
                        // the general path uses, so an out-of-range constant
                        // behaves identically to one held in a register.
                        emit!(self, "    movsd xmm1, {}", operand);
                        self.emit_cvt_float_to_int(work_type);
                        let instr = match op {
                            BinaryOp::And => "and",
                            BinaryOp::Or => "or",
                            _ => "xor",
                        };
                        emit!(self, "    {} eax, ecx", instr);
                    }
                    _ => {
                        emit!(self, "    ucomisd xmm0, {}", operand);
                        emit!(self, "    {} al", setcc(op, false));
                        self.emit("    movzx eax, al");
                        self.emit("    neg eax");
                    }
                }
                true
            }

            _ => false,
        }
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
        self.emit_file_num_check();
        self.stack_offset -= 8;
        let loc = Loc::Frame(self.stack_offset);
        emit!(self, "    mov {}, eax", loc.at("DWORD PTR", 0));
        FileNum::Slot(loc)
    }

    /// Bytes one of the MK*$ conversions writes, which is the width of the
    /// BASIC type it names.
    fn mk_width(name: &str) -> i64 {
        match name {
            "MKI$" => 2,
            "MKD$" => 8,
            // MKL$ (LONG) and MKS$ (SINGLE) are both 4 bytes.
            _ => 4,
        }
    }

    /// Evaluate a numeric expression into a parked slot, the way a file number
    /// is, but without the 1-to-15 range check.
    ///
    /// Record numbers, record lengths and FIELD widths all need the same
    /// treatment -- a Long parked somewhere the argument setup can reach after
    /// other expressions have clobbered `rax` -- but none of them is a file
    /// number, so none may be checked against the handle table's bounds.
    fn gen_file_num_like(&mut self, e: &Expr) -> FileNum {
        if let Expr::Literal(Literal::Integer(n)) = e {
            return FileNum::Imm(*n);
        }
        let ty = self.gen_expr(e);
        self.gen_coercion(ty, DataType::Long);
        self.stack_offset -= 8;
        let loc = Loc::Frame(self.stack_offset);
        emit!(self, "    mov {}, eax", loc.at("DWORD PTR", 0));
        FileNum::Slot(loc)
    }

    /// Bind a string variable to the (pointer, length) pair in `rax`/`rdx`
    /// without copying it.
    ///
    /// This is what makes a FIELD variable alias the record buffer instead of
    /// holding a snapshot of it: an ordinary assignment would call `_rt_strdup`
    /// first, and the variable would then stop tracking what `GET` reads.
    fn gen_bind_alias(&mut self, target: &LValue) {
        if let Some(indices) = &target.indices {
            let indices = indices.clone();
            emit!(self, "    sub rsp, {}", STACK_TEMP_SPACE);
            self.emit("    mov QWORD PTR [rsp], rax");
            self.emit("    mov QWORD PTR [rsp + 8], rdx");
            self.gen_array_addr(&target.name, &indices);
            self.emit("    mov rcx, rax");
            self.emit("    mov rax, QWORD PTR [rsp]");
            self.emit("    mov rdx, QWORD PTR [rsp + 8]");
            emit!(self, "    add rsp, {}", STACK_TEMP_SPACE);
            self.emit("    mov QWORD PTR [rcx], rax");
            self.emit("    mov QWORD PTR [rcx + 8], rdx");
            return;
        }
        let loc = self.get_var_loc(&target.name);
        emit!(self, "    mov {}, rax", loc.q(0));
        emit!(self, "    mov {}, rdx", loc.q(1));
    }

    /// Load a string lvalue's current (pointer, length) into `rax`/`rdx`.
    ///
    /// Used by LSET/RSET, which need the field's address rather than its value:
    /// the bytes are overwritten where they already are.
    fn gen_load_lvalue_string(&mut self, target: &LValue) {
        if let Some(indices) = &target.indices {
            let indices = indices.clone();
            self.gen_array_addr(&target.name, &indices);
            self.emit("    mov rdx, QWORD PTR [rax + 8]");
            self.emit("    mov rax, QWORD PTR [rax]");
            return;
        }
        let loc = self.get_var_loc(&target.name);
        emit!(self, "    mov rax, {}", loc.q(0));
        emit!(self, "    mov rdx, {}", loc.q(1));
    }

    /// Resolve where a PRINT writes: a file number, or the console.
    fn gen_sink(&mut self, file_num: Option<&Expr>) -> FileNum {
        match file_num {
            Some(e) => self.gen_file_num(e),
            None => FileNum::Imm(CONSOLE),
        }
    }

    /// Write an INPUT prompt, which always goes to the console.
    fn gen_console_prompt(&mut self, prompt: &str) {
        let idx = self.add_string_literal(prompt);
        self.emit_arg_file_num(0, &FileNum::Imm(CONSOLE));
        self.emit_arg_lea(1, &format!("[rip + _str_{}]", idx));
        self.emit_arg_imm(2, prompt.len() as i64);
        self.emit("    call _rt_file_print_string");
    }

    /// Check the file number in `eax` against the handle table's slots.
    ///
    /// The table has a slot per legal file number and no more, so an unchecked
    /// index reached outside it -- far enough, for a large enough number, to
    /// take the process down with it.
    fn emit_file_num_check(&mut self) {
        self.emit("    cmp eax, 1");
        self.emit_check("jl", RtError::BadFileNum);
        emit!(self, "    cmp eax, {}", crate::sema::MAX_FILE_NUM);
        self.emit_check("jg", RtError::BadFileNum);
    }

    /// Place a previously evaluated file number in an argument register.
    fn emit_arg_file_num(&mut self, idx: usize, fnum: &FileNum) {
        match fnum {
            FileNum::Imm(n) => self.emit_arg_imm(idx, *n),
            FileNum::Slot(loc) => {
                let reg = Self::arg_reg(idx);
                emit!(self, "    movsxd {}, {}", reg, loc.at("DWORD PTR", 0));
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
        emit!(self, "    sub rsp, {}", SLOTS);

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
            // `mov eax` zeroes the upper half, so this is the same positive
            // value in rax, in five bytes rather than seven.
            None => self.emit("    mov eax, 0x7FFFFFFF"),
        }
        self.emit("    mov QWORD PTR [rsp + 24], rax");

        self.gen_expr(value);
        self.emit("    mov QWORD PTR [rsp + 32], rax"); // source pointer
        self.emit("    mov QWORD PTR [rsp + 40], rdx"); // source length

        // Load the register arguments, then the stack ones on Win64.
        let regs = PlatformAbi::INT_ARG_REGS;
        for (i, off) in [0, 8, 16, 24].iter().enumerate() {
            if i < regs.len() {
                emit!(self, "    mov {}, QWORD PTR [rsp + {}]", regs[i], off);
            }
        }
        if regs.len() >= 6 {
            emit!(self, "    mov {}, QWORD PTR [rsp + 32]", regs[4]);
            emit!(self, "    mov {}, QWORD PTR [rsp + 40]", regs[5]);
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

        emit!(self, "    add rsp, {}", SLOTS);
    }

    /// Exchange two values, which sema has checked are the same type class.
    ///
    /// Both are read before either is written, so `SWAP A(I), A(J)` is correct
    /// even when the subscripts alias.
    fn gen_swap(&mut self, a: &LValue, b: &LValue) {
        // A field's type comes from its declaration, not from the base
        // variable's name, which carries no suffix for a record.
        let is_string = self.expr_type(&Self::lvalue_expr(a)) == DataType::String;

        // Read A into a temp.
        self.gen_read_lvalue(a);
        emit!(self, "    sub rsp, {}", STACK_TEMP_SPACE);
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
        emit!(self, "    add rsp, {}", STACK_TEMP_SPACE);
        self.gen_store_lvalue(b);
    }

    /// Load an assignment target's current value, in the same registers
    /// `gen_expr` would leave it in.
    fn gen_read_lvalue(&mut self, target: &LValue) {
        let expr = Self::lvalue_expr(target);
        let ty = self.gen_expr(&expr);
        if ty != DataType::String {
            // gen_store_lvalue expects a Double, as the runtime readers produce.
            self.gen_coercion(ty, DataType::Double);
        }
    }

    /// The expression form of an assignment target.
    ///
    /// Reading a target is exactly evaluating this, so SWAP and the compound
    /// readers do not need a second implementation of field and subscript
    /// resolution -- and, before this, simply dropped the field path.
    fn lvalue_expr(target: &LValue) -> Expr {
        let mut expr = match &target.indices {
            Some(indices) => Expr::ArrayAccess {
                name: target.name.clone(),
                indices: indices.clone(),
            },
            None => Expr::Variable(target.name.clone()),
        };
        for field in &target.fields {
            expr = Expr::Field {
                base: Box::new(expr),
                field: field.clone(),
            };
        }
        expr
    }

    /// Copy the string in `rax`/`rdx` onto the heap.
    ///
    /// String assignment copies, so that mutating one variable is not visible
    /// through another, and so that a string constant's shared `.data` literal
    /// can never be written through.
    /// Whether evaluating `expr` leaves a string in memory nothing else holds.
    ///
    /// Assignment copies a string because most expressions hand back a pointer
    /// into something that outlives the statement: a variable's own buffer,
    /// a slice of one -- `LEFT$`, `MID$`, `LTRIM$` all just narrow (ptr, len)
    /// -- or a `FIELD` window into a file's record buffer. Without the copy
    /// two variables would share bytes and `MID$(a$, ...) =` would edit both.
    ///
    /// These, though, have already allocated. Copying one is a second malloc
    /// and a second memcpy of bytes no one else can reach, and it leaks the
    /// first block, since nothing in this runtime frees. `S$ = S$ + "ab"` paid
    /// for two of everything.
    ///
    /// Kept as a list of names rather than a property of the call, because it
    /// is a fact about each helper's implementation: `_rt_strcat`, `_rt_space`,
    /// `_rt_string_n` and `_rt_case_convert` call malloc, and `_rt_str`,
    /// `_rt_chr`, `_rt_hex`, `_rt_oct` and `_rt_mk` end in `_rt_strdup`. A
    /// helper that stops allocating has to come off this list.
    fn expr_owns_its_string(&self, expr: &Expr) -> bool {
        match expr {
            // Concatenation. Guarded on the type, since `+` is also numeric.
            Expr::Binary {
                op: BinaryOp::Add, ..
            } => self.expr_type(expr) == DataType::String,
            Expr::FnCall { name, .. } => {
                let upper = name.to_uppercase();
                // A user procedure may not shadow one of these, but checking
                // costs nothing and the answer would be wrong if it could.
                !self.symbols.procs.contains_key(&upper)
                    && matches!(
                        upper.as_str(),
                        "STR$"
                            | "CHR$"
                            | "HEX$"
                            | "OCT$"
                            | "SPACE$"
                            | "STRING$"
                            | "UCASE$"
                            | "LCASE$"
                            | "MKI$"
                            | "MKL$"
                            | "MKS$"
                            | "MKD$"
                    )
            }
            _ => false,
        }
    }

    /// Copy a string unless `value` has already allocated one of its own.
    fn emit_string_copy_of(&mut self, value: &Expr) {
        if !self.expr_owns_its_string(value) {
            self.emit_string_copy();
        }
    }

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
            let Some(info) = self.symbols.records.get(normalized(rec)) else {
                return DataType::Double;
            };
            let Some(f) = info.field(field) else {
                return DataType::Double;
            };
            ty = f.ty.clone();
        }
        DataType::from_type_ref(&ty)
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
    /// Emit the address of a record lvalue into `rax`.
    ///
    /// Returns false when the expression does not denote one, leaving no code
    /// emitted, so the caller can fall back to its ordinary value path.
    fn gen_record_addr(&mut self, expr: &Expr) -> bool {
        match expr {
            Expr::Variable(name) => {
                let Some(ty) = self.typed_var(name) else {
                    return false;
                };
                if !matches!(ty, TypeRef::Record(_)) {
                    return false;
                }
                let loc = self.get_record_loc(name, &ty);
                match &loc {
                    Loc::Global(sym) => emit!(self, "    lea rax, [rip + {}]", sym),
                    Loc::Frame(off) => emit!(self, "    lea rax, [rbp + {}]", off),
                }
                true
            }

            // An element of an array of records: the address is only known at
            // run time, so it is computed rather than taken from a `Loc`.
            Expr::ArrayAccess { name, indices }
            | Expr::FnCall {
                name,
                args: indices,
            } => {
                match self.array_elem_type(name) {
                    Some(TypeRef::Record(_)) => {}
                    _ => return false,
                }
                let indices = indices.clone();
                self.gen_array_addr(name, &indices);
                true
            }

            // A nested record reached through a field path, on either base.
            Expr::Field { .. } => {
                if let Some((name, indices, fields)) = Self::flatten_indexed_field_path(expr) {
                    let Some(base) = self.array_elem_type(&name) else {
                        return false;
                    };
                    let Some((offset, ty)) = self.field_byte_offset(&base, &fields) else {
                        return false;
                    };
                    if !matches!(ty, TypeRef::Record(_)) {
                        return false;
                    }
                    self.gen_array_addr(&name, &indices);
                    if offset != 0 {
                        emit!(self, "    add rax, {}", offset);
                    }
                    return true;
                }

                let Some((name, fields)) = Self::flatten_field_path(expr) else {
                    return false;
                };
                let Some((loc, ty)) = self.resolve_field_path(&name, &fields) else {
                    return false;
                };
                if !matches!(ty, TypeRef::Record(_)) {
                    return false;
                }
                match &loc {
                    Loc::Global(sym) => emit!(self, "    lea rax, [rip + {}]", sym),
                    Loc::Frame(off) => emit!(self, "    lea rax, [rbp + {}]", off),
                }
                true
            }

            _ => false,
        }
    }

    /// Byte offset of a field path within `base`, and the type it arrives at.
    fn field_byte_offset(&self, base: &TypeRef, fields: &[String]) -> Option<(i32, TypeRef)> {
        let mut ty = base.clone();
        let mut offset = 0i32;
        for field in fields {
            let TypeRef::Record(rec) = &ty else {
                return None;
            };
            let f = self.symbols.records.get(normalized(rec))?.field(field)?;
            offset += f.word * 8;
            ty = f.ty.clone();
        }
        Some((offset, ty))
    }

    fn gen_array_field(&mut self, target: &LValue, value: Option<&Expr>) -> DataType {
        let indices = target.indices.clone().unwrap_or_default();
        let Some(ty) = self.array_elem_type(&target.name) else {
            unreachable!("sema checked this is an array of records")
        };

        let Some((byte_offset, ty)) = self.field_byte_offset(&ty, &target.fields) else {
            unreachable!("sema checked the field path")
        };

        match value {
            None => {
                self.gen_array_addr(&target.name, &indices);
                let loc = Loc::Frame(0); // placeholder, replaced below
                let _ = loc;
                emit!(self, "    add rax, {}", byte_offset);
                self.gen_load_indirect("rax", &ty)
            }
            Some(v) => {
                // Evaluate the value first, then the address, since computing
                // the address clobbers the value registers.
                let vt = self.gen_expr(v);
                if DataType::from_type_ref(&ty) == DataType::String {
                    self.emit_string_copy_of(v);
                    emit!(self, "    sub rsp, {}", STACK_TEMP_SPACE);
                    self.emit("    mov QWORD PTR [rsp], rax");
                    self.emit("    mov QWORD PTR [rsp + 8], rdx");
                } else {
                    self.gen_coercion(vt, DataType::Double);
                    emit!(self, "    sub rsp, {}", STACK_TEMP_SPACE);
                    self.emit("    movsd QWORD PTR [rsp], xmm0");
                }

                self.gen_array_addr(&target.name, &indices);
                emit!(self, "    add rax, {}", byte_offset);
                self.emit("    mov rcx, rax");
                self.emit_store_parked(&ty);
                DataType::Double
            }
        }
    }

    /// Store a value parked on the stack through the address in `rcx`.
    ///
    /// The pairing with the `sub rsp` that parked it is the caller's, so that
    /// the value can be produced before the address that would clobber it.
    fn emit_store_parked(&mut self, ty: &TypeRef) {
        if DataType::from_type_ref(ty) == DataType::String {
            self.emit("    mov rax, QWORD PTR [rsp]");
            self.emit("    mov rdx, QWORD PTR [rsp + 8]");
            emit!(self, "    add rsp, {}", STACK_TEMP_SPACE);
            self.emit("    mov QWORD PTR [rcx], rax");
            self.emit("    mov QWORD PTR [rcx + 8], rdx");
            return;
        }
        self.emit("    movsd xmm0, QWORD PTR [rsp]");
        emit!(self, "    add rsp, {}", STACK_TEMP_SPACE);
        self.gen_coercion(DataType::Double, DataType::from_type_ref(ty));
        match ty {
            TypeRef::Integer => self.emit("    mov WORD PTR [rcx], ax"),
            TypeRef::Long => self.emit("    mov DWORD PTR [rcx], eax"),
            TypeRef::Single => self.emit("    movss DWORD PTR [rcx], xmm0"),
            _ => self.emit("    movsd QWORD PTR [rcx], xmm0"),
        }
    }

    /// Load a scalar of the given declared type from the address in `reg`.
    fn gen_load_indirect(&mut self, reg: &str, ty: &TypeRef) -> DataType {
        match ty {
            TypeRef::Integer => {
                emit!(self, "    movsx eax, WORD PTR [{}]", reg);
                DataType::Integer
            }
            TypeRef::Long => {
                emit!(self, "    mov eax, DWORD PTR [{}]", reg);
                DataType::Long
            }
            TypeRef::Single => {
                emit!(self, "    movss xmm0, DWORD PTR [{}]", reg);
                DataType::Single
            }
            TypeRef::Double => {
                emit!(self, "    movsd xmm0, QWORD PTR [{}]", reg);
                DataType::Double
            }
            TypeRef::FixedString(_) => {
                emit!(self, "    mov rcx, {}", reg);
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
            let info = self.symbols.records.get(normalized(rec))?;
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
                emit!(self, "    movsx eax, {}", loc.at("WORD PTR", 0));
                DataType::Integer
            }
            TypeRef::Long => {
                emit!(self, "    mov eax, {}", loc.at("DWORD PTR", 0));
                DataType::Long
            }
            TypeRef::Single => {
                emit!(self, "    movss xmm0, {}", loc.at("DWORD PTR", 0));
                DataType::Single
            }
            TypeRef::Double => {
                emit!(self, "    movsd xmm0, {}", loc.q(0));
                DataType::Double
            }
            TypeRef::FixedString(_) => {
                emit!(self, "    mov rax, {}", loc.q(0));
                emit!(self, "    mov rdx, {}", loc.q(1));
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
                emit!(self, "    mov {}, ax", loc.at("WORD PTR", 0));
            }
            TypeRef::Long => {
                self.gen_coercion(value_type, DataType::Long);
                emit!(self, "    mov {}, eax", loc.at("DWORD PTR", 0));
            }
            TypeRef::Single => {
                self.gen_coercion(value_type, DataType::Single);
                emit!(self, "    movss {}, xmm0", loc.at("DWORD PTR", 0));
            }
            TypeRef::Double => {
                self.gen_coercion(value_type, DataType::Double);
                emit!(self, "    movsd {}, xmm0", loc.q(0));
            }
            TypeRef::FixedString(width) => {
                // A fixed-length string holds exactly its declared width: a
                // short value is space-padded, a long one truncated. Storing
                // the source verbatim -- as this did -- made the width mean
                // nothing, so `STRING * 5` held whatever it was given and a
                // record laid over a random-access file stopped lining up.
                self.emit_arg_reg(0, "rax");
                self.emit_arg_reg(1, "rdx");
                self.emit_arg_imm(2, *width as i64);
                self.emit("    call _rt_fixed");
                emit!(self, "    mov {}, rax", loc.q(0));
                emit!(self, "    mov {}, rdx", loc.q(1));
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
            // `arr(i).f`: the address is only known at run time, so the value
            // is parked across the address calculation that clobbers it.
            if let Some(indices) = target.indices.clone() {
                if let Some(base) = self.array_elem_type(&target.name) {
                    if let Some((offset, ty)) = self.field_byte_offset(&base, &target.fields) {
                        emit!(self, "    sub rsp, {}", STACK_TEMP_SPACE);
                        if DataType::from_type_ref(&ty) == DataType::String {
                            self.emit("    mov QWORD PTR [rsp], rax");
                            self.emit("    mov QWORD PTR [rsp + 8], rdx");
                        } else {
                            self.emit("    movsd QWORD PTR [rsp], xmm0");
                        }
                        self.gen_array_addr(&target.name, &indices);
                        emit!(self, "    add rax, {}", offset);
                        self.emit("    mov rcx, rax");
                        self.emit_store_parked(&ty);
                        return;
                    }
                }
            }
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
            // A variable declared with `AS` lives in typed storage and carries
            // its own width; gen_store_typed is what narrows for it, and is the
            // same helper an ordinary assignment to it uses.
            if let Some((loc, ty)) = self.typed_storage(&target.name) {
                self.gen_store_typed(&loc, &ty, DataType::Double);
                return;
            }

            let loc = self.get_var_loc(&target.name);
            if is_string {
                emit!(self, "    mov {}, rax", loc.q(0));
                emit!(self, "    mov {}, rdx", loc.q(1));
                return;
            }
            // Narrow to the variable's declared type, exactly as the array
            // element path below does and for the same reason.
            //
            // This used to store the incoming Double whole, so an INTEGER slot
            // received eight bytes of a double's bit pattern and every later
            // read -- `movsx eax, WORD PTR` -- took its low sixteen bits, which
            // for any ordinary value are zero. READ, INPUT and SWAP all store
            // through here, so `READ A%`, `INPUT A%` and `SWAP A%, B%` each
            // yielded 0 while plain `A% = 7`, which stores elsewhere, was fine.
            let ty = self.expr_type(&Expr::Variable(target.name.clone()));
            self.gen_coercion(DataType::Double, ty);
            match ty {
                DataType::Integer => emit!(self, "    mov {}, ax", loc.at("WORD PTR", 0)),
                DataType::Long => emit!(self, "    mov {}, eax", loc.at("DWORD PTR", 0)),
                DataType::Single => emit!(self, "    movss {}, xmm0", loc.at("DWORD PTR", 0)),
                DataType::Double => emit!(self, "    movsd {}, xmm0", loc.q(0)),
                DataType::String => unreachable!("handled above"),
            }
            return;
        };

        // Park the value, compute the element address, then store.
        emit!(self, "    sub rsp, {}", STACK_TEMP_SPACE);
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
            emit!(self, "    add rsp, {}", STACK_TEMP_SPACE);
            self.emit("    mov QWORD PTR [rcx], rax");
            self.emit("    mov QWORD PTR [rcx + 8], rdx");
        } else {
            self.emit("    movsd xmm0, QWORD PTR [rsp]");
            emit!(self, "    add rsp, {}", STACK_TEMP_SPACE);
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
                        self.gen_expr_to_double(value);
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
        self.gen_console_prompt(text);
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
                self.gen_expr_to_double(e);
                emit!(self, "    movsd xmm1, {}", sel);
                self.emit("    ucomisd xmm1, xmm0");
                emit!(self, "    je {}", body_label);
            }
            CaseClause::Range(lo, hi) => {
                // Inclusive at both ends. The low bound is tested first, and a
                // failure skips the high test.
                let skip = self.new_label("caseskip");
                self.gen_expr_to_double(lo);
                emit!(self, "    movsd xmm1, {}", sel);
                self.emit("    ucomisd xmm1, xmm0");
                emit!(self, "    jb {}", skip);
                self.gen_expr_to_double(hi);
                emit!(self, "    movsd xmm1, {}", sel);
                self.emit("    ucomisd xmm1, xmm0");
                emit!(self, "    jbe {}", body_label);
                self.emit_label(&skip);
            }
            CaseClause::Compare(op, e) => {
                self.gen_expr_to_double(e);
                emit!(self, "    movsd xmm1, {}", sel);
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
                emit!(self, "    {} {}", cc, body_label);
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
                emit!(self, "    je {}", body_label);
            }
            CaseClause::Range(lo, hi) => {
                let skip = self.new_label("caseskip");
                compare(self, lo);
                emit!(self, "    jl {}", skip);
                compare(self, hi);
                emit!(self, "    jle {}", body_label);
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
                emit!(self, "    {} {}", cc, body_label);
            }
        }
    }

    /// Emit one WRITE value: strings are quoted, numbers printed as usual.
    /// `WRITE` quotes a string; a number is written the same as by `PRINT`.
    fn gen_write_expr(&mut self, expr: &Expr, sink: &FileNum) {
        if self.expr_type(expr) == DataType::String {
            self.emit_arg_file_num(0, sink);
            self.emit_arg_imm(1, ASCII_QUOTE);
            self.emit("    call _rt_file_print_char");
            self.gen_print_expr(expr, sink);
            self.emit_arg_file_num(0, sink);
            self.emit_arg_imm(1, ASCII_QUOTE);
            self.emit("    call _rt_file_print_char");
        } else {
            self.gen_print_expr(expr, sink);
        }
    }

    /// Write one PRINT item to `sink`, which is the console when it is 0.
    ///
    /// There is one of these rather than one per destination: the file copy
    /// this replaced had missed both the TAB/SPC case and the SINGLE one, so
    /// `PRINT #1, A!` wrote digits a SINGLE does not carry and `PRINT #1,
    /// TAB(10)` positioned the console.
    fn gen_print_expr(&mut self, expr: &Expr, sink: &FileNum) {
        // TAB() and SPC() position the cursor rather than producing a value,
        // so they are emitted for their effect and nothing is printed after.
        if let Expr::FnCall { name, args } = expr {
            let upper = name.to_uppercase();
            if upper == "TAB" || upper == "SPC" {
                self.gen_print_position(&upper, &args[0], sink);
                return;
            }
        }

        if self.expr_type(expr) == DataType::String {
            // gen_expr for strings puts ptr in rax, len in rdx. The length has
            // to move first: on Win64 the pointer's argument register is rdx.
            self.gen_expr(expr);
            self.emit_arg_reg(2, "rdx"); // len
            self.emit_arg_reg(1, "rax"); // ptr
            self.emit_arg_file_num(0, sink);
            self.emit("    call _rt_file_print_string");
        } else {
            // A Single is printed via its own helper, which round-trips
            // against 32-bit precision: widening 3.14159! to a double and
            // printing every digit that survives would show 3.141590118408203.
            let expr_type = self.expr_type(expr);
            self.gen_expr_to_double(expr);
            self.emit_arg_file_num(0, sink);
            if expr_type == DataType::Single {
                self.emit("    call _rt_file_print_single");
            } else {
                self.emit("    call _rt_file_print_float");
            }
        }
    }

    /// `TAB(n)` / `SPC(n)`: move the write position within `sink`.
    fn gen_print_position(&mut self, name: &str, arg: &Expr, sink: &FileNum) {
        let t = self.gen_expr(arg);
        self.gen_coercion(t, DataType::Long);
        self.emit("    movsxd rax, eax");
        self.emit_arg_file_num(0, sink);
        self.emit_arg_reg(1, "rax");
        let rt = if name == "TAB" {
            "_rt_file_print_tab"
        } else {
            "_rt_file_print_spc"
        };
        emit!(self, "    call {}", rt);
    }

    fn gen_fn_call(&mut self, name: &str, args: &[Expr]) {
        let upper_name = name.to_uppercase();

        // Table-driven: libc math functions (SIN, COS, TAN, ATN, EXP, LOG)
        if let Some(libc_fn) = LIBC_MATH_FNS.get(upper_name.as_str()) {
            self.gen_expr_to_double(&args[0]);
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
            self.gen_expr_to_double(&args[0]);
            // sqrtsd of a negative operand yields NaN, which then printed as a
            // huge meaningless integer.
            if upper_name == "SQR" {
                self.emit_domain_check("jb");
            }
            emit!(self, "    {}", instr);
            return;
        }

        // Table-driven: evaluate one argument, coerce it, call the helper.
        if let Some(builtin) = RT_BUILTINS.get(upper_name.as_str()) {
            match builtin {
                Builtin::Call0(sym) => emit!(self, "    call {}", sym),
                Builtin::CallStr(sym) => {
                    // gen_expr leaves a string in rax/rdx. Loading the length
                    // first keeps Win64, where the pointer's register is rdx,
                    // from overwriting it.
                    self.gen_expr(&args[0]);
                    self.emit_arg_reg(1, "rdx");
                    self.emit_arg_reg(0, "rax");
                    emit!(self, "    call {}", sym);
                }
                Builtin::CallLong(sym) => {
                    let arg_type = self.gen_expr(&args[0]);
                    self.gen_coercion(arg_type, DataType::Long);
                    self.emit("    movsxd rax, eax");
                    self.emit_arg_reg(0, "rax");
                    emit!(self, "    call {}", sym);
                }
                Builtin::Coerce(ty) => {
                    let arg_type = self.gen_expr(&args[0]);
                    self.gen_coercion(arg_type, *ty);
                }
            }
            return;
        }

        // Complex built-in functions
        match upper_name.as_str() {
            "ABS" => {
                let arg_type = self.expr_type(&args[0]);
                self.gen_expr_to_double(&args[0]);
                // Everything-but-the-sign-bit, from the pool rather than a
                // 64-bit immediate, and via a register because the memory form
                // of andpd wants sixteen aligned bytes. See UnaryOp::Neg.
                let mask = self.f64_operand(f64::from_bits(0x7FFF_FFFF_FFFF_FFFF));
                emit!(self, "    movsd xmm1, {}", mask);
                self.emit("    andpd xmm0, xmm1");
                // ABS preserves its argument's type: narrow back so the value
                // matches what call_return_type promises.
                self.gen_coercion(DataType::Double, Self::abs_result_type(arg_type));
            }
            "SGN" => {
                self.gen_expr_to_double(&args[0]);
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
                    self.gen_expr_to_double(&args[0]);
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
                    emit!(self, "    movsxd {}, eax", arg2);
                } else {
                    emit!(self, "    cvttsd2si {}, xmm0", arg2);
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
                    emit!(self, "    movsxd {}, eax", arg2);
                } else {
                    emit!(self, "    cvttsd2si {}, xmm0", arg2);
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
                        emit!(self, "    movsxd {}, eax", arg3);
                    } else {
                        emit!(self, "    cvttsd2si {}, xmm0", arg3);
                    }
                } else {
                    emit!(self, "    mov {}, -1", arg3); // rest of string
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
                    self.emit("    mov ebx, 1");
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

                // Five arguments: the one call in the compiler that Win64
                // cannot carry in registers alone. The sources are chosen so
                // that assigning them last-first never clobbers one still to
                // be read -- rdx holds the needle length and is also System
                // V's third argument register.
                self.emit_call_with_args("_rt_instr", &["r12", "r13", "rax", "rdx", "rbx"]);

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
            // String builders. These allocate, so the result outlives the call.
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
            // Radix conversions.
            // Array bounds. The descriptor stores each dimension's element
            // count, so UBOUND is that minus one and LBOUND is always 0.
            "LBOUND" | "UBOUND" => {
                let Expr::Variable(arr) = &args[0] else {
                    unreachable!("sema requires an array name here")
                };
                let arr = arr.to_uppercase();
                let rank = self
                    .symbols
                    .lookup_array(&self.sema_scope(), &arr)
                    .expect("sema checked the array exists")
                    .rank as i64;

                // The lower bound does not depend on which dimension is asked
                // for, but an out-of-range dimension is still an error.
                if upper_name == "LBOUND" {
                    if let Some(dim) = args.get(1) {
                        if self.const_dim(dim).is_none() {
                            self.gen_dim_index(dim, rank);
                        }
                    }
                    emit!(self, "    mov eax, {}", self.symbols.option_base);
                    return;
                }

                let loc = self
                    .lookup_array(&arr)
                    .expect("sema checked the array exists")
                    .loc
                    .clone();

                match args.get(1).map(|d| (d, self.const_dim(d))) {
                    // The usual case: a literal or CONST dimension, resolved
                    // to a fixed descriptor slot.
                    None | Some((_, Some(_))) => {
                        let dim = args.get(1).and_then(|d| self.const_dim(d)).unwrap_or(1);
                        emit!(self, "    mov rax, {}", loc.q(dim));
                    }
                    // A computed dimension indexes the descriptor at run time.
                    Some((dim, None)) => {
                        self.gen_dim_index(dim, rank);
                        match &loc {
                            Loc::Global(sym) => emit!(self, "    lea rcx, [rip + {}]", sym),
                            Loc::Frame(off) => emit!(self, "    lea rcx, [rbp + {}]", off),
                        }
                        self.emit("    mov rax, QWORD PTR [rcx + rax*8]");
                    }
                }
                self.emit("    dec rax");
            }
            // File status. All take a file number and return a number.
            "EOF" | "LOF" | "LOC" => {
                let arg_type = self.gen_expr(&args[0]);
                self.gen_coercion(arg_type, DataType::Long);
                self.emit_file_num_check();
                self.emit_arg_reg(0, "rax");
                let rt = match upper_name.as_str() {
                    "EOF" => "_rt_file_eof",
                    "LOC" => "_rt_file_loc",
                    _ => "_rt_file_lof",
                };
                emit!(self, "    call {}", rt);
            }
            // STR$ renders what PRINT would, which means picking the same
            // table PRINT would: a SINGLE carries ~7 digits, and rendering it
            // at full double precision would turn 3.14159! into
            // "3.141590118408203".
            "STR$" => {
                // The *static* type, not what gen_expr reports: loading a
                // SINGLE already widens it to a double, so asking afterwards
                // would never see one.
                let single = self.expr_type(&args[0]) == DataType::Single;
                self.gen_expr_to_double(&args[0]);
                if single {
                    self.emit("    call _rt_str_single");
                } else {
                    self.emit("    call _rt_str");
                }
            }
            // MKI$/MKL$/MKS$/MKD$: a number's bytes, as a string. The width
            // is the type's own, so the value is coerced to it first and the
            // runtime just copies that many bytes out.
            "MKI$" | "MKL$" | "MKS$" | "MKD$" => {
                let want = match upper_name.as_str() {
                    "MKI$" => DataType::Integer,
                    "MKL$" => DataType::Long,
                    "MKS$" => DataType::Single,
                    _ => DataType::Double,
                };
                let arg_type = self.gen_expr(&args[0]);
                self.gen_coercion(arg_type, want);
                match want {
                    // Integers arrive in rax, floats in xmm0; the runtime takes
                    // the bytes in rdi either way.
                    DataType::Integer | DataType::Long => self.emit_arg_reg(0, "rax"),
                    DataType::Single => {
                        self.emit("    movd eax, xmm0");
                        self.emit_arg_reg(0, "rax");
                    }
                    _ => {
                        self.emit("    movq rax, xmm0");
                        self.emit_arg_reg(0, "rax");
                    }
                }
                self.emit_arg_imm(1, Self::mk_width(&upper_name));
                self.emit("    call _rt_mk");
            }
            // CVI/CVL/CVS/CVD: read those bytes back as a number. A string
            // narrower than the type is an error rather than a value, so the
            // line number goes along for the diagnostic.
            "CVI" | "CVL" | "CVS" | "CVD" => {
                self.gen_expr(&args[0]);
                self.emit_arg_reg(0, "rax"); // string ptr
                self.emit_arg_reg(1, "rdx"); // string len
                self.emit_arg_imm(2, self.current_line as i64);
                let rt = match upper_name.as_str() {
                    "CVI" => "_rt_cvi",
                    "CVL" => "_rt_cvl",
                    "CVS" => "_rt_cvs",
                    _ => "_rt_cvd",
                };
                emit!(self, "    call {}", rt);
            }
            // Print positioning. These emit output rather than yielding a
            // value, so they are only meaningful inside PRINT.
            "TAB" | "SPC" => {
                self.gen_print_position(&upper_name, &args[0], &FileNum::Imm(CONSOLE));
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
            emit!(self, "    call _proc_{}", mangled);
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
        emit!(self, "    sub rsp, {}", temp_bytes);

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
            // Any record lvalue qualifies, not just a plain variable: passing
            // `A(1)` used to fall through to the numeric path below and hand
            // the callee a float where it expected a pointer.
            if let Some(Param {
                ty: Some(TypeRef::Record(_)),
                ..
            }) = param_decls.get(i)
            {
                if self.gen_record_addr(arg) {
                    emit!(self, "    mov QWORD PTR [rsp + {}], rax", w * 8);
                    temp_of.push(w * 8);
                    w += 1;
                    continue;
                }
            }
            if *ty == DataType::String {
                self.gen_expr(arg);
                emit!(self, "    mov QWORD PTR [rsp + {}], rax", w * 8);
                emit!(self, "    mov QWORD PTR [rsp + {}], rdx", w * 8 + 8);
                temp_of.push(w * 8);
                w += 2;
            } else {
                // Numeric arguments travel as f64 bit patterns in integer
                // slots; the callee narrows to the declared type.
                self.gen_expr_to_double(arg);
                emit!(self, "    movsd QWORD PTR [rsp + {}], xmm0", w * 8);
                temp_of.push(w * 8);
                w += 1;
            }
        }

        // Phase 2: copy the stack-passed slots into place.
        let stack_bytes = ((stack_slots * 8 + 15) & !15) as i32;
        if stack_slots > 0 {
            emit!(self, "    sub rsp, {}", stack_bytes);
        }
        for (place, off) in places.iter().zip(&temp_of) {
            // r11 is caller-saved and an argument register on neither ABI.
            if let Slot::Stk(i) = place.ptr {
                emit!(self, "    mov r11, QWORD PTR [rsp + {}]", stack_bytes + off);
                emit!(self, "    mov QWORD PTR [rsp + {}], r11", i as i32 * 8);
            }
            if let Some(Slot::Stk(i)) = place.len {
                emit!(
                    self,
                    "    mov r11, QWORD PTR [rsp + {}]",
                    stack_bytes + off + 8
                );
                emit!(self, "    mov QWORD PTR [rsp + {}], r11", i as i32 * 8);
            }
        }

        // Phase 3: load the register slots last, so nothing can clobber them.
        let regs = PlatformAbi::INT_ARG_REGS;
        for (place, off) in places.iter().zip(&temp_of) {
            if let Slot::Reg(i) = place.ptr {
                emit!(
                    self,
                    "    mov {}, QWORD PTR [rsp + {}]",
                    regs[i],
                    stack_bytes + off
                );
            }
            if let Some(Slot::Reg(i)) = place.len {
                emit!(
                    self,
                    "    mov {}, QWORD PTR [rsp + {}]",
                    regs[i],
                    stack_bytes + off + 8
                );
            }
        }

        emit!(self, "    call _proc_{}", mangled);
        emit!(self, "    add rsp, {}", stack_bytes + temp_bytes);
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
            emit!(self, "    mov rax, {}", loc.q(1));
            for i in 1..ndims {
                emit!(self, "    imul rax, {}", loc.q(1 + i as i32));
            }
            emit!(self, "    imul rax, {}", elem_size);
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
            emit!(self, "    mov {}, rax", loc.q(1 + i as i32));
        }

        // Calculate total elements: dim0 * dim1 * dim2 * ...
        emit!(self, "    mov rax, {}", loc.q(1));
        for i in 1..ndims {
            emit!(self, "    imul rax, {}", loc.q(1 + i as i32));
        }
        emit!(self, "    imul rax, {}", elem_size);

        if preserve {
            // realloc(old_ptr, new_size)
            self.emit("    mov r10, rax"); // new size in bytes
            self.emit("    push r10");
            self.emit("    sub rsp, 8"); // keep rsp 16-byte aligned across the call
            emit!(self, "    mov {}, {}", Self::arg_reg(0), loc.q(0));
            self.emit_arg_reg(1, "r10");
            self.emit_call_libc("realloc");
            self.emit("    add rsp, 8");
            self.emit("    pop r10"); // new size
        } else {
            // calloc(1, size): BASIC guarantees a fresh array reads as 0 / "",
            // which malloc alone does not.
            emit!(self, "    mov {}, 1", Self::arg_reg(0));
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
        emit!(self, "    mov {}, rax", loc.q(0));

        if preserve {
            // Zero the newly added tail, from the old size up to the new one.
            self.emit("    add rsp, 8");
            self.emit("    pop r11"); // old size in bytes
            self.emit("    mov rcx, r11");
            let loop_label = self.new_label("preserve_zero");
            let done_label = self.new_label("preserve_done");
            self.emit_label(&loop_label);
            self.emit("    cmp rcx, r10");
            emit!(self, "    jae {}", done_label);
            self.emit("    mov BYTE PTR [rax + rcx], 0");
            self.emit("    inc rcx");
            emit!(self, "    jmp {}", loop_label);
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
    /// Element sizes x86-64 can scale an index by in an addressing mode.
    ///
    /// Which is every scalar element type -- INTEGER 2, LONG and SINGLE 4,
    /// DOUBLE 8. A string element is sixteen bytes and a record element is
    /// eight times its word count, and neither is a legal scale, so those keep
    /// the multiply.
    fn is_sib_scale(elem_size: i32) -> bool {
        matches!(elem_size, 1 | 2 | 4 | 8)
    }

    /// Compute an array element's linear index into `rax`, bounds-checked.
    ///
    /// Stops one step short of the address, returning the descriptor and the
    /// element size so the caller can choose between building a flat pointer
    /// and folding the scale into whatever addressing mode it was going to
    /// use anyway.
    fn gen_array_index(&mut self, name: &str, indices: &[Expr]) -> (Loc, i32) {
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
        //
        // Kept per access rather than hoisted with the pointer: a loop whose
        // body never runs must not report an array it never touched.
        if self.opts.checks {
            let base = self.array_base_operand(name, &loc);
            emit!(self, "    cmp {}, 0", base);
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
            let bound = self.array_bound_operand(name, &loc, 0);
            emit!(self, "    cmp rax, {}", bound);
            self.emit_check("jae", RtError::Subscript);
            self.emit_lower_bound_check("rax");
        }

        // For each subsequent index, multiply by dimension bound and add
        for (i, idx_expr) in indices.iter().enumerate().skip(1) {
            // Save current accumulated index - use 16 bytes for alignment
            emit!(self, "    sub rsp, {}", STACK_TEMP_SPACE);
            self.emit("    mov QWORD PTR [rsp], rax");
            // Evaluate next index
            let idx_type = self.gen_expr(idx_expr);
            if idx_type.is_integer() {
                self.emit("    movsxd rcx, eax");
            } else {
                self.emit("    cvttsd2si rcx, xmm0");
            }
            let bound = self.array_bound_operand(name, &loc, i);
            if self.opts.checks {
                emit!(self, "    cmp rcx, {}", bound);
                self.emit_check("jae", RtError::Subscript);
                self.emit_lower_bound_check("rcx");
            }
            self.emit("    mov rax, QWORD PTR [rsp]");
            emit!(self, "    add rsp, {}", STACK_TEMP_SPACE);
            // rax = rax * dim[i] + indices[i]
            emit!(self, "    imul rax, {}", bound);
            self.emit("    add rax, rcx");
        }

        (loc, elem_size)
    }

    /// The address of an array element, in `rax`.
    fn gen_array_addr(&mut self, name: &str, indices: &[Expr]) {
        let (loc, elem_size) = self.gen_array_index(name, indices);

        // `lea` with a scaled index does the multiply and the add at once, and
        // does the multiply as part of the address rather than as a `imul`.
        // The base is loaded here rather than earlier because evaluating an
        // index runs arbitrary code, which is free to clobber r10.
        if Self::is_sib_scale(elem_size) {
            let base = self.array_base_into_register(name, &loc);
            emit!(self, "    lea rax, [{} + rax*{}]", base, elem_size);
        } else {
            emit!(self, "    imul rax, {}", elem_size);
            let base = self.array_base_operand(name, &loc);
            emit!(self, "    add rax, {}", base);
        }
    }

    fn gen_array_load(&mut self, name: &str, indices: &[Expr]) {
        let elem_type = DataType::from_suffix(name);
        let (loc, elem_size) = self.gen_array_index(name, indices);

        // A scalar element needs no address of its own: the same scaled index
        // that would have gone into a `lea` goes into the load instead, so the
        // read costs one instruction rather than three.
        if Self::is_sib_scale(elem_size) && elem_type != DataType::String {
            let base = self.array_base_into_register(name, &loc);
            let at = format!("[{} + rax*{}]", base, elem_size);
            match elem_type {
                DataType::Integer => emit!(self, "    movsx eax, WORD PTR {}", at),
                DataType::Long => emit!(self, "    mov eax, DWORD PTR {}", at),
                DataType::Single => emit!(self, "    movss xmm0, DWORD PTR {}", at),
                DataType::Double => emit!(self, "    movsd xmm0, QWORD PTR {}", at),
                DataType::String => unreachable!("excluded above"),
            }
            return;
        }

        // Otherwise build the address and read through it.
        emit!(self, "    imul rax, {}", elem_size);
        let base = self.array_base_operand(name, &loc);
        emit!(self, "    add rax, {}", base);
        match elem_type {
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
        emit!(self, "    sub rsp, {}", STACK_TEMP_SPACE);
        self.emit("    mov QWORD PTR [rsp], rax");

        let val_type = self.gen_expr(value);
        if val_type == DataType::String {
            self.emit_string_copy_of(value);
        }

        self.emit("    mov rcx, QWORD PTR [rsp]");
        emit!(self, "    add rsp, {}", STACK_TEMP_SPACE);

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
        self.emit_string_copy_of(value);
        // Both words were reserved when the variable was first seen, so this no
        // longer has to scavenge a slot per assignment.
        let loc = self.get_var_loc(name);
        emit!(self, "    mov {}, rax", loc.q(0));
        emit!(self, "    mov {}, rdx", loc.q(1));
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

        // Double constant pool. Emitted after the strings, which are packed
        // bytes with no alignment of their own, so it states its own; the
        // DATA table below is protected by the `.p2align 3` already there.
        //
        // `.data` rather than a read-only section: nothing here is ever
        // written, but `emit_data_section` has no `.section` machinery and the
        // second assembler this has to please is clang targeting COFF. The
        // size saved is the same either way.
        if !self.f64_pool.is_empty() {
            self.output.push_str(".p2align 3\n");
            let pool = std::mem::take(&mut self.f64_pool);
            for (i, bits) in pool.iter().enumerate() {
                self.output
                    .push_str(&format!("_f64_{}: .quad 0x{:016X}\n", i, bits));
            }
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
            emit!(
                self,
                "_gosub_stack: .skip {}  # GOSUB return stack (64K entries)",
                GOSUB_STACK_SIZE
            );
        }
    }
}
