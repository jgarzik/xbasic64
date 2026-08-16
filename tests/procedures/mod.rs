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
    let lines: Vec<&str> = output.trim().lines().collect();
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
    let lines: Vec<&str> = output.trim().lines().collect();
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
    let lines: Vec<&str> = output.trim().lines().collect();
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
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "26", "nested: 2*3 + 4*5 = 6+20");
    assert_eq!(lines[1], "68", "nested three: 6+20+42");
}

/// LANGREF documents module-level variables as "global by default: accessible
/// everywhere". They used to read as 0 inside a procedure, because procedures
/// were compiled before main and so allocated their own local instead.
#[test]
fn test_module_variables_visible_in_procedures() {
    let output = compile_and_run(
        r#"
G = 99
SUB Bump
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
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["99", "100", "100", "101"], "shared storage");
}

/// An array DIM'd at module level must be usable inside a procedure. This used
/// to abort the compiler with "Array not declared".
#[test]
fn test_module_array_accessible_in_procedure() {
    let output = compile_and_run(
        r#"
DIM A(5)
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
    let lines: Vec<&str> = output.trim().lines().collect();
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
    let lines: Vec<&str> = output.trim().lines().collect();
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
    let lines: Vec<&str> = output.trim().lines().collect();
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
    let lines: Vec<&str> = output.trim().lines().collect();
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
    let lines: Vec<&str> = output.trim().lines().collect();
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
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["abc", "1x2y3z"]);
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
    let lines: Vec<&str> = output.trim().lines().collect();
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
    let lines: Vec<&str> = output.trim().lines().collect();
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
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["bob", "42", "eq"]);
}

/// String parameters are by value, like numeric ones.
#[test]
fn test_string_parameters_are_by_value() {
    let output = compile_and_run(
        r#"
SUB Change(S$)
S$ = "changed"
PRINT S$
END SUB
T$ = "original"
Change(T$)
PRINT T$
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["changed", "original"]);
}

/// DEF FN is desugared into an ordinary FUNCTION, so it reuses the whole
/// procedure path and evaluates each argument exactly once.
#[test]
fn test_def_fn() {
    let output = compile_and_run(
        "DEF FNA(X) = X * 2\nDEF FNSUM(A, B) = A + B\nDEF FNPI = 3.14159\nDEF FNG$(N$) = \"hi \" + N$\nPRINT FNA(5)\nPRINT FNSUM(3, 4)\nPRINT FNPI\nPRINT FNG$(\"bob\")\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
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
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1030", "13"]);
}

/// The default base is still 0.
#[test]
fn test_option_base_defaults_to_zero() {
    let output = compile_and_run("DIM A(3)\nA(0) = 5\nPRINT A(0); LBOUND(A)\n").unwrap();
    assert_eq!(output.trim(), "50");
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
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["3", "3000"]);
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
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, &["n=7", "plain", "n=8"]);
}

/// CALL is recognised only in statement position before a name, so a program
/// may still use it as a variable -- the same rule the random-access statement
/// names follow.
#[test]
fn test_call_is_not_reserved() {
    let output = compile_and_run("CALL = 5\nPRINT CALL\n").unwrap();
    assert_eq!(output.trim(), "5");
}
