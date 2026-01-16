//! Function and subroutine tests

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_function_definition() {
    let output = compile_and_run(
        r#"
FUNCTION Double(X)
    Double = X * 2
END FUNCTION

PRINT Double(21)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_sub_definition() {
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
fn test_sub_with_params() {
    let output = compile_and_run(
        r#"
AddPrint(10, 20)
END

SUB AddPrint(A, B)
    PRINT A + B
END SUB
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "30");
}

#[test]
fn test_nested_fn_args() {
    // Test that nested function calls in arguments don't clobber registers
    let output = compile_and_run(
        r#"
FUNCTION Add(A, B)
    Add = A + B
END FUNCTION

FUNCTION Mul(A, B)
    Mul = A * B
END FUNCTION

PRINT Add(Mul(2, 3), Mul(4, 5))
"#,
    )
    .unwrap();
    // 2*3=6, 4*5=20, 6+20=26
    assert_eq!(output.trim(), "26");
}

#[test]
fn test_seven_params() {
    // Test procedure with 7 parameters (1 more than max register args on SysV)
    let output = compile_and_run(
        r#"
SUB Sum7(A, B, C, D, E, F, G)
    PRINT A + B + C + D + E + F + G
END SUB

Sum7(1, 2, 3, 4, 5, 6, 7)
"#,
    )
    .unwrap();
    // 1+2+3+4+5+6+7 = 28
    assert_eq!(output.trim(), "28");
}

#[test]
fn test_eight_params() {
    // Test function with 8 parameters (2 more than max register args on SysV)
    let output = compile_and_run(
        r#"
FUNCTION Sum8(A, B, C, D, E, F, G, H)
    Sum8 = A + B + C + D + E + F + G + H
END FUNCTION

PRINT Sum8(1, 2, 3, 4, 5, 6, 7, 8)
"#,
    )
    .unwrap();
    // 1+2+3+4+5+6+7+8 = 36
    assert_eq!(output.trim(), "36");
}

#[test]
fn test_ten_params() {
    // Test with 10 parameters to ensure multiple overflow args work
    let output = compile_and_run(
        r#"
FUNCTION Sum10(A, B, C, D, E, F, G, H, I, J)
    Sum10 = A + B + C + D + E + F + G + H + I + J
END FUNCTION

PRINT Sum10(1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
"#,
    )
    .unwrap();
    // 1+2+3+4+5+6+7+8+9+10 = 55
    assert_eq!(output.trim(), "55");
}

#[test]
fn test_nested_fn_many_params() {
    // Test nested function calls with many parameters
    let output = compile_and_run(
        r#"
FUNCTION AddThree(A, B, C)
    AddThree = A + B + C
END FUNCTION

FUNCTION MulTwo(A, B)
    MulTwo = A * B
END FUNCTION

PRINT AddThree(MulTwo(2, 3), MulTwo(4, 5), MulTwo(6, 7))
"#,
    )
    .unwrap();
    // 2*3=6, 4*5=20, 6*7=42, 6+20+42=68
    assert_eq!(output.trim(), "68");
}
