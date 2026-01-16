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
//! # Stack Frame Layout
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
//! All local variables are allocated 8 bytes regardless of type (for alignment).
//! Variable offsets are always negative relative to `rbp`.
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
//! - String variables: Two consecutive 8-byte slots at `[rbp + offset]` (ptr) and
//!   `[rbp + offset - 8]` (len), where offset is negative (e.g., -8, -16).
//!   The ptr is at higher address, len at lower address (stack grows downward).
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
use crate::parser::*;
use std::collections::HashMap;
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

/// ASCII character codes
const ASCII_TAB: i64 = 9;

fn is_string_var(name: &str) -> bool {
    name.ends_with('$')
}

/// Variable storage information
#[derive(Clone)]
struct VarInfo {
    offset: i32,
    data_type: DataType,
}

/// Metadata for array storage
struct ArrayInfo {
    ptr_offset: i32,       // stack offset where array pointer is stored
    dim_offsets: Vec<i32>, // stack offsets where dimension bounds are stored
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
    gosub_used: bool,               // whether GOSUB is used (need return stack)
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

        // Allocate new variable - determine type from suffix
        let data_type = DataType::from_suffix(name);
        self.stack_offset -= 8; // All types use 8 bytes for alignment
        let offset = self.stack_offset;

        let info = VarInfo { offset, data_type };

        if self.current_proc.is_some() {
            self.proc_vars.insert(name.to_string(), info.clone());
        } else {
            self.vars.insert(name.to_string(), info.clone());
        }

        info
    }

    /// Get just the stack offset for a variable (convenience method)
    fn get_var_offset(&mut self, name: &str) -> i32 {
        self.get_var_info(name).offset
    }

    /// Determine the result type of an expression
    fn expr_type(&self, expr: &Expr) -> DataType {
        match expr {
            Expr::Literal(lit) => match lit {
                Literal::Integer(_) => DataType::Long, // Integer literals are Long
                Literal::Float(_) => DataType::Double,
                Literal::String(_) => DataType::String,
            },
            Expr::Variable(name) => DataType::from_suffix(name),
            Expr::ArrayAccess { name, .. } => DataType::from_suffix(name),
            Expr::FnCall { name, .. } => self.fn_return_type(name),
            Expr::Unary { operand, .. } => self.expr_type(operand),
            Expr::Binary { left, right, op } => {
                let lt = self.expr_type(left);
                let rt = self.expr_type(right);
                self.promote_types(lt, rt, *op)
            }
        }
    }

    /// Get the return type of a function (built-in or user-defined)
    fn fn_return_type(&self, name: &str) -> DataType {
        // Built-in functions that return strings
        let upper = name.to_uppercase();
        if upper.ends_with('$') {
            return DataType::String;
        }
        // Built-in functions that return integers
        match upper.as_str() {
            "LEN" | "ASC" | "INSTR" | "CINT" | "CLNG" => DataType::Long,
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

    pub fn generate(&mut self, program: &Program) -> String {
        // First pass: collect DATA statements and check for GOSUB
        for stmt in &program.statements {
            self.preprocess(stmt);
        }

        // Emit assembly header
        self.emit(".intel_syntax noprefix");
        self.emit(".text");
        let p = PREFIX;
        self.emit(&format!(".globl {}main", p));
        self.emit("");

        // Generate procedures first
        for stmt in &program.statements {
            if let Stmt::Sub { name, params, body } = stmt {
                self.gen_procedure(name, params, body, false);
            } else if let Stmt::Function { name, params, body } = stmt {
                self.gen_procedure(name, params, body, true);
            }
        }

        // Generate main
        self.emit_label(&format!("{}main", p));
        self.emit("    push rbp");
        self.emit("    mov rbp, rsp");

        // Reserve stack space (will patch later)
        self.emit("    sub rsp, 0         # STACK_RESERVE");

        // Initialize GOSUB return stack if needed
        if self.gosub_used {
            self.emit("    # Initialize GOSUB return stack");
            self.emit("    lea rax, [rip + _gosub_stack + 8192]"); // Point to end (stack grows down)
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
            match stmt {
                Stmt::Sub { .. } | Stmt::Function { .. } => {}
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
        let stack_needed = -self.stack_offset;
        let stack_size = (stack_needed + 15) & !15; // Round up to multiple of 16
        let old = "    sub rsp, 0         # STACK_RESERVE";
        let new = format!("    sub rsp, {}        # STACK_RESERVE", stack_size);
        self.output = self.output.replace(old, &new);

        // Emit data section
        self.emit_data_section();

        self.output.clone()
    }

    /// Preprocess statement: collect DATA items and check for GOSUB usage
    fn preprocess(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Data(values) => self.data_items.extend(values.clone()),
            Stmt::Gosub(_) => self.gosub_used = true,
            _ => {}
        }
        // Recurse into nested statements
        let bodies: Vec<&[Stmt]> = match stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                let mut v = vec![then_branch.as_slice()];
                if let Some(eb) = else_branch {
                    v.push(eb.as_slice());
                }
                v
            }
            Stmt::For { body, .. }
            | Stmt::While { body, .. }
            | Stmt::DoLoop { body, .. }
            | Stmt::Sub { body, .. }
            | Stmt::Function { body, .. } => vec![body.as_slice()],
            _ => vec![],
        };
        for body in bodies {
            for s in body {
                self.preprocess(s);
            }
        }
    }

    fn gen_procedure(&mut self, name: &str, params: &[String], body: &[Stmt], is_function: bool) {
        self.current_proc = Some(name.to_string());
        self.proc_vars.clear();
        let old_stack_offset = self.stack_offset;
        self.stack_offset = 0;

        // Procedure label
        self.emit_label(&format!("_proc_{}", name));
        self.emit("    push rbp");
        self.emit("    mov rbp, rsp");

        // Reserve stack space (will patch later with actual size)
        let placeholder = format!("    sub rsp, 0         # STACK_RESERVE_PROC_{}", name);
        self.emit(&placeholder);

        // Parameters are passed in registers (per platform ABI)
        // Store them in the reserved stack space
        let int_regs = PlatformAbi::INT_ARG_REGS;
        for (i, param) in params.iter().enumerate() {
            self.stack_offset -= 8;
            let data_type = DataType::from_suffix(param);
            self.proc_vars.insert(
                param.clone(),
                VarInfo {
                    offset: self.stack_offset,
                    data_type,
                },
            );
            if i < int_regs.len() {
                self.emit(&format!(
                    "    mov QWORD PTR [rbp + {}], {}",
                    self.stack_offset, int_regs[i]
                ));
            }
        }

        // If function, allocate return value slot
        if is_function {
            self.stack_offset -= 8;
            let data_type = DataType::from_suffix(name);
            self.proc_vars.insert(
                name.to_string(),
                VarInfo {
                    offset: self.stack_offset,
                    data_type,
                },
            );
        }

        // Generate body
        for stmt in body {
            self.gen_stmt(stmt);
        }

        // Return - load return value into appropriate register based on type
        if is_function {
            let ret_info = &self.proc_vars[name];
            let offset = ret_info.offset;
            let data_type = ret_info.data_type;
            match data_type {
                DataType::Integer => {
                    self.emit(&format!("    movsx eax, WORD PTR [rbp + {}]", offset));
                }
                DataType::Long => {
                    self.emit(&format!("    mov eax, DWORD PTR [rbp + {}]", offset));
                }
                DataType::Single => {
                    self.emit(&format!("    movss xmm0, DWORD PTR [rbp + {}]", offset));
                }
                DataType::Double => {
                    self.emit(&format!("    movsd xmm0, QWORD PTR [rbp + {}]", offset));
                }
                DataType::String => {
                    // Load string (ptr, len) into rax, rdx
                    self.emit(&format!("    mov rax, QWORD PTR [rbp + {}]", offset));
                    self.emit(&format!("    mov rdx, QWORD PTR [rbp + {}]", offset - 8));
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
        match stmt {
            Stmt::Label(n) => {
                self.emit_label(&format!("_line_{}", n));
            }

            Stmt::Let {
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
                    match var_info.data_type {
                        DataType::Integer => {
                            self.emit(&format!("    mov WORD PTR [rbp + {}], ax", var_info.offset));
                        }
                        DataType::Long => {
                            self.emit(&format!(
                                "    mov DWORD PTR [rbp + {}], eax",
                                var_info.offset
                            ));
                        }
                        DataType::Single => {
                            self.emit(&format!(
                                "    movss DWORD PTR [rbp + {}], xmm0",
                                var_info.offset
                            ));
                        }
                        DataType::Double => {
                            self.emit(&format!(
                                "    movsd QWORD PTR [rbp + {}], xmm0",
                                var_info.offset
                            ));
                        }
                        DataType::String => {
                            // Should be handled by gen_string_assign above
                            unreachable!("String assignment should be handled separately");
                        }
                    }
                }
            }

            Stmt::Print { items, newline } => {
                for item in items {
                    match item {
                        PrintItem::Expr(expr) => {
                            self.gen_print_expr(expr);
                        }
                        PrintItem::Tab => {
                            self.emit_arg_imm(0, ASCII_TAB);
                            self.emit("    call _rt_print_char");
                        }
                        PrintItem::Empty => {}
                    }
                }
                if *newline {
                    self.emit("    call _rt_print_newline");
                }
            }

            Stmt::Input { prompt, vars } => {
                if let Some(pstr) = prompt {
                    let idx = self.add_string_literal(pstr);
                    self.emit_arg_lea(0, &format!("[rip + _str_{}]", idx));
                    self.emit_arg_imm(1, pstr.len() as i64);
                    self.emit("    call _rt_print_string");
                }
                for var in vars {
                    if is_string_var(var) {
                        self.emit("    call _rt_input_string");
                        let offset = self.get_var_offset(var);
                        self.emit(&format!("    mov QWORD PTR [rbp + {}], rax", offset));
                        self.emit(&format!("    mov QWORD PTR [rbp + {}], rdx", offset - 8));
                    } else {
                        self.emit("    call _rt_input_number");
                        let offset = self.get_var_offset(var);
                        self.emit(&format!("    movsd QWORD PTR [rbp + {}], xmm0", offset));
                    }
                }
            }

            Stmt::LineInput { prompt, var } => {
                if let Some(pstr) = prompt {
                    let idx = self.add_string_literal(pstr);
                    self.emit_arg_lea(0, &format!("[rip + _str_{}]", idx));
                    self.emit_arg_imm(1, pstr.len() as i64);
                    self.emit("    call _rt_print_string");
                }
                self.emit("    call _rt_input_string");
                let offset = self.get_var_offset(var);
                self.emit(&format!("    mov QWORD PTR [rbp + {}], rax", offset));
                self.emit(&format!("    mov QWORD PTR [rbp + {}], rdx", offset - 8));
            }

            Stmt::If {
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

            Stmt::For {
                var,
                start,
                end,
                step,
                body,
            } => {
                let start_label = self.new_label("for");
                let end_label = self.new_label("endfor");
                let var_offset = self.get_var_offset(var);

                // Initialize loop variable - coerce to double
                let start_type = self.gen_expr(start);
                self.gen_coercion(start_type, DataType::Double);
                self.emit(&format!("    movsd QWORD PTR [rbp + {}], xmm0", var_offset));

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
                self.emit(&format!("    movsd xmm0, QWORD PTR [rbp + {}]", var_offset));
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
                for s in body {
                    self.gen_stmt(s);
                }

                // Increment
                self.emit(&format!("    movsd xmm0, QWORD PTR [rbp + {}]", var_offset));
                self.emit(&format!(
                    "    addsd xmm0, QWORD PTR [rbp + {}]",
                    step_offset
                ));
                self.emit(&format!("    movsd QWORD PTR [rbp + {}], xmm0", var_offset));
                self.emit(&format!("    jmp {}", start_label));

                self.emit_label(&end_label);
            }

            Stmt::While { condition, body } => {
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

                for s in body {
                    self.gen_stmt(s);
                }
                self.emit(&format!("    jmp {}", start_label));

                self.emit_label(&end_label);
            }

            Stmt::DoLoop {
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

                for s in body {
                    self.gen_stmt(s);
                }

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

            Stmt::Goto(target) => {
                let label = match target {
                    GotoTarget::Line(n) => format!("_line_{}", n),
                    GotoTarget::Label(s) => format!("_label_{}", s),
                };
                self.emit(&format!("    jmp {}", label));
            }

            Stmt::Gosub(target) => {
                let label = match target {
                    GotoTarget::Line(n) => format!("_line_{}", n),
                    GotoTarget::Label(s) => format!("_label_{}", s),
                };
                let ret_label = self.new_label("gosub_ret");
                // Push return address to GOSUB stack (use rcx - caller-saved on both ABIs)
                self.emit(&format!("    lea rax, [rip + {}]", ret_label));
                self.emit("    mov rcx, QWORD PTR [rip + _gosub_sp]");
                self.emit("    sub rcx, 8");
                self.emit("    mov QWORD PTR [rcx], rax");
                self.emit("    mov QWORD PTR [rip + _gosub_sp], rcx");
                self.emit(&format!("    jmp {}", label));
                self.emit_label(&ret_label);
            }

            Stmt::Return => {
                // Pop return address from GOSUB stack and jump (use rcx - caller-saved on both ABIs)
                self.emit("    mov rcx, QWORD PTR [rip + _gosub_sp]");
                self.emit("    mov rax, QWORD PTR [rcx]");
                self.emit("    add rcx, 8");
                self.emit("    mov QWORD PTR [rip + _gosub_sp], rcx");
                self.emit("    jmp rax");
            }

            Stmt::OnGoto { expr, targets } => {
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
                        GotoTarget::Label(s) => format!("_label_{}", s),
                    };
                    self.emit(&format!("    cmp rax, {}", i + 1));
                    self.emit(&format!("    je {}", label));
                }
            }

            Stmt::Dim { arrays } => {
                for arr in arrays {
                    self.gen_dim_array(arr);
                }
            }

            Stmt::Sub { .. } | Stmt::Function { .. } => {
                // Already handled in first pass
            }

            Stmt::Call { name, args } => {
                self.gen_call(name, args);
            }

            Stmt::Data(_) => {
                // Data already collected in first pass
            }

            Stmt::Read(vars) => {
                for var in vars {
                    if is_string_var(var) {
                        self.emit("    call _rt_read_string");
                        let offset = self.get_var_offset(var);
                        self.emit(&format!("    mov QWORD PTR [rbp + {}], rax", offset));
                    } else {
                        self.emit("    call _rt_read_number");
                        let offset = self.get_var_offset(var);
                        self.emit(&format!("    movsd QWORD PTR [rbp + {}], xmm0", offset));
                    }
                }
            }

            Stmt::Restore(target) => {
                let idx = if let Some(_t) = target {
                    // TODO: find DATA line index
                    0
                } else {
                    0
                };
                self.emit_arg_imm(0, idx);
                self.emit("    call _rt_restore");
            }

            Stmt::Cls => {
                self.emit("    call _rt_cls");
            }

            Stmt::SelectCase { expr, cases } => {
                let end_label = self.new_label("endselect");

                // Evaluate SELECT expression and save to temp
                let expr_type = self.gen_expr(expr);
                self.gen_coercion(expr_type, DataType::Double);
                self.stack_offset -= 8;
                let temp_offset = self.stack_offset;
                self.emit(&format!(
                    "    movsd QWORD PTR [rbp + {}], xmm0",
                    temp_offset
                ));

                // Generate code for each case
                for (i, (case_value, body)) in cases.iter().enumerate() {
                    let next_case_label = if i + 1 < cases.len() {
                        self.new_label("case")
                    } else {
                        end_label.clone()
                    };

                    if let Some(value) = case_value {
                        // Evaluate case value and compare
                        let val_type = self.gen_expr(value);
                        self.gen_coercion(val_type, DataType::Double);
                        self.emit(&format!(
                            "    movsd xmm1, QWORD PTR [rbp + {}]",
                            temp_offset
                        ));
                        self.emit("    ucomisd xmm0, xmm1");
                        self.emit(&format!("    jne {}", next_case_label));
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

            Stmt::End | Stmt::Stop => {
                self.emit("    xor eax, eax");
                self.emit("    leave");
                self.emit("    ret");
            }

            Stmt::Open {
                filename,
                mode,
                file_num,
            } => {
                // _rt_file_open(filename_ptr, filename_len, mode, file_num)
                self.gen_expr(filename);
                self.emit_arg_reg(0, "rax"); // filename ptr
                self.emit_arg_reg(1, "rdx"); // filename len
                let mode_num = match mode {
                    FileMode::Input => 0,
                    FileMode::Output => 1,
                    FileMode::Append => 2,
                };
                self.emit_arg_imm(2, mode_num);
                self.emit_arg_imm(3, *file_num as i64);
                self.emit("    call _rt_file_open");
            }

            Stmt::Close { file_num } => {
                self.emit_arg_imm(0, *file_num as i64);
                self.emit("    call _rt_file_close");
            }

            Stmt::PrintFile {
                file_num,
                items,
                newline,
            } => {
                for item in items {
                    match item {
                        PrintItem::Expr(expr) => {
                            self.gen_print_expr_to_file(expr, *file_num);
                        }
                        PrintItem::Tab => {
                            self.emit_arg_imm(0, *file_num as i64);
                            self.emit_arg_imm(1, ASCII_TAB);
                            self.emit("    call _rt_file_print_char");
                        }
                        PrintItem::Empty => {}
                    }
                }
                if *newline {
                    self.emit_arg_imm(0, *file_num as i64);
                    self.emit("    call _rt_file_print_newline");
                }
            }

            Stmt::InputFile { file_num, vars } => {
                for var in vars {
                    if is_string_var(var) {
                        self.emit_arg_imm(0, *file_num as i64);
                        self.emit("    call _rt_file_input_string");
                        let offset = self.get_var_offset(var);
                        self.emit(&format!("    mov QWORD PTR [rbp + {}], rax", offset));
                        self.emit(&format!("    mov QWORD PTR [rbp + {}], rdx", offset - 8));
                    } else {
                        self.emit_arg_imm(0, *file_num as i64);
                        self.emit("    call _rt_file_input_number");
                        let offset = self.get_var_offset(var);
                        self.emit(&format!("    movsd QWORD PTR [rbp + {}], xmm0", offset));
                    }
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
                Literal::Integer(n) => {
                    // Load as integer into eax
                    self.emit(&format!("    mov eax, {}", *n as i32));
                    DataType::Long
                }
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
                let info = self.get_var_info(name);
                match info.data_type {
                    DataType::Integer => {
                        self.emit(&format!("    movsx eax, WORD PTR [rbp + {}]", info.offset));
                    }
                    DataType::Long => {
                        self.emit(&format!("    mov eax, DWORD PTR [rbp + {}]", info.offset));
                    }
                    DataType::Single => {
                        self.emit(&format!(
                            "    movss xmm0, DWORD PTR [rbp + {}]",
                            info.offset
                        ));
                    }
                    DataType::Double => {
                        self.emit(&format!(
                            "    movsd xmm0, QWORD PTR [rbp + {}]",
                            info.offset
                        ));
                    }
                    DataType::String => {
                        self.emit(&format!("    mov rax, QWORD PTR [rbp + {}]", info.offset));
                        self.emit(&format!(
                            "    mov rdx, QWORD PTR [rbp + {}]",
                            info.offset - 8
                        ));
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
                self.fn_return_type(name)
            }
        }
    }

    /// Generate code for a binary expression
    fn gen_binary_expr(&mut self, op: BinaryOp, left: &Expr, right: &Expr) -> DataType {
        let result_type = self.promote_types(self.expr_type(left), self.expr_type(right), op);

        // Handle string concatenation specially
        if result_type == DataType::String && op == BinaryOp::Add {
            // Evaluate left string (ptr in rax, len in rdx)
            self.gen_expr(left);
            // Save left string on stack (ptr, len)
            self.emit("    push rdx"); // left len
            self.emit("    push rax"); // left ptr

            // Evaluate right string (ptr in rax, len in rdx)
            self.gen_expr(right);
            // Now: right ptr in rax, right len in rdx
            // Stack: left ptr, left len

            // Call runtime string concat: rt_strcat(left_ptr, left_len, right_ptr, right_len)
            // Save right string temporarily
            self.emit("    mov r8, rax"); // right ptr
            self.emit("    mov r9, rdx"); // right len
            // Pop left string (LIFO: ptr popped first since it was pushed last)
            self.emit("    pop rax"); // left ptr from stack
            self.emit("    pop rdx"); // left len from stack
            self.emit_arg_reg(0, "rax"); // left ptr
            self.emit_arg_reg(1, "rdx"); // left len
            self.emit_arg_reg(2, "r8"); // right ptr
            self.emit_arg_reg(3, "r9"); // right len
            self.emit("    call _rt_strcat");
            // Result: ptr in rax, len in rdx
            return DataType::String;
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
                self.emit("    divsd xmm0, xmm1");
            }
            BinaryOp::IntDiv => {
                self.emit_cvt_float_to_int(work_type);
                self.emit("    cdq");
                self.emit("    idiv ecx");
            }
            BinaryOp::Mod => {
                self.emit_cvt_float_to_int(work_type);
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

        result_type
    }

    fn gen_print_expr(&mut self, expr: &Expr) {
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
            // Numeric expression - evaluate and convert to double for printing
            let expr_type = self.gen_expr(expr);
            self.gen_coercion(expr_type, DataType::Double);
            self.emit("    call _rt_print_float");
        }
    }

    fn gen_print_expr_to_file(&mut self, expr: &Expr, file_num: i32) {
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
            self.emit_arg_imm(0, file_num as i64); // file_num → rcx or rdi
            self.emit("    call _rt_file_print_string");
        } else {
            // Numeric expression - evaluate and convert to double for printing
            let expr_type = self.gen_expr(expr);
            self.gen_coercion(expr_type, DataType::Double);
            self.emit_arg_imm(0, file_num as i64);
            self.emit("    call _rt_file_print_float");
        }
    }

    fn gen_fn_call(&mut self, name: &str, args: &[Expr]) {
        let upper_name = name.to_uppercase();

        // Table-driven: libc math functions (SIN, COS, TAN, ATN, EXP, LOG)
        if let Some(libc_fn) = LIBC_MATH_FNS.get(upper_name.as_str()) {
            let arg_type = self.gen_expr(&args[0]);
            self.gen_coercion(arg_type, DataType::Double);
            self.emit_call_libc(libc_fn);
            return;
        }

        // Table-driven: inline math functions (SQR, INT, FIX)
        if let Some(instr) = INLINE_MATH_FNS.get(upper_name.as_str()) {
            let arg_type = self.gen_expr(&args[0]);
            self.gen_coercion(arg_type, DataType::Double);
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
                self.gen_expr(&args[0]); // string: rax=ptr, rdx=len
                self.emit_arg_reg(0, "rax"); // ptr
                self.emit_arg_reg(1, "rdx"); // len
                let count_type = self.gen_expr(&args[1]); // count
                let arg2 = Self::arg_reg(2);
                if count_type.is_integer() {
                    self.emit(&format!("    movsxd {}, eax", arg2));
                } else {
                    self.emit(&format!("    cvttsd2si {}, xmm0", arg2));
                }
                self.emit("    call _rt_left");
            }
            "RIGHT$" => {
                // _rt_right(ptr, len, count)
                self.gen_expr(&args[0]);
                self.emit_arg_reg(0, "rax"); // ptr
                self.emit_arg_reg(1, "rdx"); // len
                let count_type = self.gen_expr(&args[1]);
                let arg2 = Self::arg_reg(2);
                if count_type.is_integer() {
                    self.emit(&format!("    movsxd {}, eax", arg2));
                } else {
                    self.emit(&format!("    cvttsd2si {}, xmm0", arg2));
                }
                self.emit("    call _rt_right");
            }
            "MID$" => {
                // _rt_mid(ptr, len, start, count)
                self.gen_expr(&args[0]);
                self.emit_arg_reg(0, "rax"); // ptr
                self.emit_arg_reg(1, "rdx"); // len
                let pos_type = self.gen_expr(&args[1]);
                let arg2 = Self::arg_reg(2);
                if pos_type.is_integer() {
                    self.emit(&format!("    movsxd {}, eax", arg2));
                } else {
                    self.emit(&format!("    cvttsd2si {}, xmm0", arg2));
                }
                let arg3 = Self::arg_reg(3);
                if args.len() > 2 {
                    let len_type = self.gen_expr(&args[2]);
                    if len_type.is_integer() {
                        self.emit(&format!("    movsxd {}, eax", arg3));
                    } else {
                        self.emit(&format!("    cvttsd2si {}, xmm0", arg3));
                    }
                } else {
                    self.emit(&format!("    mov {}, -1", arg3)); // rest of string
                }
                self.emit("    call _rt_mid");
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
            "CSNG" | "CDBL" => {
                let arg_type = self.gen_expr(&args[0]);
                // Convert to double
                self.gen_coercion(arg_type, DataType::Double);
            }
            "TIMER" => {
                self.emit("    call _rt_timer");
            }
            _ => {
                // User-defined function or array access
                if self.arrays.contains_key(&upper_name) || upper_name.ends_with('$') {
                    // Array access
                    self.gen_array_load(&upper_name, args);
                } else {
                    // User function call
                    self.gen_call(name, args);
                }
            }
        }
    }

    fn gen_call(&mut self, name: &str, args: &[Expr]) {
        // Push args in registers (per platform ABI)
        let int_regs = PlatformAbi::INT_ARG_REGS;

        // Save current xmm0 if we'll use it for args
        // Use 16 bytes to maintain stack alignment during arg evaluation
        if !args.is_empty() {
            self.emit(&format!("    sub rsp, {}", STACK_TEMP_SPACE));
            self.emit("    movsd QWORD PTR [rsp], xmm0");
        }

        // Pass args: numeric as doubles in integer registers, strings as ptr/len pairs
        let mut reg_idx = 0;
        for arg in args.iter() {
            let arg_type = self.gen_expr(arg);
            if arg_type == DataType::String {
                // String: ptr in rax, len in rdx - pass as two args
                if reg_idx < int_regs.len() {
                    self.emit(&format!("    mov {}, rax", int_regs[reg_idx]));
                    reg_idx += 1;
                }
                if reg_idx < int_regs.len() {
                    self.emit(&format!("    mov {}, rdx", int_regs[reg_idx]));
                    reg_idx += 1;
                }
            } else {
                // Numeric: coerce to double and pass in integer register
                self.gen_coercion(arg_type, DataType::Double);
                if reg_idx < int_regs.len() {
                    self.emit(&format!("    movq {}, xmm0", int_regs[reg_idx]));
                    reg_idx += 1;
                }
            }
        }

        self.emit(&format!("    call _proc_{}", name));

        if !args.is_empty() {
            self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));
        }
    }

    fn gen_dim_array(&mut self, arr: &ArrayDecl) {
        let elem_size = if is_string_var(&arr.name) { 16 } else { 8 };

        // First, evaluate and store all dimension bounds
        // BASIC DIM A(N) means indices 0..N (N+1 elements), so add 1 to each bound
        let mut dim_offsets = Vec::new();
        for dim in arr.dimensions.iter() {
            let dim_type = self.gen_expr(dim);
            if dim_type.is_integer() {
                // Value already in eax, sign-extend to rax
                self.emit("    movsxd rax, eax");
            } else {
                self.emit("    cvttsd2si rax, xmm0");
            }
            self.emit("    inc rax"); // DIM A(N) has N+1 elements (0 to N)
            self.stack_offset -= 8;
            dim_offsets.push(self.stack_offset);
            self.emit(&format!(
                "    mov QWORD PTR [rbp + {}], rax",
                self.stack_offset
            ));
        }

        // Calculate total elements: dim0 * dim1 * dim2 * ...
        self.emit(&format!(
            "    mov rax, QWORD PTR [rbp + {}]",
            dim_offsets[0]
        ));
        for offset in dim_offsets.iter().skip(1) {
            self.emit(&format!("    imul rax, QWORD PTR [rbp + {}]", offset));
        }

        // Allocate: total_elements * elem_size
        let arg0 = Self::arg_reg(0);
        self.emit(&format!("    imul {}, rax, {}", arg0, elem_size));
        self.emit_call_libc("malloc");

        // Store array pointer
        self.stack_offset -= 8;
        let ptr_offset = self.stack_offset;
        self.emit(&format!("    mov QWORD PTR [rbp + {}], rax", ptr_offset));

        // Record array info
        self.arrays.insert(
            arr.name.clone(),
            ArrayInfo {
                ptr_offset,
                dim_offsets,
            },
        );
    }

    fn gen_array_load(&mut self, name: &str, indices: &[Expr]) {
        let arr_info = self.arrays.get(name).expect("Array not declared");
        let ptr_offset = arr_info.ptr_offset;
        let dim_offsets = arr_info.dim_offsets.clone();
        let elem_size = if is_string_var(name) { 16 } else { 8 };

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
            self.emit("    mov rax, QWORD PTR [rsp]");
            self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));
            // rax = rax * dim[i] + indices[i]
            self.emit(&format!(
                "    imul rax, QWORD PTR [rbp + {}]",
                dim_offsets[i]
            ));
            self.emit("    add rax, rcx");
        }

        // Multiply by element size and add to base pointer
        self.emit(&format!("    imul rax, {}", elem_size));
        self.emit(&format!("    add rax, QWORD PTR [rbp + {}]", ptr_offset));

        // Load value from computed address
        if is_string_var(name) {
            self.emit("    mov rcx, rax");
            self.emit("    mov rax, QWORD PTR [rcx]");
            self.emit("    mov rdx, QWORD PTR [rcx + 8]");
        } else {
            self.emit("    movsd xmm0, QWORD PTR [rax]");
        }
    }

    fn gen_array_store(&mut self, name: &str, indices: &[Expr], value: &Expr) {
        let arr_info = self.arrays.get(name).expect("Array not declared");
        let ptr_offset = arr_info.ptr_offset;
        let dim_offsets = arr_info.dim_offsets.clone();
        let elem_size = if is_string_var(name) { 16 } else { 8 };

        // Calculate linear index using row-major order (same as gen_array_load)
        let idx_type = self.gen_expr(&indices[0]);
        if idx_type.is_integer() {
            self.emit("    movsxd rax, eax");
        } else {
            self.emit("    cvttsd2si rax, xmm0");
        }

        for (i, idx_expr) in indices.iter().enumerate().skip(1) {
            // Save current accumulated index - use 16 bytes for alignment
            self.emit(&format!("    sub rsp, {}", STACK_TEMP_SPACE));
            self.emit("    mov QWORD PTR [rsp], rax");
            let idx_type = self.gen_expr(idx_expr);
            if idx_type.is_integer() {
                self.emit("    movsxd rcx, eax");
            } else {
                self.emit("    cvttsd2si rcx, xmm0");
            }
            self.emit("    mov rax, QWORD PTR [rsp]");
            self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));
            self.emit(&format!(
                "    imul rax, QWORD PTR [rbp + {}]",
                dim_offsets[i]
            ));
            self.emit("    add rax, rcx");
        }

        // Compute final address and save it - use 16 bytes for alignment
        self.emit(&format!("    imul rax, {}", elem_size));
        self.emit(&format!("    add rax, QWORD PTR [rbp + {}]", ptr_offset));
        self.emit(&format!("    sub rsp, {}", STACK_TEMP_SPACE));
        self.emit("    mov QWORD PTR [rsp], rax"); // save address

        // Evaluate value
        let val_type = self.gen_expr(value);

        // Store value at computed address
        self.emit("    mov rcx, QWORD PTR [rsp]");
        self.emit(&format!("    add rsp, {}", STACK_TEMP_SPACE));
        if is_string_var(name) {
            self.emit("    mov QWORD PTR [rcx], rax");
            self.emit("    mov QWORD PTR [rcx + 8], rdx");
        } else {
            // Coerce to double for array storage
            self.gen_coercion(val_type, DataType::Double);
            self.emit("    movsd QWORD PTR [rcx], xmm0");
        }
    }

    fn gen_string_assign(&mut self, name: &str, value: &Expr) {
        self.gen_expr(value);
        let offset = self.get_var_offset(name);
        // For strings, also allocate space for length
        self.stack_offset -= 8; // extra space for length
        self.emit(&format!("    mov QWORD PTR [rbp + {}], rax", offset));
        self.emit(&format!("    mov QWORD PTR [rbp + {}], rdx", offset - 8));
    }

    fn emit_data_section(&mut self) {
        self.output.push_str("\n.data\n");

        // String literals - clone to avoid borrow issues
        let strings = self.string_literals.clone();
        for (i, s) in strings.iter().enumerate() {
            self.output.push_str(&format!("_str_{}:\n", i));
            let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
            self.output
                .push_str(&format!("    .ascii \"{}\"\n", escaped));
        }

        // DATA table - always define it (even if empty) to avoid linker errors
        self.output.push_str("_data_table:\n");
        let data_items = self.data_items.clone();
        for item in &data_items {
            match item {
                Literal::Integer(n) => {
                    self.output.push_str("    .quad 0  # type int\n");
                    self.output.push_str(&format!("    .quad {}\n", n));
                }
                Literal::Float(f) => {
                    self.output.push_str("    .quad 1  # type float\n");
                    self.output
                        .push_str(&format!("    .quad 0x{:X}\n", f.to_bits()));
                }
                Literal::String(s) => {
                    let idx = self.string_literals.len();
                    self.string_literals.push(s.clone());
                    self.output.push_str("    .quad 2  # type string\n");
                    self.output.push_str(&format!("    .quad _str_{}\n", idx));
                }
            }
        }
        self.output
            .push_str(&format!("_data_count: .quad {}\n", data_items.len()));

        // DATA pointer
        self.emit("_data_ptr: .quad 0");

        // GOSUB return stack pointer
        if self.gosub_used {
            self.emit("_gosub_sp: .quad 0");
        }

        self.emit("");
        self.emit(".bss");
        // GOSUB stack (if needed)
        if self.gosub_used {
            self.emit("_gosub_stack: .skip 8192  # GOSUB return stack");
        }
    }
}
