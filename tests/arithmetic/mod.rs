//! Arithmetic and operator tests

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_arithmetic_add() {
    let output = compile_and_run("PRINT 10 + 5").unwrap();
    assert_eq!(output.trim(), "15");
}

#[test]
fn test_arithmetic_sub() {
    let output = compile_and_run("PRINT 10 - 3").unwrap();
    assert_eq!(output.trim(), "7");
}

#[test]
fn test_arithmetic_mul() {
    let output = compile_and_run("PRINT 6 * 7").unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_expression_precedence() {
    let output = compile_and_run("PRINT 2 + 3 * 4").unwrap();
    assert_eq!(output.trim(), "14");
}

#[test]
fn test_parentheses() {
    let output = compile_and_run("PRINT (2 + 3) * 4").unwrap();
    assert_eq!(output.trim(), "20");
}

#[test]
fn test_negative_numbers() {
    let output = compile_and_run("PRINT -5 + 10").unwrap();
    assert_eq!(output.trim(), "5");
}

#[test]
fn test_arithmetic_division() {
    let output = compile_and_run("PRINT 10 / 4").unwrap();
    assert_eq!(output.trim(), "2.5");
}

#[test]
fn test_arithmetic_integer_division() {
    let output = compile_and_run("PRINT 10 \\ 4").unwrap();
    assert_eq!(output.trim(), "2");
}

#[test]
fn test_arithmetic_mod() {
    let output = compile_and_run("PRINT 10 MOD 3").unwrap();
    assert_eq!(output.trim(), "1");
}

#[test]
fn test_arithmetic_power() {
    let output = compile_and_run("PRINT 2 ^ 10").unwrap();
    assert_eq!(output.trim(), "1024");
}

#[test]
fn test_logical_and() {
    let output = compile_and_run(
        r#"
IF 1 AND 1 THEN PRINT "yes"
IF 1 AND 0 THEN PRINT "no"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "yes");
}

#[test]
fn test_logical_or() {
    let output = compile_and_run(
        r#"
IF 0 OR 1 THEN PRINT "yes"
IF 0 OR 0 THEN PRINT "no"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "yes");
}

#[test]
fn test_logical_not() {
    let output = compile_and_run(
        r#"
IF NOT 0 THEN PRINT "yes"
IF NOT 1 THEN PRINT "no"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "yes");
}

#[test]
fn test_logical_xor() {
    let output = compile_and_run(
        r#"
IF 1 XOR 0 THEN PRINT "a"
IF 0 XOR 1 THEN PRINT "b"
IF 1 XOR 1 THEN PRINT "c"
IF 0 XOR 0 THEN PRINT "d"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["a", "b"]);
}

#[test]
fn test_comparison_operators() {
    let output = compile_and_run(
        r#"
IF 5 < 10 THEN PRINT "ok1"
IF 10 > 5 THEN PRINT "ok2"
IF 5 <= 5 THEN PRINT "ok3"
IF 5 >= 5 THEN PRINT "ok4"
IF 5 = 5 THEN PRINT "ok5"
IF 5 <> 6 THEN PRINT "ok6"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines.len(), 6);
}

// ============================================
// Same-type arithmetic tests for Integer (%)
// ============================================

#[test]
fn test_integer_add() {
    let output = compile_and_run(
        r#"
A% = 100
B% = 50
PRINT A% + B%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "150");
}

#[test]
fn test_integer_sub() {
    let output = compile_and_run(
        r#"
A% = 100
B% = 30
PRINT A% - B%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "70");
}

#[test]
fn test_integer_mul() {
    let output = compile_and_run(
        r#"
A% = 12
B% = 5
PRINT A% * B%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "60");
}

#[test]
fn test_integer_div() {
    // Division always produces Double
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
fn test_integer_intdiv() {
    let output = compile_and_run(
        r#"
A% = 17
B% = 5
PRINT A% \ B%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_integer_mod() {
    let output = compile_and_run(
        r#"
A% = 17
B% = 5
PRINT A% MOD B%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "2");
}

#[test]
fn test_integer_power() {
    let output = compile_and_run(
        r#"
A% = 2
B% = 8
PRINT A% ^ B%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "256");
}

#[test]
fn test_integer_neg() {
    let output = compile_and_run(
        r#"
A% = 42
PRINT -A%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "-42");
}

// ============================================
// Same-type arithmetic tests for Long (&)
// ============================================

#[test]
fn test_long_add() {
    let output = compile_and_run(
        r#"
A& = 100000
B& = 50000
PRINT A& + B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "150000");
}

#[test]
fn test_long_sub() {
    let output = compile_and_run(
        r#"
A& = 100000
B& = 30000
PRINT A& - B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "70000");
}

#[test]
fn test_long_mul() {
    let output = compile_and_run(
        r#"
A& = 1000
B& = 500
PRINT A& * B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "500000");
}

#[test]
fn test_long_div() {
    let output = compile_and_run(
        r#"
A& = 7
B& = 2
PRINT A& / B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3.5");
}

#[test]
fn test_long_intdiv() {
    let output = compile_and_run(
        r#"
A& = 100
B& = 30
PRINT A& \ B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_long_mod() {
    let output = compile_and_run(
        r#"
A& = 100
B& = 30
PRINT A& MOD B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "10");
}

#[test]
fn test_long_power() {
    let output = compile_and_run(
        r#"
A& = 3
B& = 5
PRINT A& ^ B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "243");
}

#[test]
fn test_long_neg() {
    let output = compile_and_run(
        r#"
A& = 12345
PRINT -A&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "-12345");
}

// ============================================
// Same-type arithmetic tests for Single (!)
// ============================================

#[test]
fn test_single_add() {
    let output = compile_and_run(
        r#"
A! = 1.5
B! = 2.5
PRINT A! + B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "4");
}

#[test]
fn test_single_sub() {
    let output = compile_and_run(
        r#"
A! = 5.5
B! = 2.25
PRINT A! - B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3.25");
}

#[test]
fn test_single_mul() {
    let output = compile_and_run(
        r#"
A! = 2.5
B! = 4.0
PRINT A! * B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "10");
}

#[test]
fn test_single_div() {
    let output = compile_and_run(
        r#"
A! = 10.0
B! = 4.0
PRINT A! / B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "2.5");
}

#[test]
fn test_single_power() {
    let output = compile_and_run(
        r#"
A! = 2.0
B! = 3.0
PRINT A! ^ B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "8");
}

#[test]
fn test_single_neg() {
    let output = compile_and_run(
        r#"
A! = 3.14
PRINT -A!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "-3.14");
}

// ============================================
// Same-type arithmetic tests for Double (#)
// ============================================

#[test]
fn test_double_add() {
    let output = compile_and_run(
        r#"
A# = 1.5
B# = 2.5
PRINT A# + B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "4");
}

#[test]
fn test_double_sub() {
    let output = compile_and_run(
        r#"
A# = 100.75
B# = 50.25
PRINT A# - B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "50.5");
}

#[test]
fn test_double_mul() {
    let output = compile_and_run(
        r#"
A# = 3.5
B# = 2.0
PRINT A# * B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "7");
}

#[test]
fn test_double_div() {
    let output = compile_and_run(
        r#"
A# = 15.0
B# = 4.0
PRINT A# / B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3.75");
}

#[test]
fn test_double_power() {
    let output = compile_and_run(
        r#"
A# = 2.0
B# = 10.0
PRINT A# ^ B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "1024");
}

#[test]
fn test_double_neg() {
    let output = compile_and_run(
        r#"
A# = 2.71828
PRINT -A#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "-2.71828");
}
