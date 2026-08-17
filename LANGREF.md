# xbasic64 Language Reference

This document describes the BASIC dialect supported by xbasic64, a native code compiler targeting 1980s-era BASIC (Tandy Color BASIC, GW-BASIC, QuickBASIC).

## Table of Contents

- [Lexical Structure](#lexical-structure)
- [Data Types](#data-types)
- [Variables and Arrays](#variables-and-arrays)
- [Expressions and Operators](#expressions-and-operators)
- [Statements](#statements)
- [Built-in Functions](#built-in-functions)
- [File I/O](#file-io)
- [Random-Access Files](#random-access-files)
- [Procedures](#procedures)
- [Limitations](#limitations)

---

## Lexical Structure

### Character Set

Programs are ASCII text. UTF-8 input is accepted but only ASCII characters are recognized in code.

### Comments

```basic
REM This is a comment
' This is also a comment
```

Both `REM` and single-quote comments extend to end of line.

### Line Numbers

Line numbers are optional and appear at the start of a line:

```basic
10 PRINT "Hello"
20 GOTO 10
```

Line numbers serve as labels for `GOTO` and `GOSUB` targets.

### Labels

A name followed by a colon is a label, and marks a target the same way a line
number does:

```basic
GOTO Finish
PRINT "skipped"
Finish:
PRINT "done"
```

A label definition must be the first thing on its line, though a statement may
follow it there (`Finish: PRINT "done"`). Labels are case-insensitive, like
every other name, and are accepted wherever a line number is: `GOTO`, `GOSUB`,
`ON...GOTO` and `RESTORE`. A name already declared as a `SUB` is read as a call
to it rather than as a label definition.

### Statement Separators

Multiple statements can appear on one line separated by colons:

```basic
A = 1 : B = 2 : PRINT A + B
```

### Line Continuation

A trailing underscore joins a statement to the next line. Nothing may follow it
on the line it ends:

```basic
Total = Price * Quantity + _
        Shipping
```

### Block Terminators

Each multi-line block may be closed with either the two-word form or a single
word; the two are interchangeable:

| Two words      | One word       |
|----------------|----------------|
| `END IF`       | `ENDIF`        |
| `END SUB`      | `ENDSUB`       |
| `END FUNCTION` | `ENDFUNCTION`  |
| `END SELECT`   | `ENDSELECT`    |
| `END TYPE`     | `ENDTYPE`      |

The examples in this reference use the two-word form throughout.

### Identifiers

Variable and procedure names:
- Start with a letter (A-Z, a-z)
- Followed by letters, digits, or underscore
- Case-insensitive (`MyVar` and `MYVAR` are the same)
- May end with a type suffix (`%`, `&`, `!`, `#`, `$`)

### Literals

**Integers:**
```basic
A = 42          ' Decimal
B = -17         ' Negative
C = &HFF        ' Hexadecimal (255)
D = &O377       ' Octal (255)
E = &377        ' Octal too: a bare & introduces octal
F = &B1010      ' Binary (10)
```

**Floating-point:**
```basic
A = 3.14
B = .5
C = 1E+10
D = 2.5D-3      ' Double precision
```

**Strings:**
```basic
A$ = "Hello, World!"
B$ = "She said ""Hi"""   ' Embedded quote
```

---

## Data Types

xbasic64 supports five data types, indicated by suffix characters:

| Suffix | Type    | Description                | Size    | Range/Notes                    |
|--------|---------|----------------------------|---------|--------------------------------|
| `%`    | INTEGER | Signed integer             | 16-bit  | -32,768 to 32,767              |
| `&`    | LONG    | Signed long integer        | 32-bit  | -2,147,483,648 to 2,147,483,647|
| `!`    | SINGLE  | Single-precision float     | 32-bit  | ~7 digits precision            |
| `#`    | DOUBLE  | Double-precision float     | 64-bit  | ~15 digits precision           |
| `$`    | STRING  | Character string           | Dynamic | Heap-allocated                 |

### Default Type

**Unsuffixed numeric variables default to DOUBLE (`#`)** unless a `DEF*`
statement says otherwise.

### DEFINT, DEFLNG, DEFSNG, DEFDBL, DEFSTR

These set the default type for names beginning with the given letters, so a
listing need not suffix every variable:

```basic
DEFINT A-Z          ' every unsuffixed name is an INTEGER
DEFSTR S            ' except those starting with S, which are strings
DEFINT A, C-E       ' single letters and ranges, comma separated
```

A suffix always wins over the default, and the two spellings of one name are
the same variable: after `DEFINT A`, `A` and `A%` share storage. The default
applies to the whole program rather than from the statement onwards, which is
where these are written in practice.

```basic
X = 3.14159       ' X is Double
Y% = 10           ' Y% is Integer
Name$ = "Alice"   ' Name$ is String
```

### Type Coercion

Numeric types are automatically converted when mixed in expressions:

```
INTEGER → LONG → SINGLE → DOUBLE
```

The result takes the wider type. String/numeric mixing is not allowed; use `VAL()` and `STR$()` for explicit conversion.

### Division Semantics

Following GW-BASIC conventions:
- `/` (division) always produces a **Double** result
- `\` (integer division) produces a **Long** result

```basic
PRINT 7 / 2       ' Prints 3.5
PRINT 7 \ 2       ' Prints 3
```

### Boolean Values

There is no dedicated boolean type. Comparisons return:
- `-1` (all bits set) for **true**
- `0` for **false**

This matches GW-BASIC/QuickBASIC semantics and allows bitwise operations on results.

---

## Variables and Arrays

### Simple Variables

Variables are created on first use (implicit declaration):

```basic
Count% = 0
Total# = 0.0
Name$ = ""
```

### Arrays

Arrays are declared with `DIM` and support multiple dimensions:

```basic
DIM Scores(10)           ' 11 elements: 0 to 10
DIM Matrix(5, 5)         ' 6x6 elements
DIM Cube(3, 3, 3)        ' 4x4x4 elements
```

**Array indices start at 0** by default.

Arrays can hold any type:
```basic
DIM Names$(100)          ' String array
DIM Values%(50)          ' Integer array
```

### Scope

- **Global by default**: a name used anywhere at module level is global, and
  refers to the same storage inside every procedure
- **Local in procedures**: a name used only inside a `SUB` or `FUNCTION` is
  local to it, and starts fresh (0 or `""`) on every call, so recursion works
- **Parameters shadow**: a parameter, and a `FUNCTION`'s return
  pseudo-variable, are always local even when a global shares the name

The first two rules together mean that assigning to `X` inside a procedure
writes the global `X` if and only if `X` also appears at module level.

Unassigned variables read as `0` or `""`; they are never left holding whatever
was previously in memory.

---

## Expressions and Operators

### Arithmetic Operators

| Operator | Description           | Example        |
|----------|-----------------------|----------------|
| `+`      | Addition              | `A + B`        |
| `-`      | Subtraction           | `A - B`        |
| `*`      | Multiplication        | `A * B`        |
| `/`      | Division (→ Double)   | `A / B`        |
| `\`      | Integer division (→ Long) | `A \ B`    |
| `MOD`    | Modulo                | `A MOD B`      |
| `^`      | Exponentiation        | `A ^ B`        |
| `-`      | Unary negation        | `-A`           |

### Comparison Operators

| Operator | Description           |
|----------|-----------------------|
| `=`      | Equal                 |
| `<>`     | Not equal             |
| `<`      | Less than             |
| `>`      | Greater than          |
| `<=`     | Less than or equal    |
| `>=`     | Greater than or equal |

Comparisons return `-1` (true) or `0` (false).

Strings compare too, lexicographically by character value, so comparison is
case-sensitive and a prefix sorts before the longer string (`"ab" < "abc"`).

### Logical Operators

| Operator | Description           |
|----------|-----------------------|
| `AND`    | Bitwise/logical AND   |
| `OR`     | Bitwise/logical OR    |
| `XOR`    | Bitwise/logical XOR   |
| `NOT`    | Bitwise/logical NOT   |
| `EQV`    | Bitwise equivalence   |
| `IMP`    | Bitwise implication   |

These operate bitwise on integers, allowing both logical tests and bit
manipulation. Their operands are converted to integers first, and the result is
an integer:

```basic
IF A > 0 AND B > 0 THEN PRINT "Both positive"
Flags% = Flags% OR &H01    ' Set bit 0
Mask% = NOT &H00FF         ' -256: every bit flipped
```

`NOT` complements every bit, so `NOT 1` is `-2`, which is non-zero and therefore
*true*. This matters only when testing a value that is not already a truth
value: comparisons yield -1 or 0, and `NOT` maps those to each other, so
`IF NOT (A > 0)` behaves as expected while `IF NOT 1` does not.

### String Concatenation

```basic
FullName$ = First$ + " " + Last$
```

### Operator Precedence

From highest to lowest:
1. `^` (exponentiation)
2. `-` (unary negation)
3. `*`, `/`, `\`, `MOD`
4. `+`, `-`
5. `=`, `<>`, `<`, `>`, `<=`, `>=`
6. `NOT`
7. `AND`
8. `OR`, `XOR`
9. `EQV`
10. `IMP`

Because `^` binds tighter than unary negation, `-2 ^ 2` is `-(2 ^ 2)` = -4.

Operators of equal precedence associate left to right, `^` included: `2 ^ 3 ^ 2`
is `(2 ^ 3) ^ 2` = 64, and `100 - 10 - 5` is 85.

Use parentheses to override precedence:
```basic
Result = (A + B) * C
```

---

## Statements

### Assignment

```basic
LET X = 10      ' LET is optional
X = 10          ' Same as above
A$ = "Hello"
DIM Arr(10)
Arr(5) = 42
```

`LET` is accepted before any assignment, including record fields and
`MID$`:

```basic
TYPE Person
    Name AS STRING * 20
END TYPE
DIM P AS Person

LET P.Name = "Ada"
S$ = "xxllo"
LET MID$(S$, 1, 2) = "HE"       ' S$ is now "HEllo"
```

### PRINT

Output to console:

```basic
PRINT "Hello, World!"
PRINT X; Y; Z             ' Semicolon: no space between
PRINT A, B, C             ' Comma: tab-separated
PRINT "Value: "; X
PRINT                     ' Print blank line
```

Semicolon at end suppresses newline:
```basic
PRINT "Enter value: ";
```

### PRINT USING

Formatted output. The format is a string literal:

```basic
PRINT USING "###.##"; 3.14159       '   3.14
PRINT USING "Total: ###"; 42        ' Total:  42
PRINT USING "## and ##"; 1; 2       '   1 and  2
```

Numeric fields:

| Format  | Meaning                                          |
|---------|--------------------------------------------------|
| `###`   | Digit positions; the value is right-justified    |
| `##.##` | Digits either side of a decimal point            |
| `+`     | Leading or trailing sign, always shown           |
| `-`     | Trailing sign, shown only for negatives          |
| `,`     | Group the integer part in thousands              |
| `$$`    | Leading currency sign, floated against the digits|
| `**`    | Pad with asterisks instead of spaces             |
| `**$`   | Both of the above                                |
| `^^^^`  | Exponential form                                 |

String fields:

| Format  | Meaning                                          |
|---------|--------------------------------------------------|
| `!`     | First character only                             |
| `\   \` | Fixed width: 2 plus the spaces between           |
| `&`     | The whole string                                 |

`_` emits the next character literally. A value too wide for its field is
printed in full, preceded by `%`. If values remain after the format is
exhausted, the format restarts.

A field is at most 255 characters wide with at most 40 fractional digits;
a format asking for more is clamped to those.

### INPUT

Read user input:

```basic
INPUT X                   ' Prompt with "? "
INPUT "Enter name: "; N$  ' Prints: Enter name: ?
INPUT "Enter name: ", N$  ' Prints: Enter name:
INPUT "X, Y: ", X, Y      ' Multiple values
```

The separator decides the question mark: a `;` after the prompt adds `? `, a
`,` suppresses it, and a prompt-less `INPUT` prints `? ` on its own.

A `;` *before* the prompt is accepted and ignored:

```basic
INPUT ; "Enter name: "; N$
```

In GW-BASIC it suppressed the newline echoed when the operator pressed Return.
That newline comes from the terminal here rather than from the program, so
there is nothing for it to suppress.

### LINE INPUT

Read entire line as string (no parsing):

```basic
LINE INPUT "Enter text: ", Text$
```

`LINE INPUT` never adds a question mark; write one into the prompt if you want
one.

### IF...THEN...ELSE

**Single-line form:**
```basic
IF X > 0 THEN PRINT "Positive"
IF X > 0 THEN Y = 1 ELSE Y = 0
```

Both branches take a list of statements separated by colons. Everything after
`THEN` up to `ELSE` or the end of the line is conditional, and everything after
`ELSE` is too:

```basic
IF X > 0 THEN Y = 1 : PRINT "Positive" ELSE Y = 0 : PRINT "Not positive"
```

A bare line number after `THEN` or `ELSE` is an implied `GOTO`:

```basic
10 IF X < 0 THEN 90
20 IF X = 0 THEN 90 ELSE 80
80 PRINT "Positive"
90 PRINT "Done"
```

**Block form:**
```basic
IF X > 0 THEN
    PRINT "Positive"
ELSEIF X < 0 THEN
    PRINT "Negative"
ELSE
    PRINT "Zero"
END IF
```

### SELECT CASE

Multi-way branching:

```basic
SELECT CASE Grade%
    CASE 90 TO 100
        PRINT "A"
    CASE 80 TO 89
        PRINT "B"
    CASE 70 TO 79
        PRINT "C"
    CASE ELSE
        PRINT "Below C"
END SELECT
```

Case expressions can be:
- Single values: `CASE 1`
- Ranges: `CASE 1 TO 10`
- Comparisons: `CASE IS > 100`
- Lists: `CASE 1, 2, 3`

### FOR...NEXT

Counted loop:

```basic
FOR I = 1 TO 10
    PRINT I
NEXT I

FOR J = 10 TO 1 STEP -1
    PRINT J
NEXT J

FOR K = 0 TO 1 STEP 0.1
    PRINT K
NEXT K
```

The loop variable name after `NEXT` is optional, and a bare `NEXT` closes the
innermost open loop:
```basic
FOR I = 1 TO 10
    PRINT I
NEXT
```

If the name *is* given it must be the one that loop counts, so `FOR I ... NEXT J`
is an error rather than a loop closed by surprise. One `NEXT` may close several
nested loops, innermost first:
```basic
FOR I = 1 TO 3
    FOR J = 1 TO 3
        PRINT I * J
NEXT J, I
```

### WHILE...WEND

Pre-test loop:

```basic
WHILE X < 100
    X = X * 2
WEND
```

### DO...LOOP

Flexible loop with conditions:

```basic
' Pre-test with WHILE
DO WHILE X < 100
    X = X + 1
LOOP

' Post-test with WHILE
DO
    X = X + 1
LOOP WHILE X < 100

' Pre-test with UNTIL
DO UNTIL X >= 100
    X = X + 1
LOOP

' Post-test with UNTIL
DO
    X = X + 1
LOOP UNTIL X >= 100
```

### GOTO

Unconditional jump to line number or label:

```basic
10 PRINT "Loop"
20 GOTO 10
```

### GOSUB / RETURN

Call subroutine and return:

```basic
GOSUB 1000
PRINT "Back from subroutine"
END

1000 PRINT "In subroutine"
1010 RETURN
```

### ON...GOTO / ON...GOSUB

Computed jump:

```basic
ON Choice GOTO 100, 200, 300
' If Choice=1, goto 100; if Choice=2, goto 200; etc.
100 PRINT "one"
110 END
200 PRINT "two"
210 END
300 PRINT "three"
```

`ON...GOSUB` calls the chosen subroutine instead, and every `RETURN` comes back
to the statement after the `ON`, whichever one ran:

```basic
ON Choice GOSUB Draw, Erase
PRINT "back, whichever ran"
END
Draw:
PRINT "drawing"
RETURN
Erase:
PRINT "erasing"
RETURN
```

The selector is truncated to an integer. A value that matches nothing -- zero,
negative, or past the end of the list -- runs no subroutine and continues with
the next statement.

### DIM

Declare arrays:

```basic
DIM A(100)           ' 1D array, indices 0-100
DIM B(10, 20)        ' 2D array
DIM C(5, 5, 5)       ' 3D array
DIM Names$(50)       ' String array
```

### DATA / READ / RESTORE

Inline data:

```basic
100 DATA 10, 20, 30, "Hello", "World"

READ A, B, C
READ X$, Y$

RESTORE          ' Reset data pointer to beginning
RESTORE 100      ' Resume at the DATA on line 100
```

A DATA item needs quotes only if it contains a comma, a colon, or spaces that
matter. Otherwise write it plainly; surrounding spaces are trimmed and the text
is taken exactly as written, case included. An omitted item reads as 0 or `""`:

```basic
DATA hello, World, "a,b", "  padded  "
DATA 1,,3
```

A colon ends a DATA statement, so another statement may follow it on the same
line.

### CLS

Clear screen:

```basic
CLS
```

### BEEP, ERASE and SYSTEM

```basic
BEEP                  ' Ring the terminal bell
DIM Scores(10)
ERASE Scores          ' Release it, so it may be DIMed again
DIM Scores(50)
SYSTEM                ' End the program, as END does
```

`ERASE` is recognised only before a name, so `Erase` remains usable as a label
or a variable elsewhere. Its argument must be an array that is DIMed somewhere
-- a name that is not one is a mistake, not a statement that does nothing.

### LOCATE and COLOR

Console control, written as ANSI escape sequences:

```basic
CLS
LOCATE 5, 10          ' Row 5, column 10; both count from 1
LOCATE 3              ' Row only; the column is left alone
LOCATE , 20           ' Column only
COLOR 14, 1           ' Bright yellow on blue
COLOR 7               ' Foreground only
PRINT "positioned"
```

Colours are GW-BASIC's 0-15, where 8-15 are the bright half. GW-BASIC's third
`COLOR` argument sets the border, which a terminal has no equivalent for, and is
refused rather than ignored. `LOCATE` with no row asks for row 1, since the row
is not tracked the way the column is.

`POS(0)` gives the column the next character will be written to, counting from 1:

```basic
PRINT "abc";
PRINT POS(0)          ' 4
```

### SWAP

Exchange two values of the same type, including array elements:

```basic
DIM Items(10)
SWAP A, B
SWAP Items(I), Items(J)
```

### CONST

A named constant, folded at compile time. It may be used anywhere a literal
can, including as an array bound:

```basic
CONST MAX = 100
CONST HALF = MAX / 2
CONST TITLE$ = "Report"
DIM Buffer(MAX)
```

### WRITE

Like `PRINT`, but values are separated by commas and strings are quoted:

```basic
WRITE "Alice", 30       ' "Alice",30
WRITE #1, A$, N         ' the same, to a file
```

`WRITE` always ends its line, and what it writes is exactly what
`INPUT #` reads back.

### EXIT

Leave the innermost matching loop, or return early from a procedure:

```basic
FOR I = 1 TO 100
    IF Found THEN EXIT FOR
NEXT I

DO
    IF Done THEN EXIT DO
LOOP

SUB Check(N)
    IF N = 0 THEN EXIT SUB
    PRINT N
END SUB
```

### REDIM

Resize an existing array. `REDIM` alone clears it; `REDIM PRESERVE` keeps the
existing elements and zeroes the new ones:

```basic
DIM Items(10)
REDIM Items(20)             ' cleared
REDIM PRESERVE Items(30)    ' contents kept
```

Only the last dimension may change under `PRESERVE`.

### OPTION BASE

Set the lowest legal subscript. It must appear before any `DIM`, and only once:

```basic
OPTION BASE 1
DIM Items(10)     ' subscripts 1 to 10
```

### DEF FN

A single-expression function. The name must begin with `FN`:

```basic
DEF FNArea(W, H) = W * H
PRINT FNArea(3, 4)
```

### TYPE

A user-defined record. Fields may be any built-in type or another record:

```basic
TYPE Point
    X AS INTEGER
    Y AS INTEGER
END TYPE

TYPE Person
    Name AS STRING * 30
    Home AS Point
END TYPE

DIM P AS Person
P.Name = "Alice"
P.Home.X = 10

DIM People(100) AS Person
People(0).Name = "Bob"
```

Assigning one record to another copies it. A record may be passed to a
procedure, where it arrives by value:

```basic
TYPE Person
    Name AS STRING * 30
END TYPE

SUB Show(P AS Person)
    PRINT P.Name
END SUB
```

`AS` also gives an ordinary variable, parameter or `FUNCTION` result a
declared type:

```basic
DIM Count AS INTEGER

SUB Log(Level AS INTEGER, Text AS STRING * 80)
    PRINT Level; Text
END SUB

FUNCTION Total AS DOUBLE
    Total = 1.5
END FUNCTION

FUNCTION Half(X) AS INTEGER
    Half = X / 2        ' Half(7) is 3, not 3.5
END FUNCTION
```

A declared result type must not contradict a type suffix on the name, and
a `SUB` -- having no result -- cannot be declared `AS` anything.

One `DIM` may mix declarators freely:

```basic
DIM I AS INTEGER, Grid(9, 9) AS INTEGER, Names$(20)

```

### FIELD / LSET / RSET / GET / PUT / LOCK

The random-access statements. They are described together in
[Random-Access Files](#random-access-files), since none of them means anything
without the others.

### END / STOP

Terminate program:

```basic
END     ' Normal termination
STOP    ' Terminate (historically for debugging)
```

---

## Built-in Functions

### Math Functions

| Function   | Description                              |
|------------|------------------------------------------|
| `ABS(x)`   | Absolute value                           |
| `INT(x)`   | Floor (largest integer ≤ x)              |
| `FIX(x)`   | Truncate toward zero                     |
| `SGN(x)`   | Sign: -1, 0, or 1                        |
| `SQR(x)`   | Square root                              |
| `SIN(x)`   | Sine (radians)                           |
| `COS(x)`   | Cosine (radians)                         |
| `TAN(x)`   | Tangent (radians)                        |
| `ATN(x)`   | Arctangent (returns radians)             |
| `EXP(x)`   | e raised to power x                      |
| `LOG(x)`   | Natural logarithm                        |
| `RND`      | Random number 0 ≤ r < 1                  |
| `FRE(x)`   | Free memory; a large constant here       |

**Numeric output:** `PRINT` writes the shortest decimal that reads back as the
same value, so a `DOUBLE` shows its full precision (`PRINT 1 / 3` gives
`0.3333333333333333`) and a `SINGLE` shows only the ~7 digits it carries.

**RND behavior:** the argument selects between three behaviours.

```basic
X = RND           ' Next random number
X = RND(1)        ' Next random number; any positive value does this
X = RND(0)        ' The previous number again
X = RND(-7)       ' Reseed from -7, then return the next number
```

The generator starts from a fixed seed, so a program that never reseeds replays
the same numbers on every run -- which is useful while debugging and wrong for a
game. `RANDOMIZE` is how a program chooses:

```basic
RANDOMIZE            ' Seed from the clock: a different run every time
RANDOMIZE TIMER      ' The same thing, written out
RANDOMIZE 42         ' A fixed seed: the same run every time
```

GW-BASIC's bare `RANDOMIZE` asks the operator for a seed. A compiled program has
nobody to ask, so it takes the clock.

### String Functions

| Function              | Description                                    |
|-----------------------|------------------------------------------------|
| `LEN(s$)`             | Length of string                               |
| `DATE$`               | Current date, as `MM-DD-YYYY`                  |
| `TIME$`               | Current time, as `HH:MM:SS`                    |
| `LEFT$(s$, n)`        | Leftmost n characters                          |
| `RIGHT$(s$, n)`       | Rightmost n characters                         |
| `MID$(s$, start, len)`| Substring (1-based index)                      |
| `MID$(s$, start)`     | Substring from start to end                    |
| `INSTR(s$, find$)`    | Position of find$ in s$ (0 if not found)       |
| `INSTR(start, s$, find$)` | Search starting at position              |
| `ASC(s$)`             | ASCII code of first character                  |
| `CHR$(n)`             | Character from ASCII code                      |
| `VAL(s$)`             | Convert string to number                       |
| `STR$(x)`             | Convert number to string                       |
| `SPACE$(n)`           | A string of n spaces                           |
| `STRING$(n, c)`       | n copies of a character (code or first of c$)  |
| `LTRIM$(s$)`          | Drop leading spaces                            |
| `RTRIM$(s$)`          | Drop trailing spaces                           |
| `UCASE$(s$)`          | Convert to upper case                          |
| `LCASE$(s$)`          | Convert to lower case                          |
| `HEX$(n)`             | Hexadecimal text for an integer                |
| `OCT$(n)`             | Octal text for an integer                      |

**String indexing is 1-based** for `MID$` and `INSTR`.

`MID$` may also be assigned to, overwriting characters in place. The target's
length never changes:

```basic
A$ = "hello"
MID$(A$, 1, 1) = "J"      ' A$ is now "Jello"
```

### Type Conversion Functions

| Function   | Description                              |
|------------|------------------------------------------|
| `CINT(x)`  | Convert to Integer (with rounding)       |
| `CLNG(x)`  | Convert to Long (with rounding)          |
| `CSNG(x)`  | Convert to Single                        |
| `CDBL(x)`  | Convert to Double                        |

### Other Functions

| Function      | Description                                    |
|---------------|------------------------------------------------|
| `TIMER`       | Seconds since midnight, UTC, fractional (Double) |
| `LBOUND(a[,d])` | Lowest subscript of an array, of dimension d |
| `UBOUND(a[,d])` | Highest subscript, of dimension d (default 1) |
| `EOF(n)`      | True once file n has been read to the end      |
| `LOF(n)`      | Length of file n in bytes                      |
| `LOC(n)`      | Last record used, or the sequential position   |
| `TAB(n)`      | In PRINT: advance to column n                  |
| `SPC(n)`      | In PRINT: emit n spaces                        |

### Record Conversion Functions

These turn numbers into the fixed-width byte strings a random-access record
holds, and back. See [Random-Access Files](#random-access-files).

| Function   | Description                                     |
|------------|-------------------------------------------------|
| `MKI$(x)`  | An INTEGER as 2 bytes                           |
| `MKL$(x)`  | A LONG as 4 bytes                               |
| `MKS$(x)`  | A SINGLE as 4 bytes                             |
| `MKD$(x)`  | A DOUBLE as 8 bytes                             |
| `CVI(s$)`  | Those 2 bytes back as a number                  |
| `CVL(s$)`  | Those 4 bytes back as a number                  |
| `CVS(s$)`  | Those 4 bytes back as a number                  |
| `CVD(s$)`  | Those 8 bytes back as a number                  |

`TIMER` counts fractional seconds since midnight UTC on every platform, so
subtracting two readings times a section of code.

`LBOUND` and `UBOUND` take an array *name*, of any element type. The
dimension may be any numeric expression; asking for one the array does
not have is an error, reported at compile time when it is a constant and
at run time otherwise.

---

## File I/O

Files come in two flavours: **sequential**, read and written a line at a time,
and **random**, read and written a fixed-length record at a time.

### Opening Files

```basic
OPEN "filename.txt" FOR INPUT AS #1    ' Read mode
OPEN "filename.txt" FOR OUTPUT AS #1   ' Write mode (truncate)
OPEN "filename.txt" FOR APPEND AS #1   ' Write mode (append)
OPEN "data.dat" FOR RANDOM AS #1 LEN = 64   ' Fixed-length records
```

`FOR RANDOM` opens for reading *and* writing, and creates the file when it
does not exist. `LEN =` gives the record length in bytes; without it the record
is 128 bytes, as in GW-BASIC. The largest record is 32767 bytes.

File numbers range from `#1` to `#15`; the runtime has a handle slot for each.
One outside that range is refused at compile time, or reported as
`?Bad file number` when it is not a constant.

### Closing Files

```basic
CLOSE #1          ' Close specific file
CLOSE             ' Close all files
```

A file number may be any numeric expression, not only a literal:

```basic
F% = 1
OPEN "data.txt" FOR INPUT AS #F%
```

### Writing to Files

```basic
PRINT #1, "Hello, File!"
PRINT #1, X; Y; Z
PRINT #1, A$
```

Lines end with the host's terminator -- CRLF on Windows, LF elsewhere --
so `LOF` counts two bytes per line ending on Windows and one elsewhere.
Reading accepts either, so a file written on one platform reads correctly
on the other.

### Reading from Files

```basic
INPUT #1, X           ' Read value
INPUT #1, A$, B$      ' Read multiple values
LINE INPUT #1, Text$  ' Read entire line
```

`INPUT #` reads one comma-delimited field per variable, skipping leading
blanks and line breaks, so several fields may come from one line and one
field may span several. A field wrapped in quotes may contain commas.
`LINE INPUT #` takes a whole line, commas and all.

Reading past the end of a file is an error (`Input past end of file`), as is
opening a file that is not there (`File not found`) or re-using a file number
that is still open (`File already open`). `EOF()` gives the usual
read-until-the-end loop:

```basic
OPEN "data.txt" FOR INPUT AS #1
WHILE NOT EOF(1)
    LINE INPUT #1, Line$
    PRINT Line$
WEND
CLOSE #1
```

### Example

```basic
' Write data
OPEN "data.txt" FOR OUTPUT AS #1
PRINT #1, "John"
PRINT #1, 25
PRINT #1, 50000.00
CLOSE #1

' Read data
OPEN "data.txt" FOR INPUT AS #1
INPUT #1, Name$
INPUT #1, Age%
INPUT #1, Salary#
CLOSE #1

PRINT Name$; " is "; Age%; " years old"
```

---

## Random-Access Files

A random file is a row of fixed-length records, addressed by number. Reading
record 900 costs the same as reading record 1.

### FIELD

`FIELD` names the parts of a record, giving each a width in bytes:

```basic
OPEN "people.dat" FOR RANDOM AS #1 LEN = 32
FIELD #1, 20 AS Name$, 4 AS Age$, 8 AS Pay$
```

Every field variable is a **window onto the record buffer**, not a copy of it.
`GET` overwrites that buffer, so all of them change at once; `LSET` and `RSET`
write through the window, so `PUT` emits what they wrote. A field variable's
length is its declared width and never changes.

The fields must fit inside the record; asking for more is a `FIELD overflow`.

### LSET and RSET

`LSET` and `RSET` overwrite a field in place, padding with spaces to the
field's width. A value too long is truncated on the right:

```basic
LSET Name$ = "Alice"      ' "Alice" followed by 15 spaces
RSET Name$ = "Alice"      ' 15 spaces followed by "Alice"
```

Ordinary assignment does *not* do this. `Name$ = "Alice"` makes `Name$` an
ordinary 5-character string and severs its link to the record buffer, which is
exactly why `LSET` exists.

### GET and PUT

```basic
PUT #1, 3         ' write the buffer as record 3
GET #1, 3         ' read record 3 into the buffer
PUT #1            ' write the record after the last one touched
GET #1            ' read the record after the last one touched
```

Records are numbered from 1. Reading past the end of the file gives a
zero-filled record rather than the previous one. A record buffer starts out
blank, so a field never written reads as spaces.

### Numbers in Records

A record holds bytes, so numbers are converted to and from strings of the
width their type occupies:

| Function   | Width   | Inverse  |
|------------|---------|----------|
| `MKI$(x)`  | 2 bytes | `CVI(s$)` |
| `MKL$(x)`  | 4 bytes | `CVL(s$)` |
| `MKS$(x)`  | 4 bytes | `CVS(s$)` |
| `MKD$(x)`  | 8 bytes | `CVD(s$)` |

```basic
LSET Age$ = MKI$(30)
PRINT CVI(Age$)           ' 30
```

Bytes are stored little-endian, in the machine's own format. A string
narrower than the type it is read as is an `Illegal function call`: padding it
out would turn two stray characters into a plausible-looking number rather
than an error.

### LOC

`LOC(n)` is the last record read or written on a random file. On a sequential
file it is the position in 128-byte blocks, as in GW-BASIC.

### LOCK and UNLOCK

Advisory locks over a record range, for when more than one program has the
file open:

```basic
LOCK #1, 5          ' just record 5
LOCK #1, 5 TO 10    ' a range
LOCK #1             ' the whole file
UNLOCK #1, 5        ' released the same way it was taken
```

These are advisory: they hold against another process that also locks, and do
nothing against one that simply writes. A lock already held by someone else is
reported as `Permission denied`.

### Example

```basic
OPEN "people.dat" FOR RANDOM AS #1 LEN = 32
FIELD #1, 20 AS Name$, 4 AS Age$, 8 AS Pay$

LSET Name$ = "Alice"
LSET Age$ = MKI$(30)
LSET Pay$ = MKD$(50000.5)
PUT #1, 1

GET #1, 1
PRINT RTRIM$(Name$); " is "; CVI(Age$)
CLOSE #1
```

---

## Procedures

### SUB (Subroutines)

Procedures that don't return a value:

```basic
SUB PrintGreeting(Name$)
    PRINT "Hello, "; Name$; "!"
END SUB

' Call the subroutine
PrintGreeting "World"
PrintGreeting("World")     ' Parentheses optional
CALL PrintGreeting("World")  ' CALL is accepted too
```

`CALL` is recognised only before a name at the start of a statement, so a
program may still use it as a variable.

### FUNCTION

Procedures that return a value:

```basic
FUNCTION Square(X)
    Square = X * X
END FUNCTION

FUNCTION Factorial(N)
    IF N <= 1 THEN
        Factorial = 1
    ELSE
        Factorial = N * Factorial(N - 1)
    END IF
END FUNCTION

' Use functions
PRINT Square(5)
PRINT Factorial(10)
```

Return value is assigned to the function name within the function body.

### Parameters

Parameters are passed **by value**:

```basic
SUB Double(X)
    X = X * 2       ' Only affects local copy
    PRINT X
END SUB

A = 5
Double A            ' Prints 10
PRINT A             ' Prints 5 (unchanged)
```

### Recursion

Both SUB and FUNCTION support recursion:

```basic
FUNCTION Fib(N)
    IF N <= 1 THEN
        Fib = N
    ELSE
        Fib = Fib(N - 1) + Fib(N - 2)
    END IF
END FUNCTION
```

---

## Runtime Errors

Compiled programs check for the mistakes that would otherwise corrupt memory
or crash. On failure the program writes a message naming the fault and the
line it happened on to standard error, and exits with status 1:

```
?Subscript out of range in 42
```

Checked: array subscripts (against every dimension, and against the lower
bound when `OPTION BASE 1` is in effect), use of an array before its `DIM` has
run, division by zero for `/`, `\` and `MOD`, a `\` or `MOD` whose quotient
overflows, `SQR` of a negative number, `LOG` of a non-positive number, a file
number outside 1 to 15, `GOSUB` nested deeper than the return stack holds,
allocation failure, a random-access operation on a file not opened `FOR
RANDOM`, `FIELD` widths that overrun the record, a `CV` conversion given too
few bytes, and a `LOCK` another process already holds.

The message names the fault:

| Message                  | Cause                                            |
|--------------------------|--------------------------------------------------|
| `Subscript out of range` | A subscript outside a dimension's bounds         |
| `Array used before DIM`  | An array reached before its `DIM` ran            |
| `Division by zero`       | A zero divisor in `/`, `\` or `MOD`              |
| `Overflow`               | A `\` or `MOD` whose quotient does not fit       |
| `Illegal function call`  | `SQR` of a negative, `LOG` of a non-positive     |
| `Bad file number`        | A file number outside 1 to 15                    |
| `GOSUB stack overflow`   | `GOSUB` nested past the return stack's depth     |
| `Out of memory`          | A string or array allocation that failed         |
| `Bad file mode`          | `FIELD`, `GET` or `PUT` on a non-random file     |
| `FIELD overflow`         | `FIELD` widths exceeding the record length       |
| `Permission denied`      | A `LOCK` someone else already holds              |

Checks are on by default. Compiling with `--unsafe` removes them, which is
worth doing only for code already known to be correct:

```bash
xbasic64 --unsafe program.bas
```

---

## Limitations

A GW-BASIC name this compiler does not provide is **refused, with a reason**,
rather than quietly read as a new variable. That distinction matters: before
the compiler knew these words, `PRINT DATE$` printed an empty string and
`ON ERROR GOTO 100` compiled into a jump on a variable that is always zero, so
a program's error handler never ran and nothing said so.

The reason says whether waiting will help.

### Not Implemented Yet

Each of these is practical on both Linux and Windows and simply has not been
written. Programs using them are refused today.

- **Error trapping** -- `ON ERROR GOTO`, `RESUME`, `RESUME NEXT`, `ERR`, `ERL`, `ERROR`
- **Console control** -- `WIDTH`, `CSRLIN`, `VIEW PRINT`, `INKEY$`, `BEEP`, `SLEEP`
- **Operating system** -- `SHELL`, `ENVIRON$`, `KILL`, `NAME`, `FILES`, `CHDIR`, `MKDIR`, `RMDIR`
- **Odds and ends** -- `INPUT$`, `SHARED`, `STATIC`

### Out of scope

**Graphics** -- `SCREEN`, `PSET`, `PRESET`, `LINE` in its graphics form,
`CIRCLE`, `DRAW`, `PAINT`, `POINT`, `VIEW`, `WINDOW`, `PALETTE`, `PMAP`. These
need a display this compiler does not provide.

**The 8086's machine** -- `PEEK`, `POKE`, `DEF SEG`, `VARPTR`, `VARPTR$`, `USR`,
`INP`, `OUT`, `WAIT`, `BLOAD`, `BSAVE`. There is no fixed address worth naming
in a 64-bit hosted program: the video buffer is not memory, addresses are
randomised, and the pages are protected. A `POKE` that appeared to work would be
the worst outcome available, so these are refused rather than emulated.

**Commands to the interpreter** -- `RUN`, `LIST`, `LOAD`, `SAVE`, `MERGE`,
`NEW`, `EDIT`, `RENUM`, `AUTO`, `CONT`, `DELETE`, `TRON`, `TROFF`, `CLEAR`.
These operate on program text that a compiled program no longer has.

**Program chaining** -- `CHAIN` and `COMMON` need separate compilation.

Everything else GW-BASIC provides that is missing here is listed above as not
yet implemented. Each refused name says which of the two it is.

### Structural

- Single module: no `COMMON`, and no separate compilation
- A `FUNCTION` cannot return a `TYPE` record; pass one to a `SUB` instead
- `PRINT USING` has no file form -- `PRINT #n, USING` is refused

---

## Compatibility Notes

xbasic64 aims for compatibility with GW-BASIC and QuickBASIC with these notable behaviors:

1. **Default type is Double** - Unsuffixed variables are `#` (Double), not Single
2. **Division always returns Double** - Use `\` for integer division
3. **Boolean true is -1** - Comparisons return -1 (true) or 0 (false)
4. **Array indices start at 0** - `DIM A(10)` creates 11 elements (0-10)
5. **String indices are 1-based** - `MID$` and `INSTR` use 1-based positions
6. **Parameters are by-value only** - No `BYREF` support, records included:
   a record argument is passed as the address of the caller's copy, which the
   callee copies into a local, so changes to it do not escape

### Deliberate Differences from GW-BASIC

These are places where a GW-BASIC program will behave differently here. They
are choices, not oversights, and each is checked by a test.

7. **Numbers print bare** - GW-BASIC pads a number with a leading space for the
   sign and a trailing one; `PRINT 42` here writes `42` and nothing else.
   `STR$` matches `PRINT` exactly, so it also omits GW-BASIC's leading space
   and round-trips through `VAL`.

8. **Assignment to an integer truncates** - `A% = 7.9` gives 7, where GW-BASIC
   rounds to 8. `CINT` does round, half to even, as QuickBASIC does, so use
   `A% = CINT(X)` when rounding is what is wanted.

9. **Arrays must be declared** - GW-BASIC gives an undeclared array 11
   elements on first use. Here that is `Array used before DIM`, because the
   implicit version silently hides a typo.

10. **A numeric literal takes no type suffix** - GW-BASIC accepts `1.5#` and
    `100!`; write `CDBL(1.5)` or assign to a suffixed variable instead.

11. **`LOC` on a sequential file counts 128-byte blocks**, which is what
    GW-BASIC reported on MS-DOS. It is preserved for compatibility rather than
    because the number is useful.
