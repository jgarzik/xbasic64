# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

basic-rs is a BASIC-to-x86_64 native code compiler written in Rust. It targets 1980s-era BASIC dialects (Tandy Color BASIC, GW-BASIC, QuickBASIC) and compiles to Linux x86-64 ELF binaries using the System V AMD64 ABI.

## Build Commands

```bash
cargo build           # Build the compiler
cargo run             # Run the compiler
cargo test            # Run all tests
cargo test test_name  # Run a specific test
cargo clippy          # Run linter
cargo fmt             # Format code
```

## Architecture

The compiler follows a traditional multi-pass pipeline:

1. **Lexer** - Tokenizes BASIC source (case-insensitive keywords, line numbers, type suffixes like `%`, `$`, `#`)
2. **Parser** - Builds AST using Pratt parsing for expressions; handles line-number and label-based control flow
3. **Semantic Analysis** - Symbol tables, type checking, label/line-number resolution, implicit type coercion
4. **IR Generation** - Translates AST to basic-block-based intermediate representation
5. **Code Generation** - Emits x86-64 assembly following System V AMD64 ABI
6. **Assembly & Linking** - Shells out to `as`/`cc` to produce ELF executables or object files

### Runtime Library

A minimal runtime library (Rust with C ABI) provides:
- String handling (allocation, concatenation, slicing)
- Console I/O (`BASIC_PrintString`, `BASIC_InputLine`, etc.)
- Math functions (`BASIC_Sqr`, `BASIC_Sin`, etc.)
- PRNG for `RND`

### Type System

- `INTEGER` (`%`): i32
- `LONG` (`&`): i64
- `SINGLE` (`!`): f32 (default for unsuffixed variables)
- `DOUBLE` (`#`): f64
- `STRING` (`$`): heap-allocated, runtime-managed

### Key Design Decisions

- Variables are global by default; SUB/FUNCTION locals override
- Parameters are pass-by-value only (MVP)
- GOTO/GOSUB cannot cross procedure boundaries
- Booleans represented as numeric (0 = false, non-zero = true)
- String/number implicit conversion is an error; use `VAL`/`STR$`
