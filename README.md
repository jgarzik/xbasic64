# basic64

A BASIC-to-x86-64 compiler written in Rust, targeting early 1980s BASIC dialects (Tandy/GW-BASIC/QuickBASIC style).

## Features

### Supported Language Features

**Statements:**
- `PRINT` - with expressions, string literals, separators (`;`, `,`)
- `INPUT` / `LINE INPUT` - user input with optional prompts
- `LET` - variable assignment (optional keyword)
- `IF...THEN...ELSE...END IF` - conditionals (block and single-line)
- `SELECT CASE...CASE...CASE ELSE...END SELECT` - multi-way branching
- `FOR...TO...STEP...NEXT` - counted loops
- `WHILE...WEND` - conditional loops
- `DO...LOOP` - with `WHILE`/`UNTIL` conditions
- `GOTO` / `GOSUB` / `RETURN` - control flow
- `ON...GOTO` - computed jumps
- `DIM` - array declarations (1D, 2D, 3D, etc.)
- `SUB` / `FUNCTION` / `END SUB` / `END FUNCTION` - procedures
- `DATA` / `READ` / `RESTORE` - inline data
- `OPEN` / `CLOSE` / `PRINT #` / `INPUT #` - file I/O
- `CLS` - clear screen
- `END` / `STOP` - program termination

**Expressions:**
- Arithmetic: `+`, `-`, `*`, `/`, `\` (integer div), `MOD`, `^` (power)
- Comparison: `=`, `<>`, `<`, `>`, `<=`, `>=`
- Logical: `AND`, `OR`, `XOR`, `NOT`
- Parentheses for grouping

**Built-in Functions:**
- Math: `ABS`, `INT`, `FIX`, `SQR`, `SIN`, `COS`, `TAN`, `ATN`, `EXP`, `LOG`, `SGN`, `RND`
- String: `LEN`, `LEFT$`, `RIGHT$`, `MID$`, `INSTR`, `ASC`, `CHR$`, `VAL`, `STR$`
- Conversion: `CINT`, `CLNG`, `CSNG`, `CDBL`
- Other: `TIMER`

**Variables:**
- Type suffixes determine storage: `%` (16-bit int), `&` (32-bit int), `!` (32-bit float), `#` (64-bit float - default), `$` (string)
- Unsuffixed numeric variables default to Double (64-bit float)
- Line number labels

## Building

```bash
cargo build --release
```

## Usage

```bash
# Compile a BASIC program to executable
basic64 program.bas

# Specify output file
basic64 program.bas -o myprogram

# Emit assembly only (no linking)
basic64 -S program.bas
```

## Example Programs

**Hello World:**
```basic
PRINT "Hello, World!"
```

**Fibonacci:**
```basic
A = 0
B = 1
FOR I = 1 TO 10
    PRINT A
    C = A + B
    A = B
    B = C
NEXT I
```

**Factorial with Function:**
```basic
FUNCTION Factorial(N)
    IF N <= 1 THEN
        Factorial = 1
    ELSE
        Factorial = N * Factorial(N - 1)
    END IF
END FUNCTION

PRINT Factorial(5)
```

**User Input:**
```basic
INPUT "Enter your name: ", Name$
INPUT "Enter your age: ", Age
PRINT "Hello, "; Name$; "! You are"; Age; "years old."
```

**File I/O:**
```basic
' Write to a file
OPEN "data.txt" FOR OUTPUT AS #1
PRINT #1, "Hello, File!"
PRINT #1, 42
CLOSE #1

' Read from a file
OPEN "numbers.txt" FOR INPUT AS #1
INPUT #1, X
INPUT #1, Y
CLOSE #1
PRINT "Sum: "; X + Y
```

**Multi-dimensional Arrays:**
```basic
DIM Grid(9, 9)
FOR Row = 0 TO 9
    FOR Col = 0 TO 9
        Grid(Row, Col) = Row * 10 + Col
    NEXT Col
NEXT Row
PRINT Grid(5, 7)
```

## Architecture

The compiler uses a simple three-stage pipeline:

1. **Lexer** (`src/lexer.rs`) - Tokenizes BASIC source into tokens
2. **Parser** (`src/parser.rs`) - Recursive descent parser producing an AST
3. **Code Generator** (`src/codegen.rs`) - Direct AST-to-x86-64 assembly translation

The runtime library (`src/runtime/`) provides support functions for I/O, string manipulation, and other operations, implemented as x86-64 assembly using libc for cross-platform compatibility.

### Design Choices

- **No IR** - Direct AST to assembly for simplicity
- **Type-aware codegen** - Integers in eax, floats in xmm0, with automatic type coercion
- **GW-BASIC type semantics** - Division (`/`) always produces Double; integer division (`\`) produces Long
- **Strings as (ptr, len) pairs** - Not null-terminated internally
- **System V AMD64 ABI** - Standard calling convention for libc interop

## Requirements

- Rust toolchain (for building the compiler)
- System assembler (`as`)
- System C compiler/linker (`cc`) with libc

## Platforms

- macOS (x86-64, ARM64 via Rosetta)
- Linux (x86-64)

## Limitations

**Not Supported:**
- Graphics (SCREEN, PSET, LINE, CIRCLE, etc.)
- Sound (BEEP, SOUND, PLAY)
- PEEK/POKE/DEF SEG (no direct memory access)
- Error handling (ON ERROR, RESUME)
- DEF FN (use FUNCTION instead)

## License

MIT
