//! Arithmetic and operator tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_basic_arithmetic() {
    // Tests: add, sub, mul, division, integer_division, mod, power
    let output = compile_and_run(
        r#"
PRINT 10 + 5
PRINT 10 - 3
PRINT 6 * 7
PRINT 10 / 4
PRINT 10 \ 4
PRINT 10 MOD 3
PRINT 2 ^ 10
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "15", "add");
    assert_eq!(lines[1], "7", "sub");
    assert_eq!(lines[2], "42", "mul");
    assert_eq!(lines[3], "2.5", "division");
    assert_eq!(lines[4], "2", "integer division");
    assert_eq!(lines[5], "1", "mod");
    assert_eq!(lines[6], "1024", "power");
}

#[test]
fn test_expressions() {
    // Tests: precedence, parentheses, negative numbers
    let output = compile_and_run(
        r#"
PRINT 2 + 3 * 4
PRINT (2 + 3) * 4
PRINT -5 + 10
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "14", "precedence");
    assert_eq!(lines[1], "20", "parentheses");
    assert_eq!(lines[2], "5", "negative");
}

#[test]
fn test_logical_operators() {
    // Tests: AND, OR, NOT, XOR
    let output = compile_and_run(
        r#"
IF 1 AND 1 THEN PRINT "and-yes"
IF 1 AND 0 THEN PRINT "and-no"
IF 0 OR 1 THEN PRINT "or-yes"
IF 0 OR 0 THEN PRINT "or-no"
IF NOT 0 THEN PRINT "not-yes"
IF NOT 1 THEN PRINT "not-no"
IF 1 XOR 0 THEN PRINT "xor-a"
IF 0 XOR 1 THEN PRINT "xor-b"
IF 1 XOR 1 THEN PRINT "xor-c"
IF 0 XOR 0 THEN PRINT "xor-d"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        vec!["and-yes", "or-yes", "not-yes", "xor-a", "xor-b"]
    );
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

#[test]
fn test_integer_arithmetic() {
    // Tests: Integer (%) add, sub, mul, div, intdiv, mod, power, neg
    let output = compile_and_run(
        r#"
A% = 100: B% = 50: PRINT A% + B%
A% = 100: B% = 30: PRINT A% - B%
A% = 12: B% = 5: PRINT A% * B%
A% = 7: B% = 2: PRINT A% / B%
A% = 17: B% = 5: PRINT A% \ B%
A% = 17: B% = 5: PRINT A% MOD B%
A% = 2: B% = 8: PRINT A% ^ B%
A% = 42: PRINT -A%
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "150", "int add");
    assert_eq!(lines[1], "70", "int sub");
    assert_eq!(lines[2], "60", "int mul");
    assert_eq!(lines[3], "3.5", "int div");
    assert_eq!(lines[4], "3", "int intdiv");
    assert_eq!(lines[5], "2", "int mod");
    assert_eq!(lines[6], "256", "int power");
    assert_eq!(lines[7], "-42", "int neg");
}

#[test]
fn test_long_arithmetic() {
    // Tests: Long (&) add, sub, mul, div, intdiv, mod, power, neg
    let output = compile_and_run(
        r#"
A& = 100000: B& = 50000: PRINT A& + B&
A& = 100000: B& = 30000: PRINT A& - B&
A& = 1000: B& = 500: PRINT A& * B&
A& = 7: B& = 2: PRINT A& / B&
A& = 100: B& = 30: PRINT A& \ B&
A& = 100: B& = 30: PRINT A& MOD B&
A& = 3: B& = 5: PRINT A& ^ B&
A& = 12345: PRINT -A&
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "150000", "long add");
    assert_eq!(lines[1], "70000", "long sub");
    assert_eq!(lines[2], "500000", "long mul");
    assert_eq!(lines[3], "3.5", "long div");
    assert_eq!(lines[4], "3", "long intdiv");
    assert_eq!(lines[5], "10", "long mod");
    assert_eq!(lines[6], "243", "long power");
    assert_eq!(lines[7], "-12345", "long neg");
}

#[test]
fn test_single_arithmetic() {
    // Tests: Single (!) add, sub, mul, div, power, neg
    let output = compile_and_run(
        r#"
A! = 1.5: B! = 2.5: PRINT A! + B!
A! = 5.5: B! = 2.25: PRINT A! - B!
A! = 2.5: B! = 4.0: PRINT A! * B!
A! = 10.0: B! = 4.0: PRINT A! / B!
A! = 2.0: B! = 3.0: PRINT A! ^ B!
A! = 3.14: PRINT -A!
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "4", "single add");
    assert_eq!(lines[1], "3.25", "single sub");
    assert_eq!(lines[2], "10", "single mul");
    assert_eq!(lines[3], "2.5", "single div");
    assert_eq!(lines[4], "8", "single power");
    assert_eq!(lines[5], "-3.14", "single neg");
}

#[test]
fn test_double_arithmetic() {
    // Tests: Double (#) add, sub, mul, div, power, neg
    let output = compile_and_run(
        r#"
A# = 1.5: B# = 2.5: PRINT A# + B#
A# = 100.75: B# = 50.25: PRINT A# - B#
A# = 3.5: B# = 2.0: PRINT A# * B#
A# = 15.0: B# = 4.0: PRINT A# / B#
A# = 2.0: B# = 10.0: PRINT A# ^ B#
A# = 2.71828: PRINT -A#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "4", "double add");
    assert_eq!(lines[1], "50.5", "double sub");
    assert_eq!(lines[2], "7", "double mul");
    assert_eq!(lines[3], "3.75", "double div");
    assert_eq!(lines[4], "1024", "double power");
    assert_eq!(lines[5], "-2.71828", "double neg");
}
