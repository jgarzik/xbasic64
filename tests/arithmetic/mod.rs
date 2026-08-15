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

/// LANGREF's precedence table puts `^` above unary minus, as does GW-BASIC, so
/// -2^2 is -(2^2). Unary minus used to bind tighter, giving 4.
#[test]
fn test_unary_minus_vs_power_precedence() {
    let output = compile_and_run(
        r#"
PRINT -2 ^ 2
PRINT -2 ^ 3
PRINT (-2) ^ 2
PRINT 0 - 2 ^ 2
A = 3
PRINT -A ^ 2
PRINT 2 ^ 3 ^ 2
PRINT -2 * 3
PRINT 1 - -2
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        vec!["-4", "-8", "4", "-4", "-9", "512", "-6", "3"],
        "^ binds tighter than unary minus; ^ stays right-associative"
    );
}

/// Radix literals. Only &H was recognized, so &O17 lexed as two identifiers and
/// silently produced garbage.
#[test]
fn test_radix_literals() {
    let output = compile_and_run(
        r#"
PRINT &HFF
PRINT &O17
PRINT &B1010
PRINT &17
PRINT &HFFFFFFFF
PRINT &H10 + &H10
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        vec!["255", "15", "10", "15", "-1", "32"],
        "hex, octal, binary, bare-& octal; &H values are 32-bit signed"
    );
}

/// An integer literal wider than LONG becomes a Double rather than wrapping.
/// PRINT 1000000000000001 used to print -1530494975.
#[test]
fn test_wide_integer_literals() {
    let output = compile_and_run(
        r#"
PRINT 1000000000000001
PRINT 3000000000
PRINT 2147483647
PRINT -2147483648
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        vec![
            "1000000000000001",
            "3000000000",
            "2147483647",
            "-2147483648"
        ]
    );
}

/// Doubles print the shortest decimal that reads back unchanged. %g's 6
/// significant digits turned 1/3 into 0.333333 and pi into 3.14159.
#[test]
fn test_double_precision_output() {
    let output = compile_and_run(
        r#"
PRINT 1 / 3
PRINT 2 / 3
PRINT 4 * ATN(1)
PRINT SQR(2)
PRINT 3.14159
PRINT 0.1
PRINT 123456789.123456
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        vec![
            "0.3333333333333333",
            "0.6666666666666666",
            "3.141592653589793",
            "1.4142135623730951",
            // Values that are exact at fewer digits keep their short form.
            "3.14159",
            "0.1",
            "123456789.123456",
        ]
    );
}

/// A SINGLE carries ~7 significant digits, so it must not be printed with a
/// Double's digits: 3.14159! would otherwise show as 3.1415901184082.
#[test]
fn test_single_precision_output() {
    let output = compile_and_run(
        r#"
A! = 3.14159
PRINT A!
B! = 0.1
PRINT B!
C! = -3.14
PRINT ABS(C!)
PRINT CSNG(1 / 3)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["3.14159", "0.1", "3.14", "0.33333334"]);
}
