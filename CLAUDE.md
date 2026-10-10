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

xbasic64 is a BASIC native code compiler for x86-64 (Linux, Windows) and
aarch64 (Linux, macOS). The AST is lowered straight
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
- **runtime.rs** - The prebuilt runtime object (from `rt/`), embedded;
  `main.rs` writes it beside the program's object and links the two.
  Generated code reaches the runtime's state and messages, and the runtime
  the program's `_data_*`, only through exported symbols
- **rtsig.rs** - Signature of every runtime helper generated code calls,
  whether it can raise an error, and whether its string result is fresh.
  Every call goes through `FnGen::call`, which builds the call from it; tests
  read a call graph from `rt/src` and check that `fallible` is exactly "can
  reach `raise_with_code`" and `fresh` exactly "can reach `alloc_str`"
- **main.rs** - CLI driver: reads source, runs the pipeline, writes the object
  and the runtime's, and links them with `cc` (or `link.exe`)
- **build.rs** - Builds the runtime (`rt/`) with rustc for the target when the
  compiler is built: a `no_std` staticlib with fat LTO, of which only the
  crate's one merged object is kept (`$OUT_DIR/rt.o`; the rest of the
  archive is `compiler_builtins`). It reads the `ar` format itself

### The runtime (`rt/`)

A `no_std` Rust crate, one source for every target, calling the C library
through `rt/src/sys.rs`, where everything OS-specific lives (the standard
streams, errno, 64-bit seeks, locking, the clock). One module per area:
`error` (messages, trapping state, `_rt_error`), `num` (number formatting and
reading), `string`, `data`, `file` (files and the console as file 0), `input`,
`using`, `math`, `print`. Helpers are defined with `export!` under the names
and signatures `rtsig.rs` lists. Its unit tests run with std (`cargo test`
covers the workspace). In the runtime:

- **No `core::fmt`** outside tests: it adds tens of KB to every program.
  Numbers are formatted with the C library's `snprintf`, as the asm runtime
  did, so output is unchanged
- **Every string result that is fresh comes from `alloc_str`**, which raises
  Out of memory; so every fresh helper is also fallible

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
- Feature modules: `arithmetic/`, `arrays/`, `control/`, `data/`, `errors/`, `file_io/`, `input/`, `math/`, `print/`, `procedures/`, `strings/`, `types/`, `variables/`

### Key Design Decisions

- **No IR of its own**: the AST is lowered straight to Cranelift IR. Cranelift
  0.136.2, pinned exactly: its API changes between releases
- **Scalars only a function can reach live in SSA values** (`codegen/ssa.rs`):
  a numeric scalar, typed (`DIM X AS LONG`, in its declared type) or
  suffixed, not passed to a procedure, not SHARED or STATIC, and -- in
  main -- not reachable from the ON ERROR handler (`ssa::handler_names`:
  mentioned by no statement the handler can run) is a Cranelift `Variable`,
  as are FOR's limit and step (in a trapping program main's are in `_for_N`
  data, shared with the handler). Records and strings declared `AS` stay
  in memory. GOTO, GOSUB and RESUME entering a statement just give it a block
  parameter. Everything else lives in memory: module names in data
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
  included, are reached PC-relative (`call rel32` and RIP-relative on x86-64,
  `bl` and adrp+add on aarch64); on Linux the link is `-no-pie`, and macOS,
  where executables are always PIE, needs nothing more
- **Native only, two ISAs**: Cranelift's `x86` and `arm64` backends are built
  in, and the compiler targets the host it was built for. aarch64 Linux can be
  checked on an x86-64 host under qemu-user: cross-build with
  `aarch64-linux-gnu-gcc` as the linker, and put a `cc` that is that compiler
  first in PATH for the compiler's own link step
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
- **Unsupported GW-BASIC names are refused, not ignored**: an unrecognised name
  is otherwise just a new variable, so `UNSUPPORTED` in `sema.rs` names the
  keywords this compiler does not provide and why
- **String builtins returning a static buffer must copy**: two calls in one
  expression would otherwise alias, which is why `_rt_str`, `_rt_chr`, `_rt_hex`,
  `_rt_oct` and `_rt_mk` end in `_rt_strdup`
- **Strings are owned and freed** (`codegen/strings.rs`): bit 63 of a string
  slot's length word marks bytes the slot owns, which assignment frees; a
  FIELD window owns nothing. A helper's result is a temporary to free only if
  `rtsig` marks it `fresh` -- which a test checks reaches `alloc_str` -- so a new
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
- **Runtime helpers use System V on x86-64, on every OS**: a string result
  is two words returned in rax:rdx, as Cranelift returns two values, where
  the Windows C convention would return it through a hidden pointer. So
  `export!` defines each helper `extern "sysv64"` on x86-64 and codegen calls
  it with `CallConv::SystemV` (`RT_CALL_CONV`); elsewhere both use the
  platform's C convention, which returns two words in two registers.
  `rtsig.rs` lists one signature, and codegen builds each call from it

## Language Reference

See [LANGREF.md](LANGREF.md) for the supported BASIC dialect. Key points:
- Types: INTEGER (`%`), LONG (`&`), SINGLE (`!`), DOUBLE (`#`), STRING (`$`)
- Control flow: IF/THEN/ELSE, FOR/NEXT, WHILE/WEND, DO/LOOP, SELECT CASE, GOTO/GOSUB
- Procedures: SUB and FUNCTION with recursion; arguments by reference unless BYVAL
- File I/O: OPEN FOR INPUT/OUTPUT/APPEND/RANDOM, PRINT #, INPUT #, LINE INPUT #,
  FIELD/GET/PUT, CLOSE
- String indexing is 1-based (MID$, INSTR); array subscripts start at 0 by
  default, at `OPTION BASE`'s value if set, or at `lo` in `DIM A(lo TO hi)`
