//! Type system tests (conversion, promotion, truncation)

use crate::common::compile_and_run;

// === Conversion Function Tests ===

#[test]
fn test_cint_clng() {
    let output = compile_and_run(
        r#"
PRINT CINT(3.7)
PRINT CLNG(3.7)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["4", "4"]);
}

#[test]
fn test_csng_cdbl() {
    // These convert to float types; test that they work
    let output = compile_and_run(
        r#"
X! = CSNG(3)
Y# = CDBL(3)
PRINT X! + Y#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "6");
}

// === Type Promotion Tests ===

#[test]
fn test_type_promotion_integer_long() {
    // Integer + Long should promote to Long
    let output = compile_and_run(
        r#"
A% = 100
B& = 200
PRINT A% + B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "300");
}

#[test]
fn test_type_promotion_integer_single() {
    // Integer + Single should promote to Single
    let output = compile_and_run(
        r#"
A% = 10
B! = 2.5
PRINT A% + B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "12.5");
}

#[test]
fn test_type_promotion_integer_double() {
    // Integer + Double should promote to Double
    let output = compile_and_run(
        r#"
A% = 10
B# = 2.5
PRINT A% + B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "12.5");
}

#[test]
fn test_type_promotion_long_single() {
    // Long + Single should promote to Single
    let output = compile_and_run(
        r#"
A& = 100
B! = 0.5
PRINT A& + B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "100.5");
}

#[test]
fn test_type_promotion_long_double() {
    // Long + Double should promote to Double
    let output = compile_and_run(
        r#"
A& = 100
B# = 0.25
PRINT A& + B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "100.25");
}

#[test]
fn test_type_promotion_single_double() {
    // Single + Double should promote to Double
    let output = compile_and_run(
        r#"
A! = 1.5
B# = 2.5
PRINT A! + B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "4");
}

// === Cross-Type Assignment Tests ===

#[test]
fn test_cross_type_assignment_int_to_long() {
    // Assign Integer to Long variable
    let output = compile_and_run(
        r#"
A% = 42
B& = A%
PRINT B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_cross_type_assignment_int_to_double() {
    // Assign Integer to Double variable
    let output = compile_and_run(
        r#"
A% = 42
B# = A%
PRINT B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_cross_type_assignment_double_to_int() {
    // Assign Double to Integer variable (truncation)
    let output = compile_and_run(
        r#"
A# = 3.7
B% = A#
PRINT B%
"#,
    )
    .unwrap();
    // Truncates toward zero
    assert_eq!(output.trim(), "3");
}

// === Truncation Tests ===

#[test]
fn test_truncation_positive() {
    // Positive float truncation
    let output = compile_and_run(
        r#"
PRINT CINT(3.1)
PRINT CINT(3.5)
PRINT CINT(3.9)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    // CINT uses banker's rounding (round half to even)
    assert_eq!(lines[0], "3");
    assert_eq!(lines[1], "4");
    assert_eq!(lines[2], "4");
}

#[test]
fn test_truncation_negative() {
    // Negative float truncation
    let output = compile_and_run(
        r#"
PRINT CINT(-3.1)
PRINT CINT(-3.5)
PRINT CINT(-3.9)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    // CINT rounds toward nearest
    assert_eq!(lines[0], "-3");
    assert_eq!(lines[1], "-4");
    assert_eq!(lines[2], "-4");
}

// === Division Tests ===

#[test]
fn test_division_produces_double() {
    // Division (/) always produces Double
    let output = compile_and_run(
        r#"
A% = 7
B% = 2
PRINT A% / B%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3.5");
}

#[test]
fn test_integer_division() {
    // Integer division (\) produces Long
    let output = compile_and_run(
        r#"
A% = 7
B% = 2
PRINT A% \ B%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3");
}

// === Complex Expression Tests ===

#[test]
fn test_mixed_expression_complex() {
    // Complex expression with multiple type promotions
    let output = compile_and_run(
        r#"
A% = 10
B& = 20
C! = 0.5
D# = 100.0
PRINT A% + B& * C! + D#
"#,
    )
    .unwrap();
    // 10 + 20*0.5 + 100 = 10 + 10 + 100 = 120
    assert_eq!(output.trim(), "120");
}
