# BASIC-to-x86_64 Compiler Design Specification

Target: 1980s Tandy Color BASIC / GW-BASIC / QuickBASIC era dialects, compiled to Linux x86-64
Implementation language: Rust

---

## 1. Project Goals and Non-Goals

### 1.1 Goals

- Implement a **native code compiler** for a classic BASIC dialect:
  - Syntax and semantics broadly similar to **QuickBASIC** and **GW-BASIC**.
  - Compatible with the style of BASIC used on **Tandy** machines (Color BASIC / Extended Color BASIC), but without hardware-specific graphics/sound.
- Target platform: **Linux on x86-64**, using the **System V AMD64 ABI** and ELF binaries.
- Produce:
  - Either **standalone executables** (`a.out`-style) or
  - **.o object files** suitable for linking with `cc` or `ld`.
- Implement a **clean, well-typed internal representation** so the compiler can evolve towards more of QuickBASIC over time.

### 1.2 Non-Goals (Initial MVP)

- No support for Tandy-specific **graphics and sound** (e.g., `PSET`, `CIRCLE`, `PLAY`, `SOUND`, `SCREEN`, `COLOR`) beyond optional stub runtime calls.
- No support for **random-access or binary file I/O** (`OPEN ... FOR RANDOM`, `GET`, `PUT`) in the MVP; focus on console I/O.
- No support for **ON ERROR**, structured exception handling, or full QuickBASIC-style error trapping.
- No multi-module projects or QuickBASIC-style separate compilation units; **single-source-file compilation only**.
- No IDE; just a **command-line compiler** that reads `.BAS` and writes `.o` / executable.

---

## 2. Target Environment

### 2.1 OS and ABI

- OS: **Linux** (modern distributions).
- Architecture: **x86-64**.
- ABI: **System V AMD64** calling convention.
- Binary format: **ELF64**.

Compiler output options:

1. **Executable mode** (default):
   - Emit ELF64 object with a `main` symbol and link via system linker (shelling out to `cc` or `ld`) to produce an executable.

2. **Object-only mode** (`--emit=obj`):
   - Emit a `.o` file implementing `main` and any helper runtime calls, allowing manual linking.

### 2.2 Runtime Dependencies

- Minimal **runtime library** (written in Rust, compiled to a static `.a`):
  - String handling (allocation, concatenation, slicing).
  - Array allocation and bounds checking.
  - Console I/O wrappers (`BASIC_PRINT`, `BASIC_INPUT_LINE`, etc.).
  - Pseudorandom number generator for `RND`.
- Linker will combine:
  - BASIC-generated `.o` file(s).
  - BASIC runtime library.
  - System C runtime (`libc`) for `write`, `read`, etc., or direct syscalls in a later phase.

---

## 3. Source Language: BASIC Dialect Overview

The compiler supports a **subset/superset hybrid** of GW-BASIC and QuickBASIC, with the following design rules:

- Keep **line-number support** for nostalgia and portability.
- Also allow **label-based** structured control flow similar to QuickBASIC.
- Prefer **structured control flow** over heavy `GOTO` usage internally.
- Support the most commonly used numeric, string, and control-flow features.

### 3.1 Lexical Structure

- **Character set**: ASCII; input treated as UTF-8 but only ASCII tokens guaranteed.
- **Whitespace**: Spaces and tabs separate tokens; end-of-line terminates statements unless `:` separator is used.
- **Comments**:
  - `REM` followed by the rest of the line.
  - Single quote (`'`) followed by the rest of the line.
- **Identifiers**:
  - Start with a letter, followed by letters, digits, or `_`.
  - Optional type suffix characters: `%`, `&`, `!`, `#`, `$`.
- **Literals**:
  - Integer: `123`, `-42`, hex (`&HFF`), octal (`&O377`) — minimal support; hex is enough for MVP.
  - Floating-point: `1.0`, `.5`, `1E+3`, `3.14D+0`.
  - String: `"hello"`, with `""` inside representing a `"` character.

### 3.2 Data Types

Minimal but expressive set, mapped to native types:

- `INTEGER` (suffix `%`): 32-bit signed (`i32`).
- `LONG` (suffix `&`): 64-bit signed (`i64`).
- `SINGLE` (suffix `!`): 32-bit float (`f32`).
- `DOUBLE` (suffix `#`): 64-bit float (`f64`).
- `STRING` (suffix `$`): dynamically allocated UTF-8/ASCII (runtime-managed).

Rules:

- **Default type** for un-suffixed variables: `SINGLE` (`f32`), as per many BASICs.
- **DEFxxx** family (`DEFINT`, `DEFSTR`, etc.):
  - **MVP**: not supported; treated as syntax errors or ignored with a warning.
  - Future extension: can be wired into type inference.
- Booleans:
  - Represented as numeric: `0` = false, non-zero = true.

### 3.3 Operators

Arithmetic:

- Unary: `+`, `-`.
- Binary: `+`, `-`, `*`, `/`, integer division `\`, exponentiation `^`.

Relational:

- `=`, `<>`, `<`, `>`, `<=`, `>=`.

Logical (numeric):

- `AND`, `OR`, `NOT`, `XOR`.
- `EQV`, `IMP` may be omitted in MVP.

String-specific:

- `+` for concatenation.

Operator precedence roughly follows QuickBASIC; the parser will implement a precedence table matching that era.

### 3.4 Program Structure

- Program consists of **a single module** of:
  - Global declarations.
  - SUBs and FUNCTIONs.
  - Top-level executable statements.
- **Line numbers**:
  - Optional but supported for authenticity and GOTO/GOSUB targets.
  - Model: `10 PRINT "HELLO"`.
  - When present, line numbers become labels in the symbol table.
- **Labels** (QuickBASIC style):
  - Optional support for `LabelName:` form.
  - May be used as GOTO/GOSUB targets; map to internal block labels.

---

## 4. Supported Language Elements (MVP)

### 4.1 Declarations and Variables

Supported:

- Implicit variable declaration on first use.
- Explicit dimensioning:
  - `DIM a(10)`
  - `DIM a(10, 20)` up to **2 dimensions** in MVP.

- Type suffixes on variables and functions: `%`, `&`, `!`, `#`, `$`.

Not supported (MVP):

- `REDIM`.
- `STATIC` on variable declarations.
- `COMMON`, `SHARED` across modules (single module only).

### 4.2 Control Flow Statements

Supported control flow (core subset):

- `IF` / `THEN` / `ELSE` single-line:
  - `IF expr THEN statement [ELSE statement]`
- `IF` block form (QuickBASIC style):
  - `IF expr THEN`
  - `    ...`
  - `[ELSEIF expr THEN ...]`
  - `[ELSE ...]`
  - `END IF`

- `FOR` / `NEXT` loops:
  - `FOR i = start TO end [STEP step]`
  - `    ...`
  - `NEXT [i]`

- `WHILE` / `WEND` loops:
  - `WHILE expr`
  - `    ...`
  - `WEND`

- `DO` loops (support at least one form):
  - `DO WHILE expr ... LOOP`
  - `DO ... LOOP WHILE expr`

- `GOTO` label or line number.
- `GOSUB` line number / label, paired with `RETURN`.
- `ON expr GOTO` label list.
- `STOP` (terminate program).
- `END` (terminate program).

Not supported (MVP):

- `SELECT CASE` (mark as future enhancement).
- `ON expr GOSUB`.
- `RESUME`, `RESUME NEXT` (error handling-related).

### 4.3 Subroutines and Functions

Supported:

- `SUB` and `END SUB`.
- `FUNCTION` and `END FUNCTION`.
- Parameters **passed by value only** in MVP:
  - Syntax may allow `BYVAL`/`BYREF`, but:
    - `BYVAL` implemented.
    - `BYREF` either rejected or treated as BYVAL with a warning.

- Function return via:
  - Assigning to the function name: `MyFunc = 42` inside `FUNCTION MyFunc() ...`.

- Nesting:
  - No nested procedures; SUB/FUNCTION scopes are flat.

Not supported (MVP):

- `DECLARE` for forward declarations; the compiler will allow calls before textual definition (single pass over symbol table).
- Separate compilation units.

### 4.4 Console I/O

Supported:

- `PRINT`:
  - `PRINT expr [, expr] ...` or `PRINT expr ; expr`.
  - `PRINT` with no args prints a newline.
  - `;` keeps cursor on same line; `,` moves to next zone (minimal: treat as single tab or fixed spacing).

- `INPUT`:
  - `INPUT var` reads a line and parses to variable type.
  - `INPUT "Prompt", var` form.

- `LINE INPUT`:
  - `LINE INPUT var$` reads an entire line as string without parsing.

- `CLS` (optional, implemented via printing ANSI clear-screen sequence).

Not supported (MVP):

- `LOCATE`, `PRINT USING`, formatted output sequences.
- `WIDTH`, `COLOR`.
- Full terminal control; only simple text I/O guaranteed.

### 4.5 File I/O (MVP decision)

Option A (recommended for first release): **No file I/O**.

- All file statements (`OPEN`, `CLOSE`, `INPUT#`, `PRINT#`, `LINE INPUT#`, `EOF`, etc.) are **not supported** and trigger a compile-time error.

Option B (minimal text file I/O) for an early second milestone:

- `OPEN filename$ FOR INPUT AS #n`
- `OPEN filename$ FOR OUTPUT AS #n`
- `CLOSE [#n]`
- `LINE INPUT #n, var$`
- `PRINT #n, expr` (append newline)

These map to C `fopen`, `fgets`, `fprintf`, etc., in the runtime library.

### 4.6 Data Statements

Supported (optional, low-complexity, high nostalgia value):

- `DATA` lines with comma-separated literals.
- `READ` var-list.
- `RESTORE [line]`.

Implementation strategy:

- During compilation, collect all `DATA` literals into a constant data table.
- At runtime, `READ` pulls from a global pointer into this table.

### 4.7 Built-in Functions

Numeric:

- `ABS(x)`
- `INT(x)`
- `FIX(x)` (same as INT for positive values; slight semantic difference for negatives – can match QuickBASIC later).
- `SQR(x)`
- `SIN(x)`, `COS(x)`, `TAN(x)`
- `ATN(x)`
- `EXP(x)`
- `LOG(x)`
- `SGN(x)`
- `RND[(x)]` (simple PRNG; seed semantics can be approximated).

String:

- `LEN(s$)`
- `LEFT$(s$, n)`
- `RIGHT$(s$, n)`
- `MID$(s$, start [, length])`
- `INSTR([start,] s$, substring$)`
- `ASC(s$)`
- `CHR$(n)`
- `VAL(s$)`
- `STR$(x)`

Type conversion:

- `CINT`, `CLNG`, `CSNG`, `CDBL`

Time/date (optional, can be added as runtime wrappers):

- `TIMER` (seconds since midnight).

Not supported (MVP):

- `ENVIRON`, `COMMAND$`, OS interaction.
- Hardware-specific functions.

### 4.8 Statements Explicitly Unsupported / Mocked

- **Error handling**:
  - `ON ERROR`, `RESUME` – not supported.

- **Graphics & sound**:
  - `SCREEN`, `PSET`, `LINE`, `CIRCLE`, `PAINT`, `PALETTE`, `SOUND`, `PLAY`, etc.
  - Either syntax error or compiled to no-op stubs (design-time choice).

- **Multi-tasking / event-driven** features (like QB event traps) – out of scope.

---

## 5. Semantic Model

### 5.1 Scoping and Lifetime

- Variables are **global by default**.
- SUB/FUNCTION local variables:
  - Each procedure has a local frame.
  - Parameters and any explicitly `DIM`med variables inside the procedure are local.
  - Global variables remain accessible unless overshadowed by local names.

- Lifetime:
  - Globals live for entire program duration.
  - Locals live for the duration of the procedure invocation.

### 5.2 Parameter Passing

- MVP: **pass-by-value** only.
- Implementation:
  - Scalars passed in registers/stack per System V AMD64 ABI.
  - Strings passed as a pointer + length struct (runtime-managed), copied or reference-counted by the runtime.

### 5.3 Type Checking and Coercion

- Implicit numeric widening:
  - `INTEGER` → `LONG` → `SINGLE` → `DOUBLE` as needed.
- String/number conversions:
  - `"123" + 5` is an error; require explicit `VAL` or `STR$` usage.
- Comparisons:
  - Numeric vs numeric: result numeric boolean (0/−1 or 0/1 – choose and document; 0/1 recommended).
  - String vs string: lexicographic.

### 5.4 Control Flow Constraints

- `GOTO`/`GOSUB` may target:
  - Any label/line in the same procedure or global scope.
- Jumps from inside a SUB/FUNCTION to outside (or vice versa) are **compile-time errors**.
- `FOR` loops:
  - Loop variable type is inferred from its suffix.
  - Step value may be negative; termination check based on sign of step.

---

## 6. Compiler Architecture (Rust)

### 6.1 High-Level Pipeline

1. **Lexing**
   - Tokenize source into identifiers, keywords, literals, operators, punctuation, line breaks, and line numbers.
2. **Parsing**
   - Convert tokens into an **AST** representing modules, procedures, statements, and expressions.
3. **Semantic Analysis**
   - Build symbol tables for globals and procedures.
   - Resolve labels and line numbers.
   - Perform type checking and insert coercions.
4. **IR Generation**
   - Translate AST into a simple, structured **intermediate representation**.
5. **Optimization (optional)**
   - Constant folding, dead code elimination for unreachable branches.
6. **Code Generation**
   - Lower IR to x86-64 assembly or machine code following SysV ABI.
7. **Assembly & Linking**
   - Either:
     - Emit assembly and call `as` / `cc`.
     - Or directly emit ELF object using a crate like `object` (implementation detail).

### 6.2 Lexer Design

- Recognizes keywords case-insensitively (e.g., `PRINT`, `print`, `PrInT`).
- Distinguishes identifiers from keywords via keyword table.
- Treats end-of-line as a statement boundary token.
- Recognizes line numbers at the start of a line as a special token `LINE_NUM(n)`.

### 6.3 Parser Design

- Statement-level grammar organizes by:
  - `Line ::= [LineNumber] StatementList [':' StatementList]*`.
- Expression parser uses **precedence-based (Pratt) parsing** to handle BASIC operator precedence.
- Statements cover:
  - Assignment
  - Control flow (IF/FOR/WHILE/DO/GOTO/GOSUB/RETURN)
  - Procedure declarations
  - I/O and DATA/READ/RESTORE

The AST represents:

- Program
  - List of Procedures (including an implicit `__MAIN` for top-level code).
- Procedure
  - Name, parameters, return type (if FUNCTION), body statements, locals.
- Statements
  - Assign, If, For, While, DoLoop, Goto, Gosub, Return, Call, Print, Input, LineInput, Data, Read, Restore, Stop, End.
- Expressions
  - Literals, variable references, procedure calls, unary/binary operations, function calls.

### 6.4 Intermediate Representation (IR)

IR goals:

- Simple enough to implement quickly.
- Structured, block-based control flow to ease register allocation.

Proposed IR model:

- **Procedure** = sequence of **basic blocks**.
- Block = list of instructions ending with a terminator (jump, conditional, return).

Instruction kinds (examples):

- `LoadVar`, `StoreVar` (abstract variable access).
- `LoadConst` (numeric or string handle).
- Arithmetic/logical ops: `Add`, `Sub`, `Mul`, `Div`, `Cmp`, etc.
- Calls: `CallRuntime`, `CallUser`.
- `Branch`, `Jump`, `Return`.

At codegen time, variables are mapped to stack slots or registers according to liveness and ABI rules.

### 6.5 Code Generation to x86-64

- Adopt **System V AMD64 ABI**:
  - Integer/pointer args: `RDI`, `RSI`, `RDX`, `RCX`, `R8`, `R9`.
  - Floating-point args: `XMM0–XMM7`.
  - Return values: `RAX` or `XMM0`.
- Procedure prologues/epilogues:
  - Establish stack frame, save callee-saved registers, allocate space for locals.
- Strings and arrays:
  - Represented as pointers to heap-managed structures.
  - Runtime functions handle allocation/deallocation; generated code just passes pointers.

Compiler back-end strategy (MVP):

- Emit **textual assembly** and shell out to `as` and `ld`/`cc`.
- Later optimization: directly emit ELF using an object file library.

---

## 7. Runtime Library Design

### 7.1 Memory Model

- All heap allocations (strings, arrays) go through a small runtime allocator:
  - May wrap `malloc`/`free` or use Rust’s allocator depending on linking model.
- Strings:
  - Struct with `ptr`, `len`, `capacity`.
  - Semantics: copy-on-assignment for MVP (no reference counting in first iteration).

### 7.2 Console I/O API

Expose a minimal C-ABI-compatible interface for the compiled code to call:

- `void BASIC_PrintString(const char* ptr, size_t len);`
- `void BASIC_PrintNewline(void);`
- `void BASIC_InputLine(char** out_ptr, size_t* out_len);`

Compiler maps `PRINT`/`INPUT`/`LINE INPUT` to calls to these functions.

### 7.3 Numeric and String Functions

Runtime exports:

- `double BASIC_Sqr(double x);`
- `double BASIC_Sin(double x);` etc.
- String operations: `BASIC_StrConcat`, `BASIC_StrLeft`, `BASIC_StrRight`, `BASIC_StrMid`, `BASIC_StrLen`, `BASIC_StrInstr`.

These can be implemented in Rust and exposed via `extern "C"`.

### 7.4 RND Implementation

- Maintain a global RNG state (e.g., xorshift).
- `RND(0)` returns next value; `RND(-n)` resets seed; `RND(1)` etc. approximates QuickBASIC semantics.

---

## 8. Error Handling and Diagnostics

### 8.1 Compile-Time Errors

- Lexical errors (invalid characters, malformed numbers).
- Syntax errors (unexpected token, missing `END IF`, etc.).
- Semantic errors:
  - Undefined labels/line numbers.
  - Type mismatches not allowed by coercion rules.
  - Illegal GOTO from inside a procedure to outside.
  - Use of unsupported statements/features (e.g., `ON ERROR`).

### 8.2 Runtime Errors

- Basic checks:
  - Array index out of bounds.
  - Division by zero.
  - Null/invalid string pointer.

Behavior:

- Print a BASIC-style error message and terminate with non-zero exit status.
- Optionally print the line number or label name where the error occurred (maintain a PC-to-source map).

---

## 9. Testing and Compatibility Strategy

### 9.1 Test Categories

- **Unit tests** on lexer, parser, and semantic analyzer using small snippets.
- **Integration tests** compiling BASIC programs and running them:
  - Console programs using PRINT/INPUT.
  - Control flow heavy examples (nested IFs, loops).
  - Procedural tests (SUB/FUNCTION calls, recursion).
- **Regression tests**: real programs ported from sample Tandy/QuickBASIC listings (with graphics/sound stripped).

### 9.2 Compatibility Notes

- Document any known deviations from QuickBASIC/GW-BASIC semantics (e.g., boolean representation, `RND` details).
- Provide a “dialect notes” section in user docs so users can see what’s supported and what’s not.

---

## 10. Roadmap for Future Extensions

1. Add `SELECT CASE`.
2. Add file I/O (`OPEN`, `INPUT#`, `PRINT#`).
3. Improve string performance (reference counting or copy-on-write).
4. Add `BYREF` parameter passing with alias analysis.
5. Add basic graphics via SDL or terminal graphics, mapped from `SCREEN`, `PSET`, etc.
6. Add optimization passes (constant folding beyond trivial, common subexpression elimination).

---

## 11. Summary of Supported Language Elements (MVP)

**Statements** (MVP):

- `REM`, `'` (comments)
- `DIM`
- `IF` / `THEN` / `ELSE` / `ELSEIF` / `END IF`
- `FOR` / `NEXT`
- `WHILE` / `WEND`
- `DO` / `LOOP` (at least WHILE/UNTIL forms)
- `GOTO`
- `GOSUB` / `RETURN`
- `ON expr GOTO`
- `SUB` / `END SUB`
- `FUNCTION` / `END FUNCTION`
- `PRINT`
- `INPUT`
- `LINE INPUT`
- `CLS` (optional, via ANSI)
- `DATA`, `READ`, `RESTORE` (optional but recommended)
- `STOP`
- `END`

**Functions** (MVP):

- Numeric: `ABS`, `INT`, `FIX`, `SQR`, `SIN`, `COS`, `TAN`, `ATN`, `EXP`, `LOG`, `SGN`, `RND`.
- String: `LEN`, `LEFT$`, `RIGHT$`, `MID$`, `INSTR`, `ASC`, `CHR$`, `VAL`, `STR$`.
- Conversion: `CINT`, `CLNG`, `CSNG`, `CDBL`.

**Types** (MVP):

- `INTEGER` (`%`), `LONG` (`&`), `SINGLE` (`!`), `DOUBLE` (`#`), `STRING` (`$`).

All other BASIC elements are either:

- **Unsupported** → compile-time error, or
- **Planned** → mentioned in roadmap for later implementation.

---

This specification is intentionally conservative in scope, while laying out a compiler and runtime architecture that can be extended later toward full QuickBASIC/Tandy compatibility.

