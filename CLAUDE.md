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
cargo run -- program.bas           # Output: program, beside the source
cargo run -- program.bas -o out    # Custom output name
cargo run -- -S program.bas        # Cranelift IR to <output>.clif instead (no object, no link)
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
  (where names live), `ssa.rs` (which scalars live in SSA values), `hoist.rs`
  (array descriptors read once per loop), `strings.rs`
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
- `integration.rs` - The one test crate: every module directory below is
  declared there, so a new one must be added
- `common/mod.rs` - Test harness. `compile_and_run()` for the usual case;
  `compile_only()` for "this must be rejected, with this message";
  `compile_and_run_raw()` for "printed this, then aborted with this exit code";
  and `compile_to_clif()` / `uses_symbol()` for the IR the compiler generates
- `docs/mod.rs` - Compiles every ```basic example in LANGREF.md and README.md
- `megatest/` - One large program (`mega.bas`) exercising most of the language
  at once, so that features are tested coexisting rather than in isolation. It
  is its own assertion harness: a regression names itself
- `codegen/` - Assertions on the Cranelift IR the compiler generates (through
  `common::compile_to_clif()`), and behavioural tests of code paths
  with special cases: constant divisors, scalars in SSA values, huge functions
- `runtime/` - Structural checks on the two assembly runtimes: the same
  `.globl` names, no `.equ` length constants, and the like
- Feature modules: `arithmetic/`, `arrays/`, `control/`, `data/`, `errors/`, `file_io/`, `input/`, `math/`, `print/`, `procedures/`, `strings/`, `types/`, `variables/`

### Key Design Decisions

- **No IR of its own**: the AST is lowered straight to Cranelift IR. Cranelift
  0.136.2, pinned exactly: its API changes between releases
- **Scalars only a function can reach live in SSA values** (`codegen/ssa.rs`):
  a numeric scalar, typed (`DIM X AS LONG`, in its declared type) or
  suffixed, not passed to a procedure, not SHARED or STATIC, and -- in
  main -- not in a program with ON ERROR is a Cranelift `Variable`, as are
  FOR's limit and step. Records and strings declared `AS` stay in memory. GOTO, GOSUB and RESUME entering a statement just give
  it a block parameter. Everything else lives in memory: module names in data
  objects, a procedure's own in stack slots zeroed on entry. No plain SSA
  value lives across a statement, since one defined in a statement would not
  dominate the statements GOTO can reach
- **Clean loops read array descriptors once** (`codegen/hoist.rs`): a loop
  nothing in which can change one -- no DIM/REDIM/ERASE, call, GOSUB/RETURN,
  branched-to label, nor ON ERROR anywhere -- reads each array's pointer,
  counts and lower bounds into `Variable`s before its head
- **Huge functions are compiled cheaply**: past `HUGE_FUNCTION_BLOCKS` (20,000)
  a function gets no optimizer and the single-pass register allocator, since
  the backtracking one is superlinear with SSA values live across it; that
  code runs about 2x slower
- **Every reference is colocated**: functions and data, the runtime's and libc's
  included, are reached with `call rel32` and RIP-relative addresses; on Linux
  the link is `-no-pie`
- **GW-BASIC semantics**: Division (`/`) always returns Double; integer division uses `\`
- **Default type is Double**: Unsuffixed numeric variables are `#` (Double), not Single
- **Boolean -1/0**: Comparisons return -1 (true) or 0 (false) for bitwise compatibility
- **Scope is QuickBASIC's**: module-level storage is static (.bss, zeroed),
  but a procedure reaches a module name only when it is shared (`DIM SHARED`,
  or `SHARED` in the procedure; a DEF FN sees all). Its other names are its
  own -- SSA values or zeroed frame slots -- unless STATIC, which gives them
  .bss storage of their own. `Symbols::shares` and `Symbols::is_static` decide, in one place each
- **Uniform word addressing**: every variable in memory, string (pointer +
  length), array descriptor and record field is a sequence of 8-byte words
  reached through `Loc`; a scalar in an SSA value is a `Place::Var`
- **Runtime checks on by default**: subscripts, division by zero, overflow
  and the rest; `--unsafe` removes them, and is refused with ON ERROR
- **Two runtimes in lockstep**: `src/runtime/sysv/` and `src/runtime/win64-native/` export
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
  and sets `_err_pending` to 2 -- as does a handler RETURNing to main's GOSUB,
  with 3 -- so each procedure leaves through its epilogue and main dispatches
  on `_err_resume_label`. By-reference parameters are real
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
- File I/O: OPEN FOR INPUT/OUTPUT/APPEND/RANDOM, PRINT #, INPUT #, LINE INPUT #,
  FIELD/GET/PUT, CLOSE
- String indexing is 1-based (MID$, INSTR); array subscripts start at 0 by
  default, at `OPTION BASE`'s value if set, or at `lo` in `DIM A(lo TO hi)`
