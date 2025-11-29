//! Type system tests (conversion, promotion, truncation)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

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

// ============================================
// Cross-type subtraction tests
// ============================================

#[test]
fn test_type_promotion_sub_integer_long() {
    let output = compile_and_run(
        r#"
A% = 50
B& = 20
PRINT A% - B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "30");
}

#[test]
fn test_type_promotion_sub_integer_single() {
    let output = compile_and_run(
        r#"
A% = 10
B! = 2.5
PRINT A% - B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "7.5");
}

#[test]
fn test_type_promotion_sub_integer_double() {
    let output = compile_and_run(
        r#"
A% = 10
B# = 3.25
PRINT A% - B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "6.75");
}

#[test]
fn test_type_promotion_sub_long_single() {
    let output = compile_and_run(
        r#"
A& = 100
B! = 0.5
PRINT A& - B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "99.5");
}

#[test]
fn test_type_promotion_sub_long_double() {
    let output = compile_and_run(
        r#"
A& = 100
B# = 0.25
PRINT A& - B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "99.75");
}

#[test]
fn test_type_promotion_sub_single_double() {
    let output = compile_and_run(
        r#"
A! = 5.5
B# = 2.25
PRINT A! - B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3.25");
}

// ============================================
// Cross-type multiplication tests
// ============================================

#[test]
fn test_type_promotion_mul_integer_long() {
    let output = compile_and_run(
        r#"
A% = 10
B& = 20
PRINT A% * B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "200");
}

#[test]
fn test_type_promotion_mul_integer_single() {
    let output = compile_and_run(
        r#"
A% = 4
B! = 2.5
PRINT A% * B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "10");
}

#[test]
fn test_type_promotion_mul_integer_double() {
    let output = compile_and_run(
        r#"
A% = 3
B# = 2.5
PRINT A% * B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "7.5");
}

#[test]
fn test_type_promotion_mul_long_single() {
    let output = compile_and_run(
        r#"
A& = 100
B! = 0.5
PRINT A& * B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "50");
}

#[test]
fn test_type_promotion_mul_long_double() {
    let output = compile_and_run(
        r#"
A& = 100
B# = 0.25
PRINT A& * B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "25");
}

#[test]
fn test_type_promotion_mul_single_double() {
    let output = compile_and_run(
        r#"
A! = 2.5
B# = 4.0
PRINT A! * B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "10");
}

// ============================================
// Cross-type division tests (always produces Double)
// ============================================

#[test]
fn test_type_promotion_div_integer_long() {
    let output = compile_and_run(
        r#"
A% = 7
B& = 2
PRINT A% / B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3.5");
}

#[test]
fn test_type_promotion_div_integer_single() {
    let output = compile_and_run(
        r#"
A% = 5
B! = 2.0
PRINT A% / B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "2.5");
}

#[test]
fn test_type_promotion_div_long_single() {
    let output = compile_and_run(
        r#"
A& = 9
B! = 2.0
PRINT A& / B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "4.5");
}

#[test]
fn test_type_promotion_div_long_double() {
    let output = compile_and_run(
        r#"
A& = 11
B# = 4.0
PRINT A& / B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "2.75");
}

#[test]
fn test_type_promotion_div_single_double() {
    let output = compile_and_run(
        r#"
A! = 7.0
B# = 2.0
PRINT A! / B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3.5");
}

// ============================================
// Cross-type integer division tests (always produces Long)
// ============================================

#[test]
fn test_type_promotion_intdiv_integer_long() {
    let output = compile_and_run(
        r#"
A% = 17
B& = 5
PRINT A% \ B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_type_promotion_intdiv_integer_single() {
    let output = compile_and_run(
        r#"
A% = 17
B! = 5.0
PRINT A% \ B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_type_promotion_intdiv_long_double() {
    let output = compile_and_run(
        r#"
A& = 25
B# = 7.0
PRINT A& \ B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_type_promotion_intdiv_single_double() {
    let output = compile_and_run(
        r#"
A! = 100.0
B# = 30.0
PRINT A! \ B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3");
}

// ============================================
// Cross-type MOD tests (always produces Long)
// ============================================

#[test]
fn test_type_promotion_mod_integer_long() {
    let output = compile_and_run(
        r#"
A% = 17
B& = 5
PRINT A% MOD B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "2");
}

#[test]
fn test_type_promotion_mod_integer_single() {
    let output = compile_and_run(
        r#"
A% = 17
B! = 5.0
PRINT A% MOD B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "2");
}

#[test]
fn test_type_promotion_mod_long_double() {
    let output = compile_and_run(
        r#"
A& = 25
B# = 7.0
PRINT A& MOD B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "4");
}

#[test]
fn test_type_promotion_mod_single_double() {
    let output = compile_and_run(
        r#"
A! = 100.0
B# = 30.0
PRINT A! MOD B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "10");
}

// ============================================
// Cross-type power tests (always produces Double)
// ============================================

#[test]
fn test_type_promotion_pow_integer_long() {
    let output = compile_and_run(
        r#"
A% = 2
B& = 8
PRINT A% ^ B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "256");
}

#[test]
fn test_type_promotion_pow_integer_single() {
    let output = compile_and_run(
        r#"
A% = 4
B! = 0.5
PRINT A% ^ B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "2");
}

#[test]
fn test_type_promotion_pow_integer_double() {
    let output = compile_and_run(
        r#"
A% = 2
B# = 3.0
PRINT A% ^ B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "8");
}

#[test]
fn test_type_promotion_pow_long_single() {
    let output = compile_and_run(
        r#"
A& = 9
B! = 0.5
PRINT A& ^ B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_type_promotion_pow_long_double() {
    let output = compile_and_run(
        r#"
A& = 3
B# = 4.0
PRINT A& ^ B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "81");
}

#[test]
fn test_type_promotion_pow_single_double() {
    let output = compile_and_run(
        r#"
A! = 2.0
B# = 10.0
PRINT A! ^ B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "1024");
}
