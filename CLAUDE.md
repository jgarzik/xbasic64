# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

basic64 is a BASIC-to-x86_64 native code compiler written in Rust. It targets 1980s-era BASIC dialects (Tandy Color BASIC, GW-BASIC, QuickBASIC) and compiles to native executables using the System V AMD64 ABI.

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
basic64 program.bas              # Compile to executable
basic64 program.bas -o output    # Specify output name
basic64 -S program.bas           # Emit assembly only
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

All numeric values are stored as 64-bit floats (doubles) internally. Type suffixes are parsed but effectively ignored:
- `%` (integer), `&` (long), `!` (single), `#` (double) - all become f64
- `$` (string) - heap-allocated, managed as (ptr, len) pairs

### Key Design Decisions

- **No IR** - Direct AST to assembly for simplicity
- **All numerics as f64** - Simplified type system
- **Stack-based evaluation** - Expressions use x87 FPU stack
- **Variables are global** - No local scoping except procedure parameters
- **System V AMD64 ABI** - Standard calling convention for libc interop

## Platforms

- macOS (x86-64, ARM64 via Rosetta)
- Linux (x86-64)

## Requirements

- Rust toolchain
- System assembler (`as`)
- System C compiler/linker (`cc`) with libc
