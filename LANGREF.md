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

### Statement Separators

Multiple statements can appear on one line separated by colons:

```basic
A = 1 : B = 2 : PRINT A + B
```

### Identifiers

Variable and procedure names:
- Start with a letter (A-Z, a-z)
- Followed by letters, digits, or underscore
- Case-insensitive (`MyVar` and `MYVAR` are the same)
- May end with a type suffix (`%`, `&`, `!`, `#`, `$`)

### Literals

**Integers:**
```basic
42          ' Decimal
-17         ' Negative
&HFF        ' Hexadecimal (255)
&O377       ' Octal (255)
```

**Floating-point:**
```basic
3.14
.5
1E+10
2.5D-3      ' Double precision
```

**Strings:**
```basic
"Hello, World!"
"She said ""Hi"""   ' Embedded quote
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

**Unsuffixed numeric variables default to DOUBLE (`#`).**

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

- **Global by default**: Variables declared at module level are accessible everywhere
- **Local in procedures**: Variables declared inside `SUB` or `FUNCTION` are local to that procedure
- Parameters are local to their procedure

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

### Logical Operators

| Operator | Description           |
|----------|-----------------------|
| `AND`    | Bitwise/logical AND   |
| `OR`     | Bitwise/logical OR    |
| `XOR`    | Bitwise/logical XOR   |
| `NOT`    | Bitwise/logical NOT   |

These operate bitwise on integers, allowing both logical tests and bit manipulation:

```basic
IF A > 0 AND B > 0 THEN PRINT "Both positive"
Flags% = Flags% OR &H01    ' Set bit 0
```

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
Array(5) = 42
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

### INPUT

Read user input:

```basic
INPUT X                   ' Prompt with "? "
INPUT "Enter name: ", N$  ' Custom prompt
INPUT "X, Y: ", X, Y      ' Multiple values
```

### LINE INPUT

Read entire line as string (no parsing):

```basic
LINE INPUT "Enter text: ", Text$
```

### IF...THEN...ELSE

**Single-line form:**
```basic
IF X > 0 THEN PRINT "Positive"
IF X > 0 THEN Y = 1 ELSE Y = 0
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

The loop variable name after `NEXT` is optional:
```basic
FOR I = 1 TO 10
    PRINT I
NEXT
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

### ON...GOTO

Computed jump:

```basic
ON Choice GOTO 100, 200, 300
' If Choice=1, goto 100; if Choice=2, goto 200; etc.
```

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
DATA 10, 20, 30, "Hello", "World"

READ A, B, C
READ X$, Y$

RESTORE          ' Reset data pointer to beginning
RESTORE 100      ' Reset to DATA at line 100
```

### CLS

Clear screen:

```basic
CLS
```

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

**RND behavior:**
```basic
X = RND           ' Next random number
X = RND(0)        ' Same as RND
X = RND(-1)       ' Reseed with system time (implementation-defined)
```

### String Functions

| Function              | Description                                    |
|-----------------------|------------------------------------------------|
| `LEN(s$)`             | Length of string                               |
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

**String indexing is 1-based** for `MID$` and `INSTR`.

### Type Conversion Functions

| Function   | Description                              |
|------------|------------------------------------------|
| `CINT(x)`  | Convert to Integer (with rounding)       |
| `CLNG(x)`  | Convert to Long (with rounding)          |
| `CSNG(x)`  | Convert to Single                        |
| `CDBL(x)`  | Convert to Double                        |

### Other Functions

| Function   | Description                              |
|------------|------------------------------------------|
| `TIMER`    | Seconds since midnight (Double)          |

---

## File I/O

### Opening Files

```basic
OPEN "filename.txt" FOR INPUT AS #1    ' Read mode
OPEN "filename.txt" FOR OUTPUT AS #1   ' Write mode (truncate)
OPEN "filename.txt" FOR APPEND AS #1   ' Write mode (append)
```

File numbers range from `#1` to `#255`.

### Closing Files

```basic
CLOSE #1          ' Close specific file
CLOSE             ' Close all files
```

### Writing to Files

```basic
PRINT #1, "Hello, File!"
PRINT #1, X; Y; Z
PRINT #1, A$
```

### Reading from Files

```basic
INPUT #1, X           ' Read value
INPUT #1, A$, B$      ' Read multiple values
LINE INPUT #1, Text$  ' Read entire line
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

## Procedures

### SUB (Subroutines)

Procedures that don't return a value:

```basic
SUB PrintGreeting(Name$)
    PRINT "Hello, "; Name$; "!"
END SUB

' Call the subroutine
PrintGreeting "World"
PrintGreeting("World")    ' Parentheses optional
```

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

## Limitations

The following features are **not supported**:

### Graphics and Sound
- `SCREEN`, `PSET`, `LINE`, `CIRCLE`, `PAINT`, `DRAW`
- `COLOR`, `PALETTE`
- `BEEP`, `SOUND`, `PLAY`

### Memory Access
- `PEEK`, `POKE`
- `DEF SEG`
- `VARPTR`, `VARSEG`

### Error Handling
- `ON ERROR GOTO`
- `RESUME`, `RESUME NEXT`
- `ERR`, `ERL`

### Other
- `DEF FN` (use `FUNCTION` instead)
- `DEFINT`, `DEFSNG`, etc. (use type suffixes)
- `COMMON`, `SHARED` (single-module only)
- `REDIM` (dynamic array resizing)
- Random-access file I/O (`OPEN FOR RANDOM`, `GET`, `PUT`)
- `LOCATE`, `PRINT USING`
- `WIDTH`, `LPRINT`

---

## Compatibility Notes

xbasic64 aims for compatibility with GW-BASIC and QuickBASIC with these notable behaviors:

1. **Default type is Double** - Unsuffixed variables are `#` (Double), not Single
2. **Division always returns Double** - Use `\` for integer division
3. **Boolean true is -1** - Comparisons return -1 (true) or 0 (false)
4. **Array indices start at 0** - `DIM A(10)` creates 11 elements (0-10)
5. **String indices are 1-based** - `MID$` and `INSTR` use 1-based positions
6. **Parameters are by-value only** - No `BYREF` support
