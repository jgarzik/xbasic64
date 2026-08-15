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
Source → Lexer → Parser → Sema → CodeGen → Assembly → Executable
              (tokens)   (AST)  (symbols) (x86-64)
```

### Source Files (`src/`)

- **lexer.rs** - Tokenizer handling case-insensitive keywords, line numbers, type suffixes (`%`, `&`, `!`, `#`, `$`), and BASIC literals
- **parser.rs** - Recursive descent parser producing an AST; handles expression precedence via Pratt parsing
- **sema.rs** - Semantic analysis: builds the symbol table (procedures, arrays,
  records, constants, labels) and reports errors with a source line, so codegen
  never has to guess what a name means
- **codegen.rs** - Direct AST-to-x86-64 assembly translation using System V AMD64 ABI
- **using.rs** - `PRINT USING` format strings, parsed at compile time
- **runtime.rs** - Hand-written x86-64 assembly runtime library (I/O, strings, math) using libc
- **main.rs** - CLI driver: reads source, runs pipeline, shells out to `as` and `cc` for linking

### Test Structure (`tests/`)

Integration tests organized by feature area:
- `common/mod.rs` - Test harness. `compile_and_run()` for the usual case;
  `compile_only()` for "this must be rejected, with this message"; and
  `compile_and_run_raw()` for "printed this, then aborted with this exit code"
- `docs/mod.rs` - Compiles every ```basic example in LANGREF.md and README.md
- Feature modules: `arithmetic/`, `arrays/`, `control/`, `data/`, `file_io/`, `input/`, `math/`, `print/`, `procedures/`, `strings/`, `types/`, `variables/`

### Key Design Decisions

- **No IR**: AST compiles directly to assembly for simplicity
- **System V AMD64 ABI**: Enables libc interoperability for I/O and math
- **GW-BASIC semantics**: Division (`/`) always returns Double; integer division uses `\`
- **Default type is Double**: Unsuffixed numeric variables are `#` (Double), not Single
- **Boolean -1/0**: Comparisons return -1 (true) or 0 (false) for bitwise compatibility
- **Module-level storage is static**: globals live in `.bss`, so they are zeroed
  and reachable from procedures; procedure locals are stack slots zeroed on entry
- **Uniform word addressing**: every variable, string (pointer + length), array
  descriptor and record field is a sequence of 8-byte words reached through `Loc`
- **Runtime checks on by default**: `--unsafe` removes them
- **Two runtimes in lockstep**: `runtime/sysv/` and `runtime/win64-native/` export
  the same `.globl` names; a new helper must be added to both

## Language Reference

See [LANGREF.md](LANGREF.md) for the supported BASIC dialect. Key points:
- Types: INTEGER (`%`), LONG (`&`), SINGLE (`!`), DOUBLE (`#`), STRING (`$`)
- Control flow: IF/THEN/ELSE, FOR/NEXT, WHILE/WEND, DO/LOOP, SELECT CASE, GOTO/GOSUB
- Procedures: SUB and FUNCTION with recursion (parameters are by-value only)
- File I/O: OPEN FOR INPUT/OUTPUT/APPEND, PRINT #, INPUT #, LINE INPUT #, CLOSE
- String indexing is 1-based (MID$, INSTR); array indexing is 0-based
