//! Function and subroutine tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_basic_procedures() {
    // Test function definition, sub definition, and sub with params
    let output = compile_and_run(
        r#"
FUNCTION Double(X)
    Double = X * 2
END FUNCTION

SUB PrintSum(A, B)
    PRINT A + B
END SUB

PRINT Double(21)
PrintSum(10, 20)
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "42", "function");
    assert_eq!(lines[1], "30", "sub with params");
}

#[test]
fn test_sub_no_params() {
    // Test subroutine without parameters
    let output = compile_and_run(
        r#"
PrintHello
PRINT "done"
END

SUB PrintHello
    PRINT "Hello from sub"
END SUB
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["Hello from sub", "done"]);
}

#[test]
fn test_many_params() {
    // Test procedures with 7, 8, and 10 parameters (overflow handling)
    let output = compile_and_run(
        r#"
SUB Sum7(A, B, C, D, E, F, G)
    PRINT A + B + C + D + E + F + G
END SUB

FUNCTION Sum8(A, B, C, D, E, F, G, H)
    Sum8 = A + B + C + D + E + F + G + H
END FUNCTION

FUNCTION Sum10(A, B, C, D, E, F, G, H, I, J)
    Sum10 = A + B + C + D + E + F + G + H + I + J
END FUNCTION

Sum7(1, 2, 3, 4, 5, 6, 7)
PRINT Sum8(1, 2, 3, 4, 5, 6, 7, 8)
PRINT Sum10(1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "28", "7 params: 1+2+3+4+5+6+7");
    assert_eq!(lines[1], "36", "8 params: 1+..+8");
    assert_eq!(lines[2], "55", "10 params: 1+..+10");
}

#[test]
fn test_nested_calls() {
    // Test nested function calls in arguments
    let output = compile_and_run(
        r#"
FUNCTION Add(A, B)
    Add = A + B
END FUNCTION

FUNCTION Mul(A, B)
    Mul = A * B
END FUNCTION

FUNCTION AddThree(A, B, C)
    AddThree = A + B + C
END FUNCTION

PRINT Add(Mul(2, 3), Mul(4, 5))
PRINT AddThree(Mul(2, 3), Mul(4, 5), Mul(6, 7))
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "26", "nested: 2*3 + 4*5 = 6+20");
    assert_eq!(lines[1], "68", "nested three: 6+20+42");
}

/// A module-level variable a procedure SHAREs is the same storage inside it.
/// (They once read as 0 inside a procedure, because procedures were compiled
/// before main and so allocated their own local instead.)
#[test]
fn test_shared_module_variables_in_procedures() {
    let output = compile_and_run(
        r#"
G = 99
SUB Bump
SHARED G
PRINT G
G = G + 1
END SUB
Bump
PRINT G
Bump
PRINT G
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["99", "100", "100", "101"], "shared storage");
}

/// An array DIM'd at module level must be usable inside a procedure. This used
/// to abort the compiler with "Array not declared".
#[test]
fn test_module_array_accessible_in_procedure() {
    let output = compile_and_run(
        r#"
DIM SHARED A(5)
A(0) = 7
SUB Touch
PRINT A(0)
A(1) = 8
END SUB
Touch
PRINT A(1)
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["7", "8"], "array shared with procedure");
}

/// A parameter shadows a module-level variable of the same name, and assigning
/// to the parameter must not disturb the global (parameters are by value).
#[test]
fn test_parameter_shadows_global() {
    let output = compile_and_run(
        r#"
X = 1
SUB Show(X)
PRINT X
X = X * 2
PRINT X
END SUB
Show(42)
PRINT X
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["42", "84", "1"], "parameter is local");
}

/// A variable used only inside a procedure is local to it: zeroed on entry,
/// and not retained between calls.
#[test]
fn test_procedure_locals_are_fresh_each_call() {
    let output = compile_and_run(
        r#"
SUB Count
PRINT Q
Q = Q + 1
PRINT Q
END SUB
Count
Count
Count
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["0", "1", "0", "1", "0", "1"], "fresh per call");
}

/// LANGREF's own SUB example. A string argument occupies two slots (pointer and
/// length) at the call site, but the callee bound one register per parameter,
/// so the string arrived empty and printed "Hello, !".
#[test]
fn test_string_parameter() {
    let output = compile_and_run(
        r#"
SUB PrintGreeting(Name$)
PRINT "Hello, "; Name$; "!"
END SUB
PrintGreeting("World")
PrintGreeting "Again"
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["Hello, World!", "Hello, Again!"]);
}

/// A parameter following a string was also corrupted, because the caller and
/// callee disagreed about how many slots the string consumed.
#[test]
fn test_string_and_numeric_parameters() {
    let output = compile_and_run(
        r#"
SUB A(S$, N)
PRINT S$
PRINT N
END SUB
SUB B(N, S$)
PRINT N
PRINT S$
END SUB
A("hi", 42)
B(7, "world")
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["hi", "42", "7", "world"]);
}

/// Enough string parameters to exhaust the argument registers and spill to the
/// stack, on both the 6-register System V and 4-register Win64 conventions.
#[test]
fn test_parameter_register_overflow() {
    let output = compile_and_run(
        r#"
SUB S3(A$, B$, C$)
PRINT A$; B$; C$
END SUB
SUB M(A, B$, C, D$, E, F$)
PRINT A; B$; C; D$; E; F$
END SUB
S3("a", "b", "c")
M(1, "x", 2, "y", 3, "z")
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["abc", "1 x 2 y 3 z"]);
}

/// Parameters declared with a type suffix must arrive narrowed to that type.
#[test]
fn test_typed_parameters() {
    let output = compile_and_run(
        r#"
SUB T(I%, L&, S!, D#)
PRINT I%
PRINT L&
PRINT S!
PRINT D#
END SUB
T(3, 100000, 2.5, 1.25)
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["3", "100000", "2.5", "1.25"]);
}

/// A FUNCTION with a `$` suffix returns a string. This used to abort the
/// compiler: any unrecognized `$`-suffixed call was assumed to be an array.
#[test]
fn test_string_returning_function() {
    let output = compile_and_run(
        r#"
FUNCTION Greet$(N$)
Greet$ = "Hello, " + N$
END FUNCTION
FUNCTION Twice$(S$)
Twice$ = S$ + S$
END FUNCTION
PRINT Greet$("World")
PRINT Twice$(Twice$("ab"))
PRINT "[" + Greet$("x") + "]"
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(
        lines,
        vec!["Hello, World", "abababab", "[Hello, x]"],
        "string functions compose"
    );
}

/// A parameterless FUNCTION is called by naming it. Inside its own body the
/// same name is the return variable, which must not recurse.
#[test]
fn test_parameterless_function() {
    let output = compile_and_run(
        r#"
FUNCTION Name$
Name$ = "bob"
END FUNCTION
FUNCTION Answer
Answer = 42
END FUNCTION
PRINT Name$
PRINT Answer
IF Name$ = "bob" THEN PRINT "eq" ELSE PRINT "ne"
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["bob", "42", "eq"]);
}

/// A variable argument is passed by reference, as in QuickBASIC: the
/// procedure's changes reach it, strings included. BYVAL passes a copy, and
/// an expression is a copy whatever the parameter says. Every parameter used
/// to be by value.
#[test]
fn test_parameters_are_by_reference() {
    let output = compile_and_run(
        r#"
SUB Change(S$, N)
S$ = "changed"
N = N * 2
END SUB
SUB Keep(BYVAL S$, BYVAL N)
S$ = "kept?"
N = 0
END SUB
T$ = "original": X = 21
Change T$, X
PRINT T$; X
Keep T$, X
PRINT T$; X
Change T$ + "", X + 0
PRINT T$; X
DIM A%(3): A%(2) = 5
SUB Bump(K%)
K% = K% + 1
END SUB
Bump A%(2)
PRINT A%(2)
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["changed 42", "changed 42", "changed 42", "6"]);
}

/// DEF FN is desugared into an ordinary FUNCTION, so it reuses the whole
/// procedure path and evaluates each argument exactly once.
#[test]
fn test_def_fn() {
    let output = compile_and_run(
        "DEF FNA(X) = X * 2\nDEF FNSUM(A, B) = A + B\nDEF FNPI = 3.14159\nDEF FNG$(N$) = \"hi \" + N$\nPRINT FNA(5)\nPRINT FNSUM(3, 4)\nPRINT FNPI\nPRINT FNG$(\"bob\")\n",
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["10", "7", "3.14159", "hi bob"]);
}

/// A DEF FN body can see module-level variables, like any other procedure.
#[test]
fn test_def_fn_sees_globals() {
    let output = compile_and_run("G = 10\nDEF FNS(X) = X + G\nPRINT FNS(5)\n").unwrap();
    assert_eq!(output.trim(), "15");
}

/// OPTION BASE 1 makes 1 the lowest legal subscript. Storage for element 0 is
/// still allocated and simply unused, which leaves the index arithmetic alone.
#[test]
fn test_option_base_one() {
    let output = compile_and_run(
        "OPTION BASE 1\nDIM A(3)\nA(1) = 10\nA(3) = 30\nPRINT A(1); A(3)\nPRINT LBOUND(A); UBOUND(A)\n",
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["10  30", "1  3"]);
}

/// The default base is still 0.
#[test]
fn test_option_base_defaults_to_zero() {
    let output = compile_and_run("DIM A(3)\nA(0) = 5\nPRINT A(0); LBOUND(A)\n").unwrap();
    assert_eq!(output.trim(), "5  0");
}

/// `FUNCTION f(...) AS T` must actually give the result type T.
///
/// The declared type was parsed into a map nothing read, so the result fell
/// back to the name's suffix -- Double for an unsuffixed name.
#[test]
fn test_function_declared_return_type() {
    let output = compile_and_run(
        "FUNCTION F(X) AS INTEGER\nF = X / 2\nEND FUNCTION\nFUNCTION G(X) AS LONG\nG = X * 1000\nEND FUNCTION\nPRINT F(7)\nPRINT G(3)\n",
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["4", "3000"], "7 / 2 rounds half to even");
}

/// Without an AS clause the suffix still decides, as before.
#[test]
fn test_function_return_type_without_as() {
    let output = compile_and_run("FUNCTION K(X)\nK = X / 2\nEND FUNCTION\nPRINT K(7)\n").unwrap();
    assert_eq!(output.trim(), "3.5");
}

/// `CALL` is the explicit form of a procedure call.
///
/// It was not recognised at all, so `CALL MySub(1)` parsed as a paren-less call
/// to a subroutine named CALL whose argument was `MySub(1)`, and the diagnostic
/// talked about MySub having no value rather than about CALL.
#[test]
fn test_call_statement() {
    let output = compile_and_run(
        r#"
SUB Greet(N)
  PRINT "n="; N
END SUB
SUB Plain
  PRINT "plain"
END SUB
CALL Greet(7)
CALL Plain
Greet 8
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, &["n= 7", "plain", "n= 8"]);
}

/// CALL is recognised only in statement position before a name, so a program
/// may still use it as a variable -- the same rule the random-access statement
/// names follow.
#[test]
fn test_call_is_not_reserved() {
    let output = compile_and_run("CALL = 5\nPRINT CALL\n").unwrap();
    assert_eq!(output.trim(), "5");
}

/// A procedure's names are its own unless shared, as in QuickBASIC: a module
/// variable of the same name is a different variable inside it. Every name the
/// module used used to be visible in every procedure.
#[test]
fn test_procedure_variables_are_local_unless_shared() {
    let output = compile_and_run(
        r#"
X = 1: Y = 2
DIM A(2): A(1) = 7
SUB Show
PRINT X; A(1)
X = 50
END SUB
SUB ShowShared
SHARED Y, A()
PRINT Y; A(1)
Y = 60
END SUB
Show
ShowShared
PRINT X; Y
"#,
    )
    .unwrap();
    assert_eq!(crate::common::lines(&output), vec!["0  0", "2  7", "1  60"]);
}

/// DIM SHARED at module level shares a variable, array or record with every
/// procedure.
#[test]
fn test_dim_shared_reaches_every_procedure() {
    let output = compile_and_run(
        r#"
TYPE P
N AS INTEGER
END TYPE
DIM SHARED Total, Names$(3), R AS P
SUB AddUp(N)
Total = Total + N
Names$(N) = "n" + STR$(N)
R.N = R.N + 1
END SUB
AddUp 1
AddUp 2
PRINT Total; Names$(2); R.N
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3 n 2 2");
}

/// STATIC keeps a procedure's variable from one call to the next; `SUB name
/// STATIC` does that for every local. Recursion still gets a fresh ordinary
/// local per call.
#[test]
fn test_static_locals_keep_their_values() {
    let output = compile_and_run(
        r#"
FUNCTION Tick
STATIC N
N = N + 1
Tick = N
END FUNCTION
SUB Every STATIC
C = C + 1
DIM Hist(3)
PRINT C;
END SUB
PRINT Tick; Tick; Tick
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "1  2  3");
    let output = compile_and_run(
        "SUB Count STATIC\nC = C + 1\nS$ = S$ + \"x\"\nPRINT C; S$\nEND SUB\nCount\nCount\nCount\n",
    )
    .unwrap();
    assert_eq!(crate::common::lines(&output), vec!["1 x", "2 xx", "3 xxx"]);
}

/// SHARED and STATIC belong inside a procedure, and DIM SHARED outside one.
#[test]
fn test_shared_and_static_placement_is_checked() {
    for (source, msg) in [
        ("SHARED X\n", "SHARED belongs inside a SUB or FUNCTION"),
        ("STATIC X\n", "STATIC belongs inside a SUB or FUNCTION"),
        (
            "SUB S\nDIM SHARED X\nEND SUB\n",
            "DIM SHARED belongs at module level",
        ),
    ] {
        let err = crate::common::compile_only(source).expect_err(source);
        assert!(err.contains(msg), "{source:?}: {}", err.stderr);
    }
}

/// DEF FN is part of the module, as in GW-BASIC: its expression sees module
/// variables without any SHARED.
#[test]
fn test_def_fn_sees_module_variables() {
    let output =
        compile_and_run("Rate = 3\nDEF FNScale(X) = X * Rate\nPRINT FNScale(5)\n").unwrap();
    assert_eq!(output.trim(), "15");
}

/// An array parameter, `A()`, takes an array by reference: the procedure sees
/// the caller's elements and bounds, and a REDIM or ERASE in it reaches the
/// caller. Its dimensions are whatever the body subscripts it with.
#[test]
fn test_array_parameters() {
    let output = compile_and_run(
        r#"
SUB Fill(V(), N)
FOR I = LBOUND(V) TO UBOUND(V): V(I) = I * N: NEXT I
END SUB
FUNCTION Total(V())
T = 0
FOR I = LBOUND(V) TO UBOUND(V): T = T + V(I): NEXT I
Total = T
END FUNCTION
SUB Grow(V())
REDIM PRESERVE V(5)
V(5) = 50
END SUB
SUB Corner(M$())
M$(2, 3) = "x"
END SUB
DIM A(3), B(1 TO 2)
Fill A(), 10
Fill B(), 1
PRINT A(3); Total(A()); Total(B())
Grow A()
PRINT UBOUND(A); A(5); A(3)
DIM G$(2, 3)
Corner G$()
PRINT G$(2, 3)
"#,
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec!["30  60  3", "5  50  30", "x"]
    );
}

/// An array argument must be an array of the parameter's type, with the
/// dimensions the procedure uses.
#[test]
fn test_array_arguments_are_checked() {
    for (source, msg) in [
        ("SUB S(V())\nV(1) = 1\nEND SUB\nS 5\n", "must be an array"),
        (
            "SUB S(V())\nV(1) = 1\nEND SUB\nDIM M(2, 2)\nS M()\n",
            "has 2 dimensions, but V() is used with 1",
        ),
        (
            "SUB S(V())\nV(1) = 1\nEND SUB\nDIM N$(2)\nS N$()\n",
            "wrong type",
        ),
    ] {
        let err = crate::common::compile_only(source).expect_err(source);
        assert!(err.contains(msg), "{source:?}: {}", err.stderr);
    }
}

/// A variable passed by reference must be the parameter's type, as
/// QuickBASIC's "Parameter type mismatch" requires; an expression of another
/// type is converted into a temporary.
#[test]
fn test_by_reference_type_must_match() {
    let err =
        crate::common::compile_only("SUB S(N%)\nN% = 1\nEND SUB\nX# = 2\nS X#\n").unwrap_err();
    assert!(
        err.contains("passed by reference, so it must be an INTEGER, not a DOUBLE"),
        "{}",
        err.stderr
    );
    let output = compile_and_run(
        "SUB S(N%)\nN% = N% * 2\nPRINT N%\nEND SUB\nX# = 2.6\nS X# + 0\nS CINT(X#)\nPRINT X#\n",
    )
    .unwrap();
    assert_eq!(crate::common::lines(&output), vec!["6", "6", "2.6"]);
}

/// DECLARE states a procedure's signature, as QuickBASIC modules begin with;
/// it is checked against the definition.
#[test]
fn test_declare() {
    let output = compile_and_run(
        "DECLARE SUB Show (A$, BYVAL N%)\nDECLARE FUNCTION Twice# (X#)\nShow \"n\", 2\nPRINT Twice#(4)\nSUB Show (A$, BYVAL N%)\nPRINT A$; N%\nEND SUB\nFUNCTION Twice# (X#)\nTwice# = X# * 2\nEND FUNCTION\n",
    )
    .unwrap();
    assert_eq!(crate::common::lines(&output), vec!["n 2", "8"]);
    for (source, msg) in [
        ("DECLARE SUB Nope ()\n", "declared but never defined"),
        (
            "DECLARE SUB S (A)\nSUB S (A, B)\nEND SUB\n",
            "defined with 2 parameters",
        ),
        (
            "DECLARE FUNCTION S (A)\nSUB S (A)\nEND SUB\n",
            "defined as a SUB",
        ),
        (
            "DECLARE SUB S (BYVAL A)\nSUB S (A)\nEND SUB\n",
            "parameter 1 differs",
        ),
    ] {
        let err = crate::common::compile_only(source).expect_err(source);
        assert!(err.contains(msg), "{source:?}: {}", err.stderr);
    }
}

/// By reference through recursion, and when an error leaves the procedure:
/// what it had written still reaches the caller.
#[test]
fn test_by_reference_through_recursion_and_errors() {
    let output = compile_and_run(
        r#"
SUB Down(N, Steps)
IF N <= 0 THEN EXIT SUB
Steps = Steps + 1
Down N - 1, Steps
END SUB
S = 0
Down 5, S
PRINT S
SUB Fails(V)
V = 7
ERROR 5
END SUB
ON ERROR GOTO 100
X = 1
Fails X
200 PRINT "x"; X
END
100 PRINT "trapped"; X
RESUME 200
"#,
    )
    .unwrap();
    assert_eq!(crate::common::lines(&output), vec!["5", "trapped 7", "x 7"]);
}

/// A by-reference parameter is the caller's variable, not a copy of it: a
/// write is seen at once through every other name for the same storage --
/// another parameter, a SHARED name, or an array element -- as in QuickBASIC.
#[test]
fn test_by_reference_parameters_alias() {
    let output = compile_and_run(
        r#"
DIM SHARED G, ARR(3)
SUB Both(A, B)
  A = 1
  PRINT "both"; B
END SUB
SUB Global(A)
  A = 2
  PRINT "shared"; G
END SUB
SUB Element(E)
  E = 9
  PRINT "element"; ARR(1)
END SUB
SUB Swap2(A$, B$)
  A$ = A$ + B$
  PRINT B$
END SUB
X = 5
Both X, X
G = 1
Global G
Element ARR(1)
S$ = "ab"
Swap2 S$, S$
PRINT X; G; ARR(1); S$
"#,
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec!["both 1", "shared 2", "element 9", "abab", "1  2  9 abab"]
    );
}

/// A BYVAL parameter, and an expression argument, are copies: the
/// procedure's writes go nowhere the caller can see.
#[test]
fn test_byval_and_expression_arguments_are_copies() {
    let output = compile_and_run(
        r#"
SUB Bump(BYVAL A, B)
  A = A + 1
  B = B + 1
END SUB
X = 1: Y = 1
Bump X, Y
Bump X, Y + 0
Bump X, Y * 1
PRINT X; Y
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "1  2");
}

/// A BYVAL parameter declared with `AS` reads as its argument, in a FUNCTION
/// declared with `AS` too -- recursive, so each call has its own.
#[test]
fn test_typed_byval_parameters() {
    let output = compile_and_run(
        r#"
FUNCTION FIB(BYVAL N AS LONG) AS LONG
IF N < 2 THEN FIB = N ELSE FIB = FIB(N - 1) + FIB(N - 2)
END FUNCTION
SUB SHOW(BYVAL X AS INTEGER, BYVAL D AS DOUBLE, BYVAL S$)
PRINT X; D; S$
END SUB
PRINT FIB(20)
SHOW 7, 2.5, "s"
"#,
    )
    .unwrap();
    assert_eq!(crate::common::lines(&output), vec!["6765", "7  2.5 s"]);
}
