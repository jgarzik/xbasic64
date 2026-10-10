# xbasic64

A compiler for classic BASIC: build a native executable, or compile into
memory and run at once. x86-64 and aarch64, on Linux, Windows and macOS.

## Overview

xbasic64 compiles 1980s-era BASIC dialects (Tandy Color BASIC, GW-BASIC,
QuickBASIC) to native machine code with [Cranelift](https://cranelift.dev).
It does so two ways, from one code generator:

- **Ahead of time**: `xbasic64 prog.bas` writes a standalone executable.
- **Just in time**: `xbasic64 run prog.bas` compiles the program into memory
  and runs it at once, with no executable and no linker -- the immediacy of
  an interpreter, at native speed.

Either way the whole program is compiled before it starts. Nothing is
interpreted, and there is no bytecode.

**Why xbasic64?**

- **Nostalgia**: Write and run classic BASIC programs on modern hardware
- **Education**: Learn compiler design with a simple, readable Rust codebase
- **Simplicity**: The AST is lowered straight to Cranelift IR, which emits native code

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
- Runtime checks for out-of-range subscripts, division by zero and arithmetic
  overflow, with `--unsafe` to remove them
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
# Run a BASIC program at once (compiled into memory, then run)
xbasic64 run program.bas

# Compile a BASIC program to an executable
xbasic64 program.bas

# Specify output file
xbasic64 program.bas -o myprogram

# Write the program's Cranelift IR to program.clif instead of compiling it
xbasic64 -S program.bas

# Omit the runtime safety checks (for either)
xbasic64 --unsafe program.bas
xbasic64 run --unsafe program.bas
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

Save as `fib.bas` and run it with `xbasic64 run fib.bas` -- or compile it
with `xbasic64 fib.bas` and run `./fib`.

## Documentation

- **[Language Reference](LANGREF.md)** - Complete guide to the supported BASIC dialect

## Architecture

The compiler is a four-stage pipeline, ending in one of two places:

```
Source → Lexer → Parser → Semantic Analysis → Code Generator ─┬→ Object → Executable
              (tokens)   (AST)              (symbols)        │   (xbasic64 prog.bas)
                                                  (Cranelift IR)
                                                             └→ Memory → Run now
                                                                 (xbasic64 run prog.bas)
```

1. **Lexer** - Tokenizes BASIC source (case-insensitive keywords, line numbers, type suffixes)
2. **Parser** - Recursive descent parser producing an AST
3. **Semantic analysis** - Resolves names, checks types and argument counts, and
   reports problems with a source line
4. **Code Generator** - Lowers the AST to Cranelift IR; Cranelift does instruction
   selection and register allocation and writes the object file -- or, for
   `xbasic64 run`, puts the code in memory (a JIT) and runs it in the compiler's
   own process. The whole program is compiled first either way, from the same
   lowering; nothing is interpreted

The runtime library (I/O, strings, number formatting, math) is a `no_std` Rust crate in `rt/` that calls the C library. It is built once, when the compiler is built, and linked beside each program; `xbasic64 run` uses the copy linked into the compiler.

Key design choices:
- No IR of its own: the AST is lowered directly to Cranelift's
- One code generator for both executables and the JIT, so the two cannot
  disagree; the test suite runs every program both ways
- A portable `no_std` Rust runtime over the C library
- GW-BASIC type semantics (division always returns Double)

## Requirements

- Rust 1.96 or later
- To build executables, the platform's linker: `cc` with the C library on
  Linux and macOS, the MSVC linker (`link.exe`) and its C runtime on Windows.
  `xbasic64 run` needs no linker

## Platforms

| OS      | Architecture    | Executables | `xbasic64 run` |
|---------|-----------------|-------------|----------------|
| Linux   | x86-64, aarch64 | yes         | yes            |
| Windows | x86-64          | yes         | yes            |
| macOS   | arm64           | yes         | yes            |

The compiler targets the machine it runs on.

## License

[MIT](LICENSE)
