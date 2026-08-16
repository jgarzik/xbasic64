' ===========================================================================
' xbasic64 megatest -- one program exercising most of the supported language.
'
' Copyright (c) 2025-2026 Jeff Garzik
' SPDX-License-Identifier: MIT
'
' Why one enormous program rather than more small ones: the per-feature suites
' each compile a program containing almost nothing else, so they prove that a
' feature works in isolation and nothing more. Real GW-BASIC listings are not
' isolated. This one program holds records, arrays, procedures, DATA, GOSUB
' targets, sequential files and random files at the same time, so it also
' exercises the things that only go wrong when they coexist: global versus
' local storage, the static layout of .bss, a frame big enough to matter, and
' the interaction of the DATA pointer with control flow that jumps around it.
'
' The program is its own assertion harness. Check compares two strings and
' records a pass or a failure, so a mismatch names the case and prints both
' values rather than making the Rust side diff two walls of output. Everything
' is compared as text, via STR$, which keeps the expectations independent of
' how many spaces PRINT happens to put around a number.
'
' Arrays here use the default lower bound of 0. OPTION BASE may appear only
' once in a program and must precede every DIM, so it cannot share a program
' with base-0 tests; tests/arrays covers it separately.
' ===========================================================================

' ---------------------------------------------------------------------------
' Types
' ---------------------------------------------------------------------------
TYPE Point
    X AS INTEGER
    Y AS INTEGER
END TYPE

TYPE Person
    Name AS STRING * 20
    Home AS Point
END TYPE

Passed = 0
Failed = 0

' ---------------------------------------------------------------------------
' Literals and numeric bases
' ---------------------------------------------------------------------------
Check "hex", STR$(&HFF), "255"
Check "hex lower", STR$(&Hff), "255"
Check "octal &O", STR$(&O377), "255"
Check "octal bare", STR$(&377), "255"
Check "binary", STR$(&B1010), "10"
Check "decimal", STR$(42), "42"
Check "negative", STR$(-17), "-17"
Check "leading dot", STR$(.5), "0.5"
Check "exponent", STR$(1E3), "1000"
Check "double exp", STR$(2.5D3), "2500"

' ---------------------------------------------------------------------------
' Arithmetic, division semantics and precedence
' ---------------------------------------------------------------------------
Check "add", STR$(2 + 3), "5"
Check "sub", STR$(10 - 4), "6"
Check "mul", STR$(6 * 7), "42"
Check "real divide", STR$(7 / 2), "3.5"
Check "integer divide", STR$(7 \ 2), "3"
Check "integer divide neg", STR$(-7 \ 2), "-3"
Check "mod", STR$(7 MOD 3), "1"
Check "power", STR$(2 ^ 10), "1024"
Check "unary minus binds looser than ^", STR$(-2 ^ 2), "-4"
Check "parens override", STR$((2 + 3) * 4), "20"
Check "precedence", STR$(2 + 3 * 4), "14"

' ---------------------------------------------------------------------------
' Comparisons and the -1 / 0 booleans
' ---------------------------------------------------------------------------
Check "true is -1", STR$(1 = 1), "-1"
Check "false is 0", STR$(1 = 2), "0"
Check "not equal", STR$(1 <> 2), "-1"
Check "less", STR$(1 < 2), "-1"
Check "less or equal", STR$(2 <= 2), "-1"
Check "greater", STR$(3 > 2), "-1"
Check "greater or equal", STR$(2 >= 3), "0"
Check "string equal", STR$("ab" = "ab"), "-1"
Check "string orders by character", STR$("ab" < "abc"), "-1"
Check "string compare is case sensitive", STR$("A" = "a"), "0"

' ---------------------------------------------------------------------------
' Logical operators, which are bitwise
' ---------------------------------------------------------------------------
Check "and", STR$(12 AND 10), "8"
Check "or", STR$(12 OR 10), "14"
Check "xor", STR$(12 XOR 10), "6"
Check "not", STR$(NOT 0), "-1"
Check "and as a test", STR$(1 = 1 AND 2 = 2), "-1"
Check "or as a test", STR$(1 = 2 OR 2 = 2), "-1"
Flags% = 0
Flags% = Flags% OR &H05
Check "bit set", STR$(Flags%), "5"

' ---------------------------------------------------------------------------
' Types, coercion and the default DOUBLE
' ---------------------------------------------------------------------------
I% = 300
L& = 100000
S! = 1.5
D# = 1.0 / 3.0
T$ = "text"
Check "integer", STR$(I%), "300"
Check "long", STR$(L&), "100000"
Check "single", STR$(S!), "1.5"
Check "double keeps its digits", STR$(D#), "0.3333333333333333"
Check "string", T$, "text"
Check "unsuffixed is double", STR$(1 / 3), "0.3333333333333333"
Check "integer truncates on store", STR$(CINT(2.6)), "3"
Check "widening in an expression", STR$(I% + S!), "301.5"
Check "CLNG", STR$(CLNG(2.5)), "2"
Check "CSNG", STR$(CSNG(1.5)), "1.5"
Check "CDBL", STR$(CDBL(2)), "2"
DIM DeclaredInt AS INTEGER
DeclaredInt = 7.9
Check "DIM AS INTEGER truncates", STR$(DeclaredInt), "7"

' ---------------------------------------------------------------------------
' CONST
' ---------------------------------------------------------------------------
CONST MAXN = 10
CONST HALFN = MAXN / 2
CONST TITLE$ = "Report"
Check "const", STR$(MAXN), "10"
Check "const folds", STR$(HALFN), "5"
Check "const string", TITLE$, "Report"

' ---------------------------------------------------------------------------
' Arrays: several ranks, string elements, bounds, REDIM
' ---------------------------------------------------------------------------
DIM Nums(MAXN)
DIM Grid(3, 3)
DIM Cube(2, 2, 2)
DIM Names$(4)
DIM Typed%(4)

FOR I = 0 TO MAXN
    Nums(I) = I * I
NEXT I
Check "array element", STR$(Nums(4)), "16"
Check "array last element", STR$(Nums(MAXN)), "100"
Check "LBOUND", STR$(LBOUND(Nums)), "0"
Check "UBOUND", STR$(UBOUND(Nums)), "10"

Grid(2, 3) = 23
Check "2D array", STR$(Grid(2, 3)), "23"
Check "UBOUND dimension 2", STR$(UBOUND(Grid, 2)), "3"
Cube(1, 1, 1) = 111
Check "3D array", STR$(Cube(1, 1, 1)), "111"

Names$(0) = "zero"
Names$(3) = "three"
Check "string array", Names$(3), "three"
Check "string array untouched", Names$(1), ""

Typed%(2) = 7.6
Check "typed array truncates", STR$(Typed%(2)), "7"

DIM Growable(2)
Growable(0) = 1
Growable(1) = 2
Growable(2) = 3
REDIM PRESERVE Growable(5)
Check "REDIM PRESERVE keeps", STR$(Growable(1)), "2"
Check "REDIM PRESERVE zeroes the rest", STR$(Growable(5)), "0"
Check "REDIM PRESERVE grows", STR$(UBOUND(Growable)), "5"
REDIM Growable(4)
Check "REDIM clears", STR$(Growable(1)), "0"

' ---------------------------------------------------------------------------
' Records
' ---------------------------------------------------------------------------
DIM Here AS Point
DIM Somebody AS Person
DIM Crowd(3) AS Person

Here.X = 3
Here.Y = 4
Check "record field", STR$(Here.X), "3"

Somebody.Name = "Ada"
Somebody.Home.X = 10
Somebody.Home.Y = 20
Check "record string field", RTRIM$(Somebody.Name), "Ada"
Check "nested record field", STR$(Somebody.Home.Y), "20"

Crowd(1).Name = "Grace"
Crowd(1).Home.X = 99
Check "record in an array", RTRIM$(Crowd(1).Name), "Grace"
Check "nested field in an array", STR$(Crowd(1).Home.X), "99"

DIM Copy AS Person
Copy = Somebody
Somebody.Name = "changed"
Check "record assignment copies", RTRIM$(Copy.Name), "Ada"

Check "record passed by value", ShowName$(Copy), "Ada"

' ---------------------------------------------------------------------------
' IF in all its forms
' ---------------------------------------------------------------------------
X = 5
IF X > 0 THEN Sign$ = "pos"
Check "single line IF", Sign$, "pos"
IF X > 100 THEN Sign$ = "big" ELSE Sign$ = "small"
Check "single line IF ELSE", Sign$, "small"

IF X < 0 THEN
    Band$ = "negative"
ELSEIF X = 0 THEN
    Band$ = "zero"
ELSEIF X < 10 THEN
    Band$ = "small"
ELSE
    Band$ = "large"
END IF
Check "ELSEIF chain", Band$, "small"

IF X = 5 THEN
    IF X > 4 THEN
        Nested$ = "both"
    END IF
END IF
Check "nested IF", Nested$, "both"

' ---------------------------------------------------------------------------
' SELECT CASE
' ---------------------------------------------------------------------------
Grade% = 85
SELECT CASE Grade%
    CASE 90 TO 100
        Letter$ = "A"
    CASE 80 TO 89
        Letter$ = "B"
    CASE IS >= 70
        Letter$ = "C"
    CASE ELSE
        Letter$ = "F"
END SELECT
Check "SELECT CASE range", Letter$, "B"

SELECT CASE 3
    CASE 1, 2, 3
        List$ = "low"
    CASE ELSE
        List$ = "high"
END SELECT
Check "SELECT CASE list", List$, "low"

SELECT CASE "beta"
    CASE "alpha"
        Word$ = "first"
    CASE "beta"
        Word$ = "second"
    CASE ELSE
        Word$ = "other"
END SELECT
Check "SELECT CASE on strings", Word$, "second"

SELECT CASE 999
    CASE 1
        Fell$ = "no"
    CASE ELSE
        Fell$ = "else ran"
END SELECT
Check "SELECT CASE ELSE", Fell$, "else ran"

' ---------------------------------------------------------------------------
' Loops
' ---------------------------------------------------------------------------
Total = 0
FOR I = 1 TO 10
    Total = Total + I
NEXT I
Check "FOR sums", STR$(Total), "55"

Total = 0
FOR I = 10 TO 1 STEP -1
    Total = Total + 1
NEXT I
Check "FOR counts down", STR$(Total), "10"

Total = 0
FOR K = 0 TO 1 STEP .25
    Total = Total + 1
NEXT K
Check "FOR fractional STEP", STR$(Total), "5"

Total = 0
FOR I = 1 TO 3
    FOR J = 1 TO 3
        Total = Total + 1
    NEXT J
NEXT I
Check "nested FOR", STR$(Total), "9"

Total = 0
FOR I = 1 TO 5
    Total = Total + 1
NEXT
Check "bare NEXT", STR$(Total), "5"

Total = 0
FOR I = 5 TO 1
    Total = Total + 1
NEXT I
Check "FOR that never runs", STR$(Total), "0"

N = 1
WHILE N < 100
    N = N * 2
WEND
Check "WHILE WEND", STR$(N), "128"

N = 0
DO WHILE N < 5
    N = N + 1
LOOP
Check "DO WHILE", STR$(N), "5"

N = 0
DO
    N = N + 1
LOOP WHILE N < 5
Check "LOOP WHILE", STR$(N), "5"

N = 0
DO UNTIL N >= 5
    N = N + 1
LOOP
Check "DO UNTIL", STR$(N), "5"

N = 0
DO
    N = N + 1
LOOP UNTIL N >= 5
Check "LOOP UNTIL", STR$(N), "5"

N = 99
DO WHILE N < 0
    N = 1
LOOP
Check "DO WHILE that never runs", STR$(N), "99"

Found = 0
FOR I = 1 TO 100
    IF I = 7 THEN
        Found = I
        EXIT FOR
    END IF
NEXT I
Check "EXIT FOR", STR$(Found), "7"

N = 0
DO
    N = N + 1
    IF N = 3 THEN EXIT DO
LOOP
Check "EXIT DO", STR$(N), "3"

' ---------------------------------------------------------------------------
' GOTO, GOSUB and labels
' ---------------------------------------------------------------------------
Trace$ = ""
GOTO SkipMe
Trace$ = Trace$ + "not reached"
SkipMe:
Trace$ = Trace$ + "jumped"
Check "GOTO a label", Trace$, "jumped"

Counter = 0
GOSUB Bump
GOSUB Bump
Check "GOSUB and RETURN", STR$(Counter), "2"

Pick = 2
ON Pick GOTO First, Second, Third
First:
Chose$ = "first"
GOTO DoneChoosing
Second:
Chose$ = "second"
GOTO DoneChoosing
Third:
Chose$ = "third"
DoneChoosing:
Check "ON GOTO", Chose$, "second"

' ---------------------------------------------------------------------------
' Procedures
' ---------------------------------------------------------------------------
Check "FUNCTION", STR$(Square(7)), "49"
Check "FUNCTION recursion", STR$(Fact(6)), "720"
Check "FUNCTION two arguments", STR$(AddUp(3, 4)), "7"
Check "FUNCTION returning a string", Shout$("hi"), "HI!"
Check "FUNCTION AS INTEGER truncates", STR$(HalfInt(7)), "3"

Arg = 5
Doubler Arg
Check "parameters are by value", STR$(Arg), "5"

CallCount = 0
Bumper
Bumper
Check "SUB reaches a global", STR$(CallCount), "2"

Check "SUB with parentheses", Joined$("a", "b"), "a-b"
Check "EXIT FUNCTION", STR$(EarlyOut(0)), "0"
Check "EXIT SUB", STR$(SetUnlessZero(0)), "0"

Check "DEF FN", STR$(FNArea(3, 4)), "12"
Check "DEF FN with an expression argument", STR$(FNArea(2 + 1, 4)), "12"

Depth = 0
Check "mutual recursion", STR$(IsEven(10)), "-1"

' ---------------------------------------------------------------------------
' Strings
' ---------------------------------------------------------------------------
A$ = "Hello, World"
Check "LEN", STR$(LEN(A$)), "12"
Check "LEFT$", LEFT$(A$, 5), "Hello"
Check "RIGHT$", RIGHT$(A$, 5), "World"
Check "MID$ three arguments", MID$(A$, 8, 5), "World"
Check "MID$ two arguments", MID$(A$, 8), "World"
Check "MID$ is 1-based", MID$(A$, 1, 1), "H"
Check "INSTR", STR$(INSTR(A$, "World")), "8"
Check "INSTR missing", STR$(INSTR(A$, "zzz")), "0"
Check "INSTR from a position", STR$(INSTR(4, A$, "l")), "4"
Check "ASC", STR$(ASC("A")), "65"
Check "CHR$", CHR$(65), "A"
Check "VAL", STR$(VAL("42.5")), "42.5"
Check "VAL of junk", STR$(VAL("abc")), "0"
Check "STR$ of a negative", STR$(-5), "-5"
Check "SPACE$", "[" + SPACE$(3) + "]", "[   ]"
Check "STRING$ of a character", STRING$(3, "x"), "xxx"
Check "STRING$ of a code", STRING$(3, 65), "AAA"
Check "LTRIM$", LTRIM$("  x"), "x"
Check "RTRIM$", RTRIM$("x  "), "x"
Check "UCASE$", UCASE$("MiXeD"), "MIXED"
Check "LCASE$", LCASE$("MiXeD"), "mixed"
Check "HEX$", HEX$(255), "FF"
Check "OCT$", OCT$(8), "10"
Check "concatenation", "a" + "b" + "c", "abc"
Check "embedded quote", "say ""hi""", "say " + CHR$(34) + "hi" + CHR$(34)
Check "empty string", "[" + "" + "]", "[]"

B$ = "hello"
MID$(B$, 1, 1) = "J"
Check "MID$ assignment", B$, "Jello"
C$ = "xxxxx"
MID$(C$, 2, 3) = "abc"
Check "MID$ assignment mid-string", C$, "xabcx"
D2$ = "abc"
MID$(D2$, 1, 2) = "ZZZZZ"
Check "MID$ assignment never grows", D2$, "ZZc"

' ---------------------------------------------------------------------------
' Math
' ---------------------------------------------------------------------------
Check "ABS", STR$(ABS(-5)), "5"
Check "INT floors", STR$(INT(-2.5)), "-3"
Check "FIX truncates", STR$(FIX(-2.5)), "-2"
Check "SGN negative", STR$(SGN(-9)), "-1"
Check "SGN zero", STR$(SGN(0)), "0"
Check "SGN positive", STR$(SGN(9)), "1"
Check "SQR", STR$(SQR(144)), "12"
Check "EXP of zero", STR$(EXP(0)), "1"
Check "LOG of one", STR$(LOG(1)), "0"
Check "SIN of zero", STR$(SIN(0)), "0"
Check "COS of zero", STR$(COS(0)), "1"
Check "TAN of zero", STR$(TAN(0)), "0"
Check "ATN", STR$(INT(ATN(1) * 4 * 1000)), "3141"
Check "CINT rounds half to even", STR$(CINT(2.5)), "2"
Check "CINT rounds half to even up", STR$(CINT(3.5)), "4"
Check "CINT rounds down", STR$(CINT(2.4)), "2"

R = RND
IF R >= 0 AND R < 1 THEN RndOK$ = "in range" ELSE RndOK$ = "out of range"
Check "RND is in range", RndOK$, "in range"
IF TIMER > 0 THEN TimerOK$ = "positive" ELSE TimerOK$ = "not positive"
Check "TIMER runs", TimerOK$, "positive"

' ---------------------------------------------------------------------------
' SWAP and LET
' ---------------------------------------------------------------------------
P = 1
Q = 2
SWAP P, Q
Check "SWAP numbers", STR$(P) + STR$(Q), "21"
DIM SwapMe(2)
SwapMe(0) = 10
SwapMe(1) = 20
SWAP SwapMe(0), SwapMe(1)
Check "SWAP array elements", STR$(SwapMe(0)), "20"
E$ = "one"
F$ = "two"
SWAP E$, F$
Check "SWAP strings", E$, "two"

LET Explicit = 42
Check "LET", STR$(Explicit), "42"
LET Somebody.Name = "Let"
Check "LET on a record field", RTRIM$(Somebody.Name), "Let"

' ---------------------------------------------------------------------------
' DATA, READ and RESTORE
' ---------------------------------------------------------------------------
READ N1, N2, N3
Check "READ numbers", STR$(N1 + N2 + N3), "60"
READ W1$, W2$
Check "READ strings", W1$ + W2$, "HelloWorld"
RESTORE
READ Again
Check "RESTORE rewinds", STR$(Again), "10"
RESTORE Later
READ LaterValue
Check "RESTORE to a label", STR$(LaterValue), "777"

' ---------------------------------------------------------------------------
' PRINT USING
'
' PRINT USING has no file form -- `PRINT #n, USING` is rejected -- so these go
' to the console between markers, and the Rust side checks the block. Every
' other case in this program checks itself.
' ---------------------------------------------------------------------------
PRINT "USING-BEGIN"
PRINT USING "###.##"; 3.14159
PRINT USING "Total: ###"; 42
PRINT USING "## and ##"; 1; 2
PRINT USING "+###"; 5
PRINT USING "#,###"; 1234
PRINT USING "$$###.##"; 9.5
PRINT USING "**###"; 7
PRINT USING "!"; "abc"
PRINT USING "\   \"; "abcdefg"
PRINT USING "&"; "whole"
PRINT "USING-END"

' ---------------------------------------------------------------------------
' Sequential file I/O
' ---------------------------------------------------------------------------
OPEN "seq.txt" FOR OUTPUT AS #1
PRINT #1, "first line"
PRINT #1, 25
PRINT #1, 1.5
WRITE #1, "quoted", 7
CLOSE #1

OPEN "seq.txt" FOR INPUT AS #1
LINE INPUT #1, L1$
INPUT #1, Age
INPUT #1, Rate
INPUT #1, QuotedText$, QuotedNum
CLOSE #1
Check "sequential line", L1$, "first line"
Check "sequential number", STR$(Age), "25"
Check "sequential real", STR$(Rate), "1.5"
Check "WRITE quotes strings", QuotedText$, "quoted"
Check "WRITE and INPUT agree", STR$(QuotedNum), "7"

OPEN "seq.txt" FOR APPEND AS #1
PRINT #1, "appended"
CLOSE #1
OPEN "seq.txt" FOR INPUT AS #1
LineCount = 0
WHILE NOT EOF(1)
    LINE INPUT #1, Any$
    LineCount = LineCount + 1
    LastLine$ = Any$
WEND
CLOSE #1
Check "APPEND adds a line", LastLine$, "appended"
Check "EOF loop reads every line", STR$(LineCount), "5"

OPEN "seq.txt" FOR INPUT AS #2
Check "LOF is positive", STR$(LOF(2) > 0), "-1"
CLOSE #2
CLOSE

' A file number held in a variable, not a literal.
Handle% = 4
OPEN "byvar.txt" FOR OUTPUT AS #Handle%
PRINT #Handle%, "via a variable"
CLOSE #Handle%
OPEN "byvar.txt" FOR INPUT AS #Handle%
LINE INPUT #Handle%, ByVar$
CLOSE #Handle%
Check "file number from a variable", ByVar$, "via a variable"

' ---------------------------------------------------------------------------
' Random-access file I/O
' ---------------------------------------------------------------------------
OPEN "people.dat" FOR RANDOM AS #1 LEN = 32
FIELD #1, 20 AS RecName$, 4 AS RecAge$, 8 AS RecPay$

Check "FIELD sets the width", STR$(LEN(RecName$)), "20"

LSET RecName$ = "Alice"
LSET RecAge$ = MKI$(30)
LSET RecPay$ = MKD$(50000.5)
PUT #1, 1

LSET RecName$ = "Bob"
LSET RecAge$ = MKI$(45)
LSET RecPay$ = MKD$(61234.25)
PUT #1, 2

RSET RecName$ = "Carol"
LSET RecAge$ = MKI$(-7)
LSET RecPay$ = MKD$(0.125)
PUT #1, 3

Check "three records written", STR$(LOF(1)), "96"

GET #1, 2
Check "GET by number", RTRIM$(RecName$), "Bob"
Check "CVI round trip", STR$(CVI(RecAge$)), "45"
Check "CVD round trip", STR$(CVD(RecPay$)), "61234.25"

GET #1, 1
Check "GET rewinds", RTRIM$(RecName$), "Alice"
Check "LSET pads on the right", RecName$, "Alice" + SPACE$(15)

GET #1, 3
Check "RSET pads on the left", RecName$, SPACE$(15) + "Carol"
Check "CVI of a negative", STR$(CVI(RecAge$)), "-7"

GET #1, 1
GET #1
Check "GET without a number advances", RTRIM$(RecName$), "Bob"
Check "LOC tracks the record", STR$(LOC(1)), "2"

GET #1, 2
LSET RecName$ = "Robert"
PUT #1, 2
GET #1, 2
Check "record updated in place", RTRIM$(RecName$), "Robert"
Check "other fields survive the update", STR$(CVI(RecAge$)), "45"

LOCK #1, 1
UNLOCK #1, 1
LOCK #1
UNLOCK #1
Check "LOCK and UNLOCK", "ran", "ran"

CLOSE #1

Check "MKI$ width", STR$(LEN(MKI$(1))), "2"
Check "MKL$ width", STR$(LEN(MKL$(1))), "4"
Check "MKS$ width", STR$(LEN(MKS$(1))), "4"
Check "MKD$ width", STR$(LEN(MKD$(1))), "8"
Check "CVI extreme high", STR$(CVI(MKI$(32767))), "32767"
Check "CVI extreme low", STR$(CVI(MKI$(-32768))), "-32768"
Check "CVL round trip", STR$(CVL(MKL$(2147483647))), "2147483647"
Check "CVS round trip", STR$(CVS(MKS$(3.5))), "3.5"

' ---------------------------------------------------------------------------
' Scope: a module-level name is the same storage inside a procedure, a name
' used only inside one is not.
' ---------------------------------------------------------------------------
Shared% = 1
TouchShared
Check "procedures see module storage", STR$(Shared%), "2"
Check "locals start fresh", STR$(FreshLocal()), "1"
Check "locals start fresh again", STR$(FreshLocal()), "1"

' ---------------------------------------------------------------------------
' Report
' ---------------------------------------------------------------------------
PRINT "PASSED"; Passed
PRINT "FAILED"; Failed
END

' ---------------------------------------------------------------------------
' GOSUB targets, reachable only by GOSUB
' ---------------------------------------------------------------------------
Bump:
Counter = Counter + 1
RETURN

' ---------------------------------------------------------------------------
' DATA
' ---------------------------------------------------------------------------
DATA 10, 20, 30
DATA "Hello", "World"
Later:
DATA 777

' ---------------------------------------------------------------------------
' Procedures
' ---------------------------------------------------------------------------

' The harness. Comparing as text keeps every expectation independent of how
' PRINT spaces a number, and a failure names itself.
SUB Check(Name$, Got$, Want$)
    IF Got$ = Want$ THEN
        Passed = Passed + 1
    ELSE
        PRINT "FAIL "; Name$; " got["; Got$; "] want["; Want$; "]"
        Failed = Failed + 1
    END IF
END SUB

FUNCTION Square(N)
    Square = N * N
END FUNCTION

FUNCTION Fact(N)
    IF N <= 1 THEN
        Fact = 1
    ELSE
        Fact = N * Fact(N - 1)
    END IF
END FUNCTION

FUNCTION AddUp(A, B)
    AddUp = A + B
END FUNCTION

FUNCTION Shout$(S$)
    Shout$ = UCASE$(S$) + "!"
END FUNCTION

FUNCTION HalfInt(N) AS INTEGER
    HalfInt = N / 2
END FUNCTION

FUNCTION Joined$(A$, B$)
    Joined$ = A$ + "-" + B$
END FUNCTION

' Leaves early, so the result keeps the zero it started with.
FUNCTION EarlyOut(N)
    IF N = 0 THEN EXIT FUNCTION
    EarlyOut = 99
END FUNCTION

FUNCTION SetUnlessZero(N)
    Marker = 0
    Marked N
    SetUnlessZero = Marker
END FUNCTION

SUB Marked(N)
    IF N = 0 THEN EXIT SUB
    Marker = 1
END SUB

SUB Doubler(X)
    X = X * 2
END SUB

SUB Bumper
    CallCount = CallCount + 1
END SUB

SUB TouchShared
    Shared% = Shared% + 1
END SUB

' Uses a name that appears nowhere at module level, so the slot is local and
' starts at zero on every call however many times it is called.
FUNCTION FreshLocal()
    OnlyHere = OnlyHere + 1
    FreshLocal = OnlyHere
END FUNCTION

FUNCTION ShowName$(P AS Person)
    ShowName$ = RTRIM$(P.Name)
END FUNCTION

FUNCTION IsEven(N)
    IF N = 0 THEN
        IsEven = -1
    ELSE
        IsEven = IsOdd(N - 1)
    END IF
END FUNCTION

FUNCTION IsOdd(N)
    IF N = 0 THEN
        IsOdd = 0
    ELSE
        IsOdd = IsEven(N - 1)
    END IF
END FUNCTION

DEF FNArea(W, H) = W * H
