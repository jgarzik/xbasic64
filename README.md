# xbasic64

A BASIC-to-x86_64 native code compiler.

## Overview

xbasic64 compiles 1980s-era BASIC dialects (Tandy Color BASIC, GW-BASIC, QuickBASIC) directly to native x86-64 executables. No interpreter, no bytecode—just fast native binaries.

**Why xbasic64?**

- **Nostalgia**: Write and run classic BASIC programs on modern hardware
- **Education**: Learn compiler design with a simple, readable Rust codebase
- **Simplicity**: Direct AST-to-assembly compilation with no intermediate representation

## Features

- Classic BASIC syntax with line numbers or structured code
- Numeric types: Integer, Long, Single, Double (with type suffixes)
- String handling with standard functions (LEFT$, MID$, etc.)
- Control flow: IF/THEN/ELSE, FOR/NEXT, WHILE/WEND, DO/LOOP, SELECT CASE
- Procedures: SUB and FUNCTION with recursion support
- File I/O: Sequential file reading and writing
- DATA/READ/RESTORE for inline data
- Full expression support with proper operator precedence

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

## Architecture

The compiler uses a three-stage pipeline:

```
Source → Lexer → Parser → Code Generator → Assembly → Executable
              (tokens)   (AST)          (x86-64)
```

1. **Lexer** - Tokenizes BASIC source (case-insensitive keywords, line numbers, type suffixes)
2. **Parser** - Recursive descent parser producing an AST
3. **Code Generator** - Direct AST-to-x86-64 assembly translation

The runtime library provides I/O, string operations, and math functions as hand-written x86-64 assembly using libc for portability.

Key design choices:
- No IR—direct AST to assembly for simplicity
- System V AMD64 ABI for libc interoperability
- GW-BASIC type semantics (division always returns Double)

## Requirements

- Rust toolchain
- System assembler (`as`)
- System C compiler/linker (`cc`) with libc

## Platforms

- macOS (x86-64, ARM64 via Rosetta)
- Linux (x86-64)

## License

[MIT](LICENSE)
