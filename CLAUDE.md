# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Test Commands

```bash
# Build
cargo build --release

# Run all tests
cargo test

# Run a single test by name
cargo test test_name

# Run tests in a specific module
cargo test arithmetic::
cargo test control::
cargo test strings::

# Compile a BASIC program
cargo run -- program.bas           # Output: ./program
cargo run -- program.bas -o out    # Custom output name
cargo run -- -S program.bas        # Emit assembly only (no linking)
```

## Architecture

xbasic64 is a BASIC-to-x86_64 native code compiler with a direct AST-to-assembly pipeline (no IR):

```
Source → Lexer → Parser → CodeGen → Assembly → Executable
              (tokens)   (AST)    (x86-64)
```

### Source Files (`src/`)

- **lexer.rs** - Tokenizer handling case-insensitive keywords, line numbers, type suffixes (`%`, `&`, `!`, `#`, `$`), and BASIC literals
- **parser.rs** - Recursive descent parser producing an AST; handles expression precedence via Pratt parsing
- **codegen.rs** - Direct AST-to-x86-64 assembly translation using System V AMD64 ABI
- **runtime.rs** - Hand-written x86-64 assembly runtime library (I/O, strings, math) using libc
- **main.rs** - CLI driver: reads source, runs pipeline, shells out to `as` and `cc` for linking

### Test Structure (`tests/`)

Integration tests organized by feature area:
- `common/mod.rs` - Test harness with `compile_and_run()` helper that compiles BASIC source and captures output
- Feature modules: `arithmetic/`, `arrays/`, `control/`, `data/`, `file_io/`, `input/`, `math/`, `print/`, `procedures/`, `strings/`, `types/`, `variables/`

### Key Design Decisions

- **No IR**: AST compiles directly to assembly for simplicity
- **System V AMD64 ABI**: Enables libc interoperability for I/O and math
- **GW-BASIC semantics**: Division (`/`) always returns Double; integer division uses `\`
- **Default type is Double**: Unsuffixed numeric variables are `#` (Double), not Single
- **Boolean -1/0**: Comparisons return -1 (true) or 0 (false) for bitwise compatibility

## Language Reference

See [LANGREF.md](LANGREF.md) for the supported BASIC dialect. Key points:
- Types: INTEGER (`%`), LONG (`&`), SINGLE (`!`), DOUBLE (`#`), STRING (`$`)
- Control flow: IF/THEN/ELSE, FOR/NEXT, WHILE/WEND, DO/LOOP, SELECT CASE, GOTO/GOSUB
- Procedures: SUB and FUNCTION with recursion (parameters are by-value only)
- File I/O: OPEN FOR INPUT/OUTPUT/APPEND, PRINT #, INPUT #, LINE INPUT #, CLOSE
- String indexing is 1-based (MID$, INSTR); array indexing is 0-based
