# xbasic64

A BASIC-to-x86_64 native code compiler.

## Overview

xbasic64 compiles 1980s-era BASIC dialects (Tandy Color BASIC, GW-BASIC, QuickBASIC) directly to native x86-64 executables. No interpreter, no bytecode—just fast native binaries.

**Why xbasic64?**

- **Nostalgia**: Write and run classic BASIC programs on modern hardware
- **Education**: Learn compiler design with a simple, readable Rust codebase
- **Simplicity**: Direct AST-to-assembly compilation with no intermediate representation

## Features

- Classic BASIC syntax with line numbers, named labels, or structured code
- Numeric types: Integer, Long, Single, Double (with type suffixes)
- Strings with the standard function set (`LEFT$`, `MID$`, `UCASE$`, `INSTR`, ...),
  including `MID$` as an assignment target
- Control flow: `IF`/`THEN`/`ELSE`, `FOR`/`NEXT`, `WHILE`/`WEND`, `DO`/`LOOP`,
  `SELECT CASE` with ranges, lists and `IS` comparisons, and `EXIT`
- Procedures: `SUB` and `FUNCTION` with recursion; `DEF FN` for one-liners
- Arrays with `REDIM`, `REDIM PRESERVE`, `OPTION BASE`, and `LBOUND`/`UBOUND`
- User-defined record types with `TYPE`, including nesting and arrays of records
- File I/O: sequential reading and writing, with `EOF` and `LOF`; random-access
  records with `FIELD`, `LSET`/`RSET`, `GET`/`PUT`, `LOCK` and the `MKI$`/`CVI`
  conversion family
- `DATA`/`READ`/`RESTORE` for inline data
- Formatted output with `PRINT USING`
- Runtime checks for out-of-range subscripts and division by zero, with
  `--unsafe` to remove them
- Diagnostics that name the file, line and problem rather than failing at link
  time, including a reason for every GW-BASIC keyword the compiler does not
  provide, so a program using one is refused rather than quietly misbehaving

## Quick Start

### Building

```bash
cargo build --release
```

### Usage

```bash
# Compile a BASIC program to executable
xbasic64 program.bas

# Specify output file
xbasic64 program.bas -o myprogram

# Emit assembly only (no linking)
xbasic64 -S program.bas

# Omit the runtime safety checks
xbasic64 --unsafe program.bas
```

### Example

```basic
' Fibonacci sequence
A = 0
B = 1
FOR I = 1 TO 10
    PRINT A
    C = A + B
    A = B
    B = C
NEXT I
```

Save as `fib.bas`, compile with `xbasic64 fib.bas`, and run `./fib`.

## Documentation

- **[Language Reference](LANGREF.md)** - Complete guide to the supported BASIC dialect
- **[Non-Goals](NONGOALS.md)** - The parts of GW-BASIC that will never be
  supported, and why

## Architecture

The compiler is a four-stage pipeline:

```
Source → Lexer → Parser → Semantic Analysis → Code Generator → Assembly → Executable
              (tokens)   (AST)              (symbols)        (x86-64)
```

1. **Lexer** - Tokenizes BASIC source (case-insensitive keywords, line numbers, type suffixes)
2. **Parser** - Recursive descent parser producing an AST
3. **Semantic analysis** - Resolves names, checks types and argument counts, and
   reports problems with a source line
4. **Code Generator** - Direct AST-to-x86-64 assembly translation

The runtime library provides I/O, string operations, and math functions as hand-written x86-64 assembly using libc for portability.

Key design choices:
- No IR—direct AST to assembly for simplicity
- System V AMD64 ABI for libc interoperability
- GW-BASIC type semantics (division always returns Double)

## Requirements

- Rust toolchain
- Linux: system assembler (`as`) and C compiler/linker (`cc`) with libc
- Windows: Clang (used to assemble) and the MSVC linker (`link.exe`)

## Platforms

- Linux (x86-64)
- Windows (x86-64)

## License

[MIT](LICENSE)
