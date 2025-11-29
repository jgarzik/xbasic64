# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

xbasic64 is a BASIC-to-x86_64 native code compiler written in Rust. It targets 1980s-era BASIC dialects (Tandy Color BASIC, GW-BASIC, QuickBASIC) and compiles to native executables using the System V AMD64 ABI.

## Build Commands

```bash
cargo build           # Build the compiler
cargo test            # Run all tests
cargo test test_name  # Run a specific test
cargo clippy          # Run linter
cargo fmt             # Format code
```

## Usage

```bash
xbasic64 program.bas              # Compile to executable
xbasic64 program.bas -o output    # Specify output name
xbasic64 -S program.bas           # Emit assembly only
```

## Architecture

The compiler uses a simple three-stage pipeline with no intermediate representation:

1. **Lexer** (`src/lexer.rs`) - Tokenizes BASIC source (case-insensitive keywords, line numbers, type suffixes)
2. **Parser** (`src/parser.rs`) - Recursive descent parser producing an AST
3. **Code Generator** (`src/codegen.rs`) - Direct AST-to-x86-64 assembly translation

Assembly output is passed to the system assembler (`as`) and linker (`cc`).

### Runtime Library

The runtime (`src/runtime/`) is hand-written x86-64 assembly using libc for portability:
- `data_defs.s` - Format strings, buffers
- `print.s` - PRINT functions
- `input.s` - INPUT functions
- `string.s` - String manipulation (LEFT$, MID$, etc.)
- `math.s` - Math functions (SQR, SIN, RND, etc.)
- `data.s` - DATA/READ support
- `file.s` - File I/O (OPEN, CLOSE, PRINT#, INPUT#)

### Type System

Following GW-BASIC/QuickBASIC conventions with type suffixes:
- `%` INTEGER - 16-bit signed (i16), stored in eax
- `&` LONG - 32-bit signed (i32), stored in eax
- `!` SINGLE - 32-bit float (f32), stored in xmm0
- `#` DOUBLE - 64-bit float (f64), stored in xmm0 - **DEFAULT for unsuffixed variables**
- `$` STRING - heap-allocated, managed as (ptr, len) pairs

Division follows GW-BASIC semantics:
- `/` always produces Double result
- `\` integer division produces Long result

Type coercion is automatic: Integer < Long < Single < Double. Comparisons return Long (-1 for true, 0 for false).

### Key Design Decisions

- **No IR** - Direct AST to assembly for simplicity
- **Type-aware codegen** - Integers in eax, floats in xmm0 with automatic coercion
- **Variables are global** - No local scoping except procedure parameters
- **System V AMD64 ABI** - Standard calling convention for libc interop

## Platforms

- macOS (x86-64, ARM64 via Rosetta)
- Linux (x86-64)

## Requirements

- Rust toolchain
- System assembler (`as`)
- System C compiler/linker (`cc`) with libc
