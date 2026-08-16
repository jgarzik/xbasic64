# What xbasic64 Will Never Support

xbasic64 compiles 1980s BASIC to native x86-64 for Linux and Windows. A good
deal of GW-BASIC cannot follow it there — not because the work is hard, but
because the features describe a machine that no longer exists and a way of
working that a compiler does not have.

This document is the decision, so that the question does not have to be
reopened every time someone finds a listing that uses `POKE`. The companion
list of features that *are* coming is in
[LANGREF.md](LANGREF.md#not-implemented-yet).

Every name here is **refused by the compiler with a reason**, not silently
accepted. That is the part that matters. An unrecognised name in BASIC is
ordinarily just a new variable, so without this a program using `PEEK` would
compile without complaint and read zero forever.

---

## The four reasons

Everything below is out for one of four reasons.

**1. It addresses the 8086.** GW-BASIC ran in a 64 KB segmented address space
it shared with the BIOS, the video buffer and the interpreter itself. `PEEK`,
`POKE` and `DEF SEG` are how programs reached that memory, and `VARPTR` is how
they found their own variables in it. On x86-64 under a modern operating
system there is no fixed address worth naming: the video buffer is not memory,
the addresses are randomised, and the pages are protected. A `POKE` that did
anything at all would be a bug.

**2. It drives hardware directly.** `INP`, `OUT` and `WAIT` are I/O port
instructions. `SOUND` and `PLAY` program the PC speaker's timer chip. `STICK`
and `STRIG` read the game port. These are privileged operations in user mode;
a program that executes them is killed by the kernel, not obeyed.

**3. It is a command to the interpreter, not a statement in a program.**
`RUN`, `LIST`, `SAVE`, `RENUM` and the rest operate on the program text that
GW-BASIC kept in memory while you worked on it. A compiled program has no
program text at run time — that was the point of compiling it — so these have
nothing to act on.

**4. It belongs to a display model we do not have.** GW-BASIC's graphics
statements assume a CGA or EGA screen mode, a current cursor position, a
palette of at most 16 entries, and a coordinate space the interpreter owns
outright. Reproducing that faithfully means shipping a graphics stack; not
reproducing it faithfully means programs that draw the wrong thing. Text-mode
console control is a different matter and *is* planned — see
[LANGREF.md](LANGREF.md#not-implemented-yet).

---

## The list

### Memory and hardware access

`PEEK`, `POKE`, `DEF SEG`, `VARPTR`, `VARPTR$`, `VARSEG`, `DEF USR`, `USR`,
`CALL` (the GW-BASIC form, which calls machine code at an address), `INP`,
`OUT`, `WAIT`, `BLOAD`, `BSAVE`, `IOCTL`, `IOCTL$`, `ERDEV`, `ERDEV$`,
`EXTERR`

Reasons 1 and 2. Note that QuickBASIC's `CALL` — calling a `SUB` by name — is
a different statement that happens to share a keyword; that one is merely
[not implemented yet](LANGREF.md#not-implemented-yet), and `SUB` calls work
without it today.

### Graphics

`SCREEN`, `PSET`, `PRESET`, `LINE` (the drawing statement; `LINE INPUT` is
supported and unrelated), `CIRCLE`, `PAINT`, `DRAW`, `GET`/`PUT` in their
graphics forms, `VIEW`, `WINDOW`, `PMAP`, `POINT`, `PALETTE`, `PALETTE USING`

Reason 4. `GET` and `PUT` *are* supported in their file forms — see
[Random-Access Files](LANGREF.md#random-access-files) — which is why the
graphics forms are called out separately here.

### Sound

`SOUND`, `PLAY`, `PLAY(n)`, `ON PLAY`

Reason 2. (`BEEP` is the exception: it is one control character, and it is
planned rather than refused forever.)

### Light pen, joystick and soft keys

`PEN`, `ON PEN`, `STICK`, `STRIG`, `ON STRIG`, `KEY`, `KEY(n)`, `ON KEY(n)`

Reason 2, and in the case of `KEY` a display model — the soft-key line across
the bottom of the screen — that reason 4 covers.

### Serial communications

`COM(n)`, `ON COM(n)`, `OPEN "COM1:..."`

Reason 2. Serial ports are reachable on both platforms, but through the
operating system rather than through GW-BASIC's model of them, and a program
written against that model would not work unchanged anyway. A program needing
a serial port is better served by the host OS than by a 1983 abstraction of
it.

### Event trapping

`ON TIMER(n)`, and the `ON` forms of the device statements above

GW-BASIC checked for pending events between statements. Reproducing that means
a check between every statement of compiled code, which would slow down every
program to serve a feature almost none of them use. `ON ERROR` is not in this
category and is [planned](LANGREF.md#not-implemented-yet).

### The line printer

`LPRINT`, `LPRINT USING`, `LPOS`

There is no `LPT1:`. A program that wants a printer on either supported
platform should write a file and hand it to the spooler; `PRINT #` already
does the first half.

### Interpreter and editor commands

`AUTO`, `CONT`, `DELETE`, `EDIT`, `LIST`, `LLIST`, `LOAD`, `MERGE`, `NEW`,
`RENUM`, `RUN`, `SAVE`, `TRON`, `TROFF`, `CLEAR`

Reason 3.

### Program chaining

`CHAIN`, `COMMON`

Reason 3. `CHAIN` loads another BASIC program over the running one and jumps
into it, with `COMMON` naming the variables that survive the transition. A
compiled executable cannot absorb another program's code, and the modern
equivalent — running a second program and passing data through a file, a pipe
or the command line — is a different design rather than a port of this one.

---

## Deliberately different, not missing

Some GW-BASIC behaviour is supported but does not match exactly. Those choices
are listed in
[LANGREF.md](LANGREF.md#deliberate-differences-from-gw-basic) — number
formatting, integer assignment, and the requirement that arrays be declared
before use.

---

## Keeping this honest

The compiler's own table of refused names lives in `UNSUPPORTED` in
`src/sema.rs`, and `tests/errors/mod.rs` checks that these names are diagnosed
rather than quietly accepted. If a name is added here, it belongs in that table
too, so that the documentation and the compiler cannot drift apart.

---

## Appendix: every GW-BASIC keyword, accounted for

The keyword list is the index of the *Microsoft GW-BASIC User's Guide and
Reference*. Every entry has a status:

- **yes** — supported
- **later** — practical on both platforms, not written yet
- **never** — one of the four reasons above

| Keyword | Status | Keyword | Status |
|---|---|---|---|
| `ABS` | yes | `LOC` | yes |
| `ASC` | yes | `LOCATE` | later |
| `ATN` | yes | `LOCK` | yes |
| `AUTO` | never | `LOF` | yes |
| `BEEP` | later | `LOG` | yes |
| `BLOAD` | never | `LPOS` | never |
| `BSAVE` | never | `LPRINT` | never |
| `CALL` | later (QB form) | `LPRINT USING` | never |
| `CDBL` | yes | `LSET` | yes |
| `CHAIN` | never | `MERGE` | never |
| `CHDIR` | later | `MID$` (function) | yes |
| `CHR$` | yes | `MID$` (statement) | yes |
| `CINT` | yes | `MKDIR` | later |
| `CIRCLE` | never | `MKD$` | yes |
| `CLEAR` | never | `MKI$` | yes |
| `CLOSE` | yes | `MKS$` | yes |
| `CLS` | yes | `NAME` | later |
| `COLOR` | later (text) | `NEW` | never |
| `COM(n)` | never | `NEXT` | yes |
| `COMMON` | never | `OCT$` | yes |
| `CONT` | never | `ON COM(n)` | never |
| `COS` | yes | `ON ERROR GOTO` | later |
| `CSNG` | yes | `ON KEY(n)` | never |
| `CSRLIN` | later | `ON PEN` | never |
| `CVD` | yes | `ON PLAY(n)` | never |
| `CVI` | yes | `ON STRIG(n)` | never |
| `CVS` | yes | `ON TIMER(n)` | never |
| `DATA` | yes | `ON...GOSUB` | later |
| `DATE$` | later | `ON...GOTO` | yes |
| `DEF FN` | yes | `OPEN` | yes |
| `DEF SEG` | never | `OPEN "COM(n)"` | never |
| `DEF USR` | never | `OPTION BASE` | yes |
| `DEFDBL` | later | `OUT` | never |
| `DEFINT` | later | `PAINT` | never |
| `DEFSNG` | later | `PALETTE` | never |
| `DEFSTR` | later | `PCOPY` | never |
| `DELETE` | never | `PEEK` | never |
| `DIM` | yes | `PEN` | never |
| `DRAW` | never | `PLAY` | never |
| `EDIT` | never | `PMAP` | never |
| `END` | yes | `POINT` | never |
| `ENVIRON` | later | `POKE` | never |
| `ENVIRON$` | later | `POS` | later |
| `EOF` | yes | `PRESET` | never |
| `ERASE` | later | `PRINT` | yes |
| `ERDEV` | never | `PRINT USING` | yes |
| `ERL` | later | `PRINT#` | yes |
| `ERR` | later | `PRINT# USING` | no (see LANGREF) |
| `ERROR` | later | `PSET` | never |
| `EXP` | yes | `PUT` (files) | yes |
| `EXTERR` | never | `PUT` (graphics) | never |
| `FIELD` | yes | `RANDOMIZE` | later |
| `FILES` | later | `READ` | yes |
| `FIX` | yes | `REM` | yes |
| `FOR` | yes | `RENUM` | never |
| `FRE` | later | `RESET` | never |
| `GET` (files) | yes | `RESTORE` | yes |
| `GET` (graphics) | never | `RESUME` | later |
| `GOSUB` | yes | `RETURN` | yes |
| `GOTO` | yes | `RIGHT$` | yes |
| `HEX$` | yes | `RMDIR` | later |
| `IF` | yes | `RND` | yes |
| `INKEY$` | later | `RSET` | yes |
| `INP` | never | `RUN` | never |
| `INPUT` | yes | `SAVE` | never |
| `INPUT#` | yes | `SCREEN` | never |
| `INPUT$` | later | `SGN` | yes |
| `INSTR` | yes | `SHELL` | later |
| `INT` | yes | `SIN` | yes |
| `IOCTL` | never | `SOUND` | never |
| `IOCTL$` | never | `SPACE$` | yes |
| `KEY` | never | `SPC` | yes |
| `KEY(n)` | never | `SQR` | yes |
| `KILL` | later | `STICK` | never |
| `LEFT$` | yes | `STOP` | yes |
| `LEN` | yes | `STR$` | yes |
| `LET` | yes | `STRIG` | never |
| `LINE` (graphics) | never | `STRING$` | yes |
| `LINE INPUT` | yes | `SWAP` | yes |
| `LINE INPUT#` | yes | `SYSTEM` | never |
| `LIST` | never | `TAB` | yes |
| `LLIST` | never | `TAN` | yes |
| `LOAD` | never | `TIME$` | later |
| `TIMER` | yes | `USR` | never |
| `TROFF` | never | `VAL` | yes |
| `TRON` | never | `VARPTR` | never |
| `UNLOCK` | yes | `VARPTR$` | never |
| `VIEW` | never | `WAIT` | never |
| `VIEW PRINT` | later | `WEND` | yes |
| `WHILE` | yes | `WIDTH` | later |
| `WINDOW` | never | `WRITE` | yes |
| `WRITE#` | yes | | |

### Beyond GW-BASIC

xbasic64 also supports these later QuickBASIC features, which GW-BASIC has no
equivalent for: `SUB`/`FUNCTION` with recursion, `TYPE` records, `SELECT CASE`,
`DO`/`LOOP`, `EXIT`, `CONST`, `REDIM`/`REDIM PRESERVE`, `LBOUND`/`UBOUND`,
named labels, `UCASE$`, `LCASE$`, `LTRIM$`, `RTRIM$`, `CLNG`, `MKL$` and `CVL`.
