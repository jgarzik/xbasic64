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

Line numbers serve as labels for `GOTO` and `GOSUB` targets. Each number may
appear once; GW-BASIC replaces a line typed twice, but in a file the second is
a mistake, and is refused.

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

A whole number is an INTEGER if it fits 16 bits, a LONG if it fits 32, and a
DOUBLE beyond that. This matters because INTEGER arithmetic is checked at 16
bits: with `X% = 300`, `X% * 300` is `Overflow`, as in QuickBASIC, where
`X% * 300&` is 90000.

A hexadecimal, octal or binary literal of up to 16 bits is an INTEGER, read as
two's complement, so `&HFFFF` is -1 and `&H8000` is -32768; one of up to 32
bits is a LONG, likewise, so `&HFFFFFFFF` is -1. Write `&` after it for a LONG
whatever its size: `&HFFFF&` is 65535.

**Floating-point:**
```basic
A = 3.14
B = .5
C = 1E+10
D = 2.5D-3      ' Double precision
```

A number with a point or an exponent is a DOUBLE.

**Type suffixes** may follow any number, as in GW-BASIC: `%` makes it an
INTEGER, `&` a LONG, `!` a SINGLE and `#` a DOUBLE. A value the type cannot
hold -- `32768%`, `1.5%`, `1E39!` -- is refused when the program is compiled.

```basic
A& = 5&         ' LONG
B! = 2.5!       ' SINGLE: rounded to a SINGLE's precision
C# = 100#       ' DOUBLE
```

A `CONST` is folded in these types too, so `CONST A = 300 * 300` is an
`Overflow`, and `CONST A = 300 * 300&` is 90000.

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

A value converted to an INTEGER or LONG -- by assignment, `INPUT`, `READ`, an
argument, a subscript, a string position, or as an operand of `\`, `MOD` or a
bitwise operator -- **rounds half to even**, QuickBASIC's rule and what `CINT`
does: `A% = 7.9` gives 8, `A% = 2.5` gives 2 and `A% = 3.5` gives 4.

Every result that does not fit its type is `Overflow` (error 6), as in
GW-BASIC:

- INTEGER arithmetic is checked at 16 bits and LONG at 32: with `X% = 300`,
  `X% * X%` is `Overflow`, not 90000 or a wrapped value
- a value converted to an INTEGER or LONG that does not fit it, so
  `A% = 40000` and `CINT(40000)` are both `Overflow`
- a SINGLE or DOUBLE result too large for its type, such as `1E308 * 10` or
  `EXP(1000)`, rather than an infinity

`--unsafe` removes these checks with the others, and the results then wrap.

### Division Semantics

Following GW-BASIC conventions:
- `/` (division) always produces a **Double** result
- `\` (integer division) produces a **Long** result; its operands are rounded
  to LONG first, so `7.6 \ 2` is `8 \ 2`
- `0 ^` a negative number is `Division by zero`, as in GW-BASIC

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

**Array indices start at 0** by default. A dimension may name its own lower
bound, as in QuickBASIC:

```basic
DIM Year(1980 TO 1989)   ' 10 elements, 1980 to 1989
DIM Grid(-5 TO 5, 1 TO 3)
```

An array no `DIM` declares is **created by its first use**, as in GW-BASIC:
with as many dimensions as that use has subscripts, each running from the
`OPTION BASE` to 10. The same happens to a declared array used before its
`DIM` has run, or after `ERASE`. A `DIM` of an array that already exists -- by
an earlier `DIM`, or by a use -- is `Duplicate definition` (10); `REDIM` may
always resize one. Since a mistyped function would quietly become such an
array, a name close to a function or procedure draws a warning.

Arrays can hold any type:
```basic
DIM Names$(100)          ' String array
DIM Values%(50)          ' Integer array
```

### Scope

Scope follows QuickBASIC:

- **Module-level names stay at module level.** A `SUB` or `FUNCTION` sees one
  only when it is shared -- by `DIM SHARED` at module level, for every
  procedure, or by a `SHARED` statement inside the procedure, for that one.
- **A procedure's other names are its own**, and start fresh (0 or `""`) on
  every call, so recursion works -- even when the module has a name the same.
- **`STATIC`** inside a procedure keeps the names it lists from one call to the
  next; `SUB name STATIC` (or `FUNCTION`) does that for every local.
- **A parameter**, and a `FUNCTION`'s return pseudo-variable, are always local.
- **`CONST`s and `DEF FN`** belong to the whole program; a `DEF FN` sees every
  module-level name.

```basic
DIM SHARED Total          ' every procedure sees Total
Count = 5                 ' only the module does
SUB Add(N)
    SHARED Count          ' ...and now this procedure too
    STATIC Calls          ' kept between calls
    Calls = Calls + 1
    Total = Total + N
END SUB
```

`SHARED` lists names, and `A()` for an array: `SHARED X, Names$(), R AS P`.
`DIM SHARED` takes the same forms as `DIM`, and a plain name -- `DIM SHARED X`
-- shares a scalar. Every procedure used to see every name the module used.

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
manipulation. Their operands are converted to LONG first -- rounded, and
`Overflow` outside its range -- and the result is a LONG:

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
3. `*`, `/`
4. `\` (integer division)
5. `MOD`
6. `+`, `-`
7. `=`, `<>`, `<`, `>`, `<=`, `>=`
8. `NOT`
9. `AND`
10. `OR`
11. `XOR`
12. `EQV`
13. `IMP`

This is GW-BASIC's and QuickBASIC's table. Because `^` binds tighter than unary
negation, `-2 ^ 2` is `-(2 ^ 2)` = -4. Because `\` and `MOD` sit below `*`,
`6 \ 2 * 3` is `6 \ 6` = 1, and `10 - 7 MOD 4` is 7.

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
PRINT X; Y; Z             ' Semicolon: items adjoin
PRINT A, B, C             ' Comma: next 14-column print zone
PRINT "Value: "; X
PRINT                     ' Print blank line
```

**A number carries its own spacing**, as in GW-BASIC: a blank where the minus
sign would go if it is not negative, and a blank after it. So `PRINT 1; 2; 3`
writes ` 1  2  3 `, and `PRINT 1; -2; 3` writes ` 1 -2  3 ` -- the minus takes
the leading blank's place. A string is written exactly as it is.

An INTEGER or LONG is written in full. A SINGLE or DOUBLE is rounded to the
digits its type carries -- 7 significant digits for a SINGLE, 16 for a DOUBLE -- and
written in fixed point when that shows it in at most that many digits, as
GW-BASIC and QuickBASIC do. Otherwise it gets an exponent, spelled `E` for a
SINGLE and `D` for a DOUBLE, with at least two digits:

```basic
PRINT .1 + .2       ' .3
PRINT 1 / 3         ' .3333333333333333
X! = 1.1E-6         ' .0000011: seven digits after the point
X! = 1.1E-7         ' 1.1E-07: eight would be needed
X! = 12345678       ' 1.234568E+07
PRINT 1E16          ' 1D+16
```

Trailing zeros are dropped, and a value below one has no leading zero (`.5`,
not `0.5`). `STR$` gives exactly what `PRINT` writes.

A comma moves to the start of the next 14-column zone, padding with blanks.

`WIDTH w` sets the line length console output wraps at, and `WIDTH #n, w` that
of file `n`; `w` is 1 to 255, and 255 means no limit, which is where every
program and every newly opened file starts. Under a width, text that runs past
the end of the line continues on the next, a number that does not fit on the
rest of the line moves to the next one whole, and a comma whose zone would not
fit starts a new line -- as in GW-BASIC, whose screen started at 80.

```basic
WIDTH 40
PRINT STRING$(50, "*")    ' 40 stars, then 10 on the next line
``` `WRITE` and
`PRINT USING` are unaffected -- `WRITE` pads nothing, because its output is
meant to be read back by `INPUT`, and `PRINT USING` lays out its own.

Semicolon at end suppresses newline:
```basic
PRINT "Enter value: ";
```

### PRINT USING

Formatted output. The format is any string expression, read as the statement
runs, and `PRINT #n, USING` writes to a file:

```basic
PRINT USING "###.##"; 3.14159       '   3.14
PRINT USING "Total: ###"; 42        ' Total:  42
PRINT USING "## and ##"; 1; 2       '   1 and  2
F$ = "$$#,###.##"
PRINT USING F$; 1234.5              '  $1,234.50
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
exhausted, the format restarts; after the last value, its text is printed up
to the next field. A value of the wrong kind for its field is `Type mismatch`
(13), and a format with no field for a value is `Illegal function call`.

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

`INPUT` reads one line and splits it on commas, as GW-BASIC does: `INPUT A, B$`
takes `3, hello` from a single line. A field may be quoted, which lets it hold
a comma or keep its blanks; an unquoted one has its surrounding blanks dropped.
A number is read as `VAL` reads one, and an empty field is 0 or `""`.

Each field must suit its variable before any is stored. Text where a number
belongs, a number too large for the variable (`40000` for an INTEGER), a quoted
number, or too few fields prints `?Redo from start`, and the statement asks
again, prompt and all. More fields than variables prints `?Extra ignored`.
When the input has ended, every variable reads as empty.

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

A branch stays in its own scope -- module-level code, or one `SUB` or
`FUNCTION` -- for every form: `GOTO`, `GOSUB`, `ON ... GOTO`, `ON ... GOSUB`.
Each scope has its own frame, so code reached across the boundary would run with
the wrong one. `RESTORE` is the exception: it names a place in the `DATA`, which
is one list for the whole program.

### GOSUB / RETURN

Call subroutine and return:

```basic
GOSUB 1000
PRINT "Back from subroutine"
END

1000 PRINT "In subroutine"
1010 RETURN
```

A `RETURN` goes back only to a `GOSUB` made in the same scope: main's
`RETURN` to main's `GOSUB`, a procedure's to its own. Reaching one with no such
`GOSUB` outstanding -- typically by falling into a subroutine -- is the
runtime error `RETURN without GOSUB`.

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

The selector is rounded to an integer, as any conversion is. A value that
matches nothing -- zero, or past the end of the list -- runs no subroutine and
continues with the next statement. As in GW-BASIC, the selector must fit a
byte: a negative one, or one above 255, is `Illegal function call`.

### ON ERROR GOTO

`ON ERROR GOTO n` installs an error handler. From then on a runtime error
jumps to line `n` instead of stopping the program, with `ERR` holding the
error's number and `ERL` the line it happened on.

```basic
10 ON ERROR GOTO 900
20 OPEN "data.txt" FOR INPUT AS #1
30 CLOSE #1
40 PRINT "read it"
50 END
900 PRINT "cannot open it: error"; ERR; "at line"; ERL
910 END
```

`ON ERROR GOTO 0` puts the fatal path back:

```basic
10 ON ERROR GOTO 900
20 ON ERROR GOTO 0
30 END
900 END
```

The rules, which are GW-BASIC's:

- An error raised **inside the handler** is not trapped -- it stops the
  program. Without that, a handler that faults would call itself forever.
- Trapping stays suspended while the handler runs. Executing an `ON ERROR`
  statement again re-arms it, so a handler that ends in `GOTO` rather than
  returning must re-arm to keep trapping.
- The handler must be **module-level** code, and `ON ERROR` itself may not
  appear inside a `SUB` or `FUNCTION`: a trapped error unwinds out of every
  procedure before the handler runs.
- A `GOSUB` in progress survives, so a handler can `RETURN` from a subroutine
  the error interrupted. For the same reason a `GOSUB` *inside* a procedure is
  refused in a program that traps -- its return address would point into a
  frame the unwind discarded.
- `--unsafe` and `ON ERROR` are refused together. `--unsafe` removes the checks
  that raise most trappable errors, so the handler would look right and never
  run.

A program that never uses `ON ERROR` is compiled exactly as before, and pays
nothing for the feature.

### RESUME

`RESUME` ends a handler and goes back to the program. Three forms:

| Form           | Goes back to                                    |
|----------------|-------------------------------------------------|
| `RESUME`       | The statement that failed, to try it again      |
| `RESUME NEXT`  | The statement after the one that failed         |
| `RESUME n`     | Line `n`, or a named label                      |

`RESUME 0` means the same as a bare `RESUME`.

```basic
10 ON ERROR GOTO 200
20 Divisor = 0
30 Tries = Tries + 1 : R = 100 / Divisor
40 PRINT "took"; Tries; "tries, got"; R
50 END
200 Divisor = 4
210 RESUME
```

The statement, not the line: on line 30 above, `RESUME` returns to
`R = 100 / Divisor` and leaves `Tries` alone, and `RESUME NEXT` would carry on
at line 40. Where the failing statement is the last in a `FOR` or `WHILE` body,
`RESUME NEXT` continues the loop rather than leaving it.

Two rules follow from where a handler runs:

- `RESUME` must be module-level code, like the handler it ends.
- After an error raised **inside a `SUB` or `FUNCTION`**, a bare `RESUME` or
  `RESUME NEXT` stops the program with `RESUME cannot return into a SUB or
  FUNCTION`: the unwind discarded that frame, so there is nothing to go back
  to. `RESUME n` still works, and is the way out.

`RESUME` outside a handler stops the program with `RESUME without error`.
Neither of these two is trappable -- a handler that caught its own failing
`RESUME` would be re-entered by it forever.

### DIM

Declare arrays:

```basic
DIM A(100)           ' 1D array, indices 0-100
DIM B(10, 20)        ' 2D array
DIM C(5, 5, 5)       ' 3D array
DIM Names$(50)       ' String array
DIM D(1 TO 12)       ' indices 1-12
```

`LBOUND(A, n)` and `UBOUND(A, n)` give dimension `n`'s bounds; `n` defaults
to 1.

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

What an item is depends on what reads it, as in GW-BASIC. `READ` into a string
gives the item as written -- `DATA 007` reads as `"007"`, and a quoted item
without its quotes. `READ` into a number reads the item as `VAL` does, so
`&HFF` and `1D3` work, but the whole item must be a number: a quoted item, or
text that is not a number, is `Syntax error` (2), and an empty item is 0.
`READ` past the last item is `Out of DATA` (4).

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

Colours are GW-BASIC's: 0 black, 1 blue, 2 green, 3 cyan, 4 red, 5 magenta,
6 brown, 7 white, and 8-15 the bright half of each. A foreground of 16-31 is
the same colour blinking. A background is 0-15. GW-BASIC's third `COLOR`
argument sets the border, which a terminal has no equivalent for, and is
refused rather than ignored.

An omitted argument leaves that setting as it was, for both statements. A
value out of range, or a row or column below 1, is `Illegal function call`.

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

It is the lower bound of every dimension that does not name its own.

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
procedure, by reference like any other variable, or by value with `BYVAL`:

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
STOP    ' Report where, then terminate
```

`STOP` writes `Break in` and the line number to standard error, as GW-BASIC
does -- the source line when the program has no line numbers -- and then ends
the program normally, with exit status 0. GW-BASIC would return to its prompt
and allow `CONT`; a compiled program has neither.

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

**Numeric output:** `PRINT` shows 16 significant digits of a `DOUBLE`
(`PRINT 1 / 3` gives `.3333333333333333`) and 7 of a `SINGLE`; see
[PRINT](#print).

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
| `STR$(x)`             | The number as PRINT writes it, less the trailing blank |
| `SPACE$(n)`           | A string of n spaces                           |
| `STRING$(n, c)`       | n copies of a character (code or first of c$)  |
| `LTRIM$(s$)`          | Drop leading spaces                            |
| `RTRIM$(s$)`          | Drop trailing spaces                           |
| `UCASE$(s$)`          | Convert to upper case                          |
| `LCASE$(s$)`          | Convert to lower case                          |
| `HEX$(n)`             | Hexadecimal text for an integer                |
| `OCT$(n)`             | Octal text for an integer                      |

**String indexing is 1-based** for `MID$` and `INSTR`.

`VAL` reads the number a string begins with, as GW-BASIC does: blanks are
ignored wherever they are (`VAL("1 2")` is 12), an exponent may be spelled `E`
or `D`, `&H` and `&O` work as they do in a literal (`VAL("&HFFFF")` is -1), and
the number ends where the text stops being one (`VAL("12abc")` is 12). Text
that does not begin with a number is 0; one too large for a DOUBLE is
`Overflow`.

`HEX$` and `OCT$` show a negative number as two's complement in its own type's
width, as QuickBASIC does: `HEX$(-1)` is `FFFF`, since -1 is an INTEGER, and
`HEX$(-1&)` is `FFFFFFFF`. Any other type is converted to a LONG first.

`MID$` may also be assigned to, overwriting characters in place. The target's
length never changes:

```basic
A$ = "hello"
MID$(A$, 1, 1) = "J"      ' A$ is now "Jello"
```

### Error Functions

| Function | Returns                                                      |
|----------|--------------------------------------------------------------|
| `ERR`    | The number of the trapped error, 0 before any is trapped      |
| `ERL`    | The line it happened on, 0 if the program has no line numbers |

Both are only meaningful inside an `ON ERROR` handler. `ERL` reports the BASIC
line number, as the error messages do.

`STR$` keeps the blank PRINT puts where a minus sign would go, so `STR$(5)`
is `" 5"` and `STR$(-5)` is `"-5"`. `MID$(STR$(N), 2)` is the usual way to drop
it.

### Type Conversion Functions

| Function   | Description                              |
|------------|------------------------------------------|
| `CINT(x)`  | Convert to Integer, rounding half to even |
| `CLNG(x)`  | Convert to Long, rounding half to even    |
| `CSNG(x)`  | Convert to Single                        |
| `CDBL(x)`  | Convert to Double                        |

### Other Functions

| Function      | Description                                    |
|---------------|------------------------------------------------|
| `TIMER`       | Seconds since midnight, local time, fractional (Double) |
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

`TIMER` counts fractional seconds since local midnight -- the clock `TIME$`
reads, through the same C library, so both honour `TZ` -- and subtracting two
readings times a section of code.

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
RESET             ' Close all files, as GW-BASIC spells it
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
field may span several. A field wrapped in quotes may contain commas. A numeric
field is read as `VAL` reads a string. `LINE INPUT #` takes a whole line,
commas and all.

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

Parameters are passed **by reference**, as in QuickBASIC: a variable, array
element or record field given as an argument is changed by what the procedure
does to its parameter. `BYVAL` passes a copy instead, and an argument that is
any other expression -- `A + 0`, `CINT(B#)`, a literal -- is a copy whatever
the parameter says.

```basic
SUB Double(X)
    X = X * 2
END SUB
SUB Show(BYVAL X)
    X = 0           ' only the copy
END SUB

A = 5
Double A            ' A is now 10
Show A              ' A is still 10
Double A + 0        ' an expression: A is still 10
```

A variable passed by reference must be the parameter's type -- an INTEGER
variable for an INTEGER parameter -- as QuickBASIC requires; to pass another
type, pass an expression, which is converted.

An **array parameter** is written `name()`, and takes an array, written the
same way, by reference: the procedure works on the caller's elements and
bounds, and a `REDIM` or `ERASE` in it reaches the caller. It has as many
dimensions as the procedure subscripts it with.

```basic
SUB Fill(V(), N)
    FOR I = LBOUND(V) TO UBOUND(V): V(I) = N: NEXT I
END SUB
DIM Scores(10)
Fill Scores(), 1
```

`DECLARE SUB name (params)` and `DECLARE FUNCTION name (params) [AS type]`
state a procedure's signature ahead of it, as QuickBASIC modules do; each is
checked against the definition.

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

The number is the **BASIC line number**, as in GW-BASIC -- the number the
listing itself branches to. A program written without line numbers has none to
quote, so it reports the source line instead, which is the only number its
author can act on. A statement ahead of the first line number does the same.

Checked: array subscripts (against every dimension, and against the lower
bound), a `DIM` of an array that already exists, division by zero for `/`, `\` and `MOD`, every arithmetic result and
conversion that does not fit its type, `SQR` of a negative number, `LOG` of a
non-positive number, a file number outside 1 to 15, `GOSUB` nested deeper than
the return stack holds, a `RETURN` with no `GOSUB` of its own scope to go back
to, `READ` past the last `DATA` item, allocation failure, each way `OPEN` can
fail, a file used against the mode it was opened in, `FIELD` widths that
overrun the record, a `CV` conversion given too few bytes, and a `LOCK` another
process already holds.

A comparison involving NaN follows IEEE: it is false, except `<>`, which is
true. Arithmetic never produces one in a checked program -- each way to is an
error -- so this matters under `--unsafe`, or for a NaN read from outside.

The message names the fault, and each carries GW-BASIC's number for it:

| No. | Message                  | Cause                                        |
|-----|--------------------------|----------------------------------------------|
| 2   | `Syntax error`           | `READ` of text that is not a number into a numeric variable |
| 3   | `RETURN without GOSUB`   | A `RETURN` no `GOSUB` of its scope is waiting for |
| 4   | `Out of DATA`            | A `READ` after the last `DATA` item          |
| 5   | `Illegal function call`  | `SQR` of a negative, `LOG` of a non-positive, an argument out of range |
| 6   | `Overflow`               | A result or conversion that does not fit its type |
| 7   | `Out of memory`          | A string or array allocation that failed     |
| 7   | `GOSUB stack overflow`   | `GOSUB` nested past the return stack's depth |
| 9   | `Subscript out of range` | A subscript outside a dimension's bounds     |
| 10  | `Duplicate definition`   | A `DIM` of an array that already exists      |
| 11  | `Division by zero`       | A zero divisor in `/`, `\` or `MOD`          |
| 13  | `Type mismatch`          | A `PRINT USING` value of the wrong kind for its field |
| 50  | `FIELD overflow`         | `FIELD` widths exceeding the record length   |
| 52  | `Bad file number`        | A file number outside 1 to 15, or not open   |
| 53  | `File not found`         | Opening a file for `INPUT` that is not there |
| 54  | `Bad file mode`          | Writing a file opened for reading, reading one opened for writing, or `FIELD`, `GET` or `PUT` on a non-random file |
| 55  | `File already open`      | `OPEN` on a file number already in use       |
| 62  | `Input past end of file` | Reading past the end of an input file        |
| 64  | `Bad file name`          | `OPEN` of an empty or malformed name         |
| 70  | `Permission denied`      | A file the program may not open that way, or a `LOCK` someone else holds |
| 75  | `Path/File access error` | Any other `OPEN` failure, such as a directory |
| 76  | `Path not found`         | Creating a file in a directory that is not there |

### ERROR

`ERROR n` raises the error numbered `n`, exactly as if the runtime had raised
it. The number must be 1 to 255; one the table above does not list is raised as
`Unprintable error`, which is GW-BASIC's own wording.

```basic
10 IF Total < 0 THEN ERROR 5
20 PRINT "fine"
```

Two numbers appear twice above. `ERROR 7` and `ERROR 9` raise the first message
listed for each -- `Out of memory` and `Subscript out of range`.

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

- **Console control** -- `CSRLIN`, `VIEW PRINT`, `INKEY$`, `SLEEP`, `KEY`
- **Operating system** -- `SHELL`, `ENVIRON$`, `KILL`, `NAME`, `FILES`, `CHDIR`, `MKDIR`, `RMDIR`
- **Sound and printer** -- `SOUND`, `PLAY`, `LPRINT`, `LPOS`
- **Odds and ends** -- `INPUT$`

### Out of scope

**Graphics** -- `SCREEN`, `PSET`, `PRESET`, `LINE` in its graphics form,
`CIRCLE`, `DRAW`, `PAINT`, `POINT`, `VIEW`, `WINDOW`, `PALETTE`, `PMAP`. These
need a display this compiler does not provide.

**The 8086's machine** -- `PEEK`, `POKE`, `DEF SEG`, `VARPTR`, `VARPTR$`,
`VARSEG`, `USR`, `INP`, `OUT`, `WAIT`, `BLOAD`, `BSAVE`, `PCOPY`. There is no fixed address worth naming
in a 64-bit hosted program: the video buffer is not memory, addresses are
randomised, and the pages are protected. A `POKE` that appeared to work would be
the worst outcome available, so these are refused rather than emulated.

**Commands to the interpreter** -- `RUN`, `LIST`, `LOAD`, `SAVE`, `MERGE`,
`NEW`, `EDIT`, `RENUM`, `AUTO`, `CONT`, `DELETE`, `TRON`, `TROFF`, `CLEAR`.
These operate on program text that a compiled program no longer has.

**Program chaining** -- `CHAIN` and `COMMON` need separate compilation.

**MS-DOS devices** -- `ERDEV`, `ERDEV$`, `IOCTL`, `IOCTL$`, `MOTOR`, the
light pen (`PEN`) and joysticks (`STICK`, `STRIG`).

**Event trapping** -- `ON TIMER`, `ON KEY`, `ON COM`, `ON PEN`, `ON PLAY`,
`ON STRIG` and their `ON`/`OFF`/`STOP` switches. These would interrupt the
program between statements, which compiled code has no point to do.

Everything else GW-BASIC provides that is missing here is listed above as not
yet implemented. Each refused name says which of the two it is.

### Structural

- Single module: no `COMMON`, and no separate compilation
- A `FUNCTION` cannot return a `TYPE` record; pass one to a `SUB` instead

---

## Compatibility Notes

xbasic64 aims for compatibility with GW-BASIC and QuickBASIC with these notable behaviors:

1. **Default type is Double** - Unsuffixed variables are `#` (Double), not Single
2. **Division always returns Double** - Use `\` for integer division
3. **Boolean true is -1** - Comparisons return -1 (true) or 0 (false)
4. **Array indices start at 0** - `DIM A(10)` creates 11 elements (0-10)
5. **String indices are 1-based** - `MID$` and `INSTR` use 1-based positions
6. **Parameters are by reference**, as in QuickBASIC; `BYVAL` passes a copy

### Deliberate Differences from GW-BASIC

These are places where a GW-BASIC program will behave differently here. They
are choices, not oversights.

7. **`LOC` on a sequential file counts 128-byte blocks**, which is what
    GW-BASIC reported on MS-DOS. It is preserved for compatibility rather than
    because the number is useful.
