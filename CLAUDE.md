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
- **runtime.rs** - Hand-written x86-64 assembly runtime library (I/O, strings, math) using libc
- **rtsig.rs** - Signature of every runtime helper generated code calls, and
  whether it can raise an error. Every call goes through `CodeGen::call_rt`,
  which looks it up; a test derives `fallible` from the assembly's control flow
- **main.rs** - CLI driver: reads source, runs pipeline, shells out to `as` and `cc` for linking

### Test Structure (`tests/`)

Integration tests organized by feature area:
- `common/mod.rs` - Test harness. `compile_and_run()` for the usual case;
  `compile_only()` for "this must be rejected, with this message"; and
  `compile_and_run_raw()` for "printed this, then aborted with this exit code"
- `docs/mod.rs` - Compiles every ```basic example in LANGREF.md and README.md
- `megatest/` - One large program (`mega.bas`) exercising most of the language
  at once, so that features are tested coexisting rather than in isolation. It
  is its own assertion harness: a regression names itself
- Feature modules: `arithmetic/`, `arrays/`, `control/`, `data/`, `file_io/`, `input/`, `math/`, `print/`, `procedures/`, `strings/`, `types/`, `variables/`

### Key Design Decisions

- **No IR**: AST compiles directly to assembly for simplicity
- **System V AMD64 ABI**: Enables libc interoperability for I/O and math
- **GW-BASIC semantics**: Division (`/`) always returns Double; integer division uses `\`
- **Default type is Double**: Unsuffixed numeric variables are `#` (Double), not Single
- **Boolean -1/0**: Comparisons return -1 (true) or 0 (false) for bitwise compatibility
- **Scope is QuickBASIC's**: module-level storage is static (.bss, zeroed),
  but a procedure reaches a module name only when it is shared (`DIM SHARED`,
  or `SHARED` in the procedure; a DEF FN sees all). Its other names are frame
  slots zeroed on entry, unless STATIC, which gives them .bss storage of their
  own. `Symbols::shares` and `Symbols::is_static` decide, in one place each
- **Uniform word addressing**: every variable, string (pointer + length), array
  descriptor and record field is a sequence of 8-byte words reached through `Loc`
- **Runtime checks on by default**: `--unsafe` removes them
- **Two runtimes in lockstep**: `runtime/sysv/` and `runtime/win64-native/` export
  the same `.globl` names; a new helper must be added to both
- **Two assemblers, not just two runtimes**: `main.rs` assembles with GNU `as`
  on Linux and `clang -c` on Windows, so the Win64 tree is the one built by the
  assembler no test here runs. They disagree about `.equ len, . - label`: GNU
  `as` folds it to an immediate, and an assembler that cannot prove the symbol
  absolute assembles `mov reg, len` as a *load from that address* instead.
  `CLS` did that on every Windows run. Bracket data with an `_end` label and
  subtract at run time
- **A runtime helper takes at most four arguments**: Win64 passes only four in
  registers, so `arg_reg(4)` panics there while System V accepts six. A helper
  that needs a fifth is invisible on Linux and breaks every Windows compile;
  fold an argument away instead (see `_rt_file_open_random`)
- **Unsupported GW-BASIC names are refused, not ignored**: an unrecognised name
  is otherwise just a new variable, so `UNSUPPORTED` in `sema.rs` names the
  keywords this compiler does not provide and why
- **String builtins returning a static buffer must copy**: two calls in one
  expression would otherwise alias, which is why `_rt_str`, `_rt_chr`, `_rt_hex`,
  `_rt_oct` and `_rt_mk` end in `_rt_strdup`
- **No jumps to code addresses**: GOSUB pushes a return site *number* and
  RETURN dispatches through `_gosub_sites`; ON GOTO/GOSUB and the ON ERROR
  handler (`_err_handler` holds a number) go through tables too. Every indirect
  transfer is a bounded table jump, the shape Cranelift's `br_table` has
- **ON ERROR is error-status propagation**: a trapped `_rt_error` sets
  `_err_pending` and returns. A helper that raised leaves through its own
  epilogue; generated code tests the flag after each fallible call and each
  procedure call, leaving a procedure through its epilogue and main through
  `_err_dispatch`, which resets rsp and jumps to the handler. Main saves the
  callee-saved registers itself when it traps, and nothing in main keeps a value
  in one across a statement then (FOR promotion is off). Only programs with
  ON ERROR pay for any of this
- **Every helper follows its platform's standard ABI**: a double goes where the
  ABI puts it -- by position on Win64, so `(file, value)` passes the value in
  xmm1 there; `Abi::float_arg_reg` knows the rule. `rtsig.rs` lists one
  signature that serves both ABIs, so a compiler that follows the ABI can call
  any helper

## Language Reference

See [LANGREF.md](LANGREF.md) for the supported BASIC dialect. Key points:
- Types: INTEGER (`%`), LONG (`&`), SINGLE (`!`), DOUBLE (`#`), STRING (`$`)
- Control flow: IF/THEN/ELSE, FOR/NEXT, WHILE/WEND, DO/LOOP, SELECT CASE, GOTO/GOSUB
- Procedures: SUB and FUNCTION with recursion (parameters are by-value only)
- File I/O: OPEN FOR INPUT/OUTPUT/APPEND, PRINT #, INPUT #, LINE INPUT #, CLOSE
- String indexing is 1-based (MID$, INSTR); array indexing is 0-based
