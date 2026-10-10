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
cargo run -- -S program.bas        # Write the program's Cranelift IR to program.clif (no linking)
```

## Architecture

xbasic64 is a BASIC-to-x86_64 native code compiler. The AST is lowered straight
to Cranelift IR, and Cranelift writes the object file:

```
Source → Lexer → Parser → Sema → CodeGen → Object → Executable
              (tokens)   (AST)  (symbols) (Cranelift)
```

### Source Files (`src/`)

- **lexer.rs** - Tokenizer handling case-insensitive keywords, line numbers, type suffixes (`%`, `&`, `!`, `#`, `$`), and BASIC literals
- **parser.rs** - Recursive descent parser producing an AST; handles expression precedence via Pratt parsing
- **sema.rs** - Semantic analysis: builds the symbol table (procedures, arrays,
  records, constants, labels) and reports errors with a source line, so codegen
  never has to guess what a name means
- **codegen/** - Lowers the AST to Cranelift IR with a `FunctionBuilder`, one
  function at a time (`FnGen`), into an `ObjectModule` (`CodeGen`): `storage.rs`
  (where names live), `ssa.rs` (which scalars live in SSA values), `strings.rs`
  (string ownership), `expr.rs`, `builtins.rs`, `stmt.rs`, `calls.rs`
  (procedures and by-reference calls), `io.rs`, `trap.rs` (ON ERROR)
- **rtbuild.rs** - The hand-written x86-64 runtime library's trees
  (`runtime/sysv`, `runtime/win64-native`: I/O, strings, math, using libc) and
  how they become one assembly unit. Shared by `build.rs`, which assembles the
  target's tree into `$OUT_DIR/runtime.o`, and by the crate's tests
- **runtime.rs** - The prebuilt runtime object, embedded; `main.rs` writes it
  beside the program's object and links the two. Generated code reaches the
  runtime's state and messages, and the runtime the program's `_data_*`, only
  through `.globl` symbols
- **rtsig.rs** - Signature of every runtime helper generated code calls,
  whether it can raise an error, and whether its string result is fresh.
  Every call goes through `FnGen::call`, which builds the call from it; tests
  derive `fallible` from the assembly's control flow and check that every
  `fresh` helper reaches `malloc`
- **main.rs** - CLI driver: reads source, runs the pipeline, writes the object
  and the runtime's, and links them with `cc` (or `link.exe`)
- **build.rs** - Assembles the runtime when the compiler is built, so building
  needs the target's assembler too

### Test Structure (`tests/`)

Integration tests organized by feature area:
- `common/mod.rs` - Test harness. `compile_and_run()` for the usual case;
  `compile_only()` for "this must be rejected, with this message"; and
  `compile_and_run_raw()` for "printed this, then aborted with this exit code"
- `docs/mod.rs` - Compiles every ```basic example in LANGREF.md and README.md
- `megatest/` - One large program (`mega.bas`) exercising most of the language
  at once, so that features are tested coexisting rather than in isolation. It
  is its own assertion harness: a regression names itself
- `codegen/` - Assertions on the Cranelift IR the compiler generates
  (`compile_to_clif()`, `uses_symbol()`), and behavioural tests of code paths
  with special cases: constant divisors, scalars in SSA values, huge functions
- Feature modules: `arithmetic/`, `arrays/`, `control/`, `data/`, `errors/`, `file_io/`, `input/`, `math/`, `print/`, `procedures/`, `strings/`, `types/`, `variables/`

### Key Design Decisions

- **No IR of its own**: the AST is lowered straight to Cranelift IR. Cranelift
  0.136.2, pinned exactly: its API changes between releases
- **Every variable lives in memory**: module names in data objects, a
  procedure's own in zeroed stack slots. No SSA value lives across a statement,
  because GOTO, GOSUB and RESUME may enter any statement from anywhere and a
  value defined in one would not dominate the next; FOR's limit and step and
  SELECT's selector are slots for the same reason
- **Every reference is colocated**: functions and data, the runtime's and libc's
  included, are reached with `call rel32` and RIP-relative addresses; the link is
  `-no-pie`
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
- **Two assemblers, not just two runtimes**: `build.rs` assembles the runtime
  with GNU `as` on Linux and `clang -c` on Windows, so the Win64 tree is the one built by the
  assembler no test here runs. They disagree about `.equ len, . - label`: GNU
  `as` folds it to an immediate, and an assembler that cannot prove the symbol
  absolute assembles `mov reg, len` as a *load from that address* instead.
  `CLS` did that on every Windows run. Bracket data with an `_end` label and
  subtract at run time
- **A runtime helper should take at most four arguments**: Win64 passes only
  four in registers, so a fifth is on the stack, where the hand-written Win64
  helper must find it above the shadow space. Cranelift places it correctly,
  but `_rt_instr` and `_rt_mid_assign` show what the runtime side costs; fold an
  argument away where possible (see `_rt_file_open_random`)
- **Unsupported GW-BASIC names are refused, not ignored**: an unrecognised name
  is otherwise just a new variable, so `UNSUPPORTED` in `sema.rs` names the
  keywords this compiler does not provide and why
- **String builtins returning a static buffer must copy**: two calls in one
  expression would otherwise alias, which is why `_rt_str`, `_rt_chr`, `_rt_hex`,
  `_rt_oct` and `_rt_mk` end in `_rt_strdup`
- **Strings are owned and freed** (`codegen/strings.rs`): bit 63 of a string
  slot's length word marks bytes the slot owns, which assignment frees; a
  FIELD window owns nothing. A helper's result is a temporary to free only if
  `rtsig` marks it `fresh` -- which a test checks reaches `malloc` -- so a new
  helper that allocates must say so, and one that returns a slice or a static
  buffer must not. The runtime allocates every string with `malloc`, never
  `HeapAlloc`, since generated code frees with `free`
- **No jumps to code addresses**: GOSUB pushes a return site *number* and
  RETURN dispatches on it with a `br_table` over its scope's sites; ON
  GOTO/GOSUB, the ON ERROR handler's entry (`_err_handler` holds a number) and
  main's unwind to a RESUME label are `br_table`s too
- **ON ERROR is error-status propagation**: a trapped `_rt_error` sets
  `_err_pending` and returns. A helper that raised leaves through its own
  epilogue; generated code tests the flag after each fallible call and each
  procedure call, to the statement's fault block. That calls
  `_err_handler_fn` -- main's code compiled a second time, entered at the ON
  ERROR target -- on top of the faulting frame, so RESUME and RESUME NEXT go
  back into a SUB as in QuickBASIC: the handler returns 1 or 2 and the fault
  block jumps to the statement's start or next block. A RESUME label returns 3
  and sets `_err_pending` to 2, so each procedure leaves through its epilogue
  and main dispatches on `_err_resume_label`. By-reference parameters are real
  references, so the handler sees what a procedure wrote. Only programs with
  ON ERROR pay for any of this
- **Every helper follows its platform's standard ABI**: a double goes where the
  ABI puts it -- by position on Win64, so `(file, value)` passes the value in
  xmm1 there. `rtsig.rs` lists one signature that serves both ABIs, and codegen
  builds each call's Cranelift signature from it

## Language Reference

See [LANGREF.md](LANGREF.md) for the supported BASIC dialect. Key points:
- Types: INTEGER (`%`), LONG (`&`), SINGLE (`!`), DOUBLE (`#`), STRING (`$`)
- Control flow: IF/THEN/ELSE, FOR/NEXT, WHILE/WEND, DO/LOOP, SELECT CASE, GOTO/GOSUB
- Procedures: SUB and FUNCTION with recursion; arguments by reference unless BYVAL
- File I/O: OPEN FOR INPUT/OUTPUT/APPEND, PRINT #, INPUT #, LINE INPUT #, CLOSE
- String indexing is 1-based (MID$, INSTR); array indexing is 0-based
