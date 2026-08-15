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
