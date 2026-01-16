//! Type system tests (conversion, promotion, truncation)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_type_conversions() {
    // CINT, CLNG, CSNG, CDBL conversion functions
    let output = compile_and_run(
        r#"
PRINT CINT(3.7)
PRINT CLNG(3.7)
X! = CSNG(3): PRINT X!
Y# = CDBL(3): PRINT Y#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "4", "cint rounds");
    assert_eq!(lines[1], "4", "clng rounds");
    assert_eq!(lines[2], "3", "csng");
    assert_eq!(lines[3], "3", "cdbl");
}

#[test]
fn test_truncation_and_assignment() {
    // Truncation and cross-type assignments
    let output = compile_and_run(
        r#"
PRINT CINT(3.1)
PRINT CINT(3.5)
PRINT CINT(3.9)
PRINT CINT(-3.1)
PRINT CINT(-3.5)
PRINT CINT(-3.9)
A% = 42: B& = A%: PRINT B&
A% = 42: B# = A%: PRINT B#
A# = 3.7: B% = A#: PRINT B%
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "3", "cint 3.1");
    assert_eq!(lines[1], "4", "cint 3.5");
    assert_eq!(lines[2], "4", "cint 3.9");
    assert_eq!(lines[3], "-3", "cint -3.1");
    assert_eq!(lines[4], "-4", "cint -3.5");
    assert_eq!(lines[5], "-4", "cint -3.9");
    assert_eq!(lines[6], "42", "int to long");
    assert_eq!(lines[7], "42", "int to double");
    assert_eq!(lines[8], "3", "double to int truncates");
}

#[test]
fn test_division_types() {
    // Division (/) always produces Double, integer division (\) produces Long
    let output = compile_and_run(
        r#"
A% = 7: B% = 2: PRINT A% / B%
A% = 7: B% = 2: PRINT A% \ B%
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "3.5", "division produces double");
    assert_eq!(lines[1], "3", "integer division");
}

#[test]
fn test_promotion_add() {
    // Type promotion for addition: int+long, int+single, int+double, long+single, long+double, single+double
    let output = compile_and_run(
        r#"
A% = 100: B& = 200: PRINT A% + B&
A% = 10: B! = 2.5: PRINT A% + B!
A% = 10: B# = 2.5: PRINT A% + B#
A& = 100: B! = 0.5: PRINT A& + B!
A& = 100: B# = 0.25: PRINT A& + B#
A! = 1.5: B# = 2.5: PRINT A! + B#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "300", "int+long");
    assert_eq!(lines[1], "12.5", "int+single");
    assert_eq!(lines[2], "12.5", "int+double");
    assert_eq!(lines[3], "100.5", "long+single");
    assert_eq!(lines[4], "100.25", "long+double");
    assert_eq!(lines[5], "4", "single+double");
}

#[test]
fn test_promotion_sub() {
    // Type promotion for subtraction
    let output = compile_and_run(
        r#"
A% = 50: B& = 20: PRINT A% - B&
A% = 10: B! = 2.5: PRINT A% - B!
A% = 10: B# = 3.25: PRINT A% - B#
A& = 100: B! = 0.5: PRINT A& - B!
A& = 100: B# = 0.25: PRINT A& - B#
A! = 5.5: B# = 2.25: PRINT A! - B#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "30", "int-long");
    assert_eq!(lines[1], "7.5", "int-single");
    assert_eq!(lines[2], "6.75", "int-double");
    assert_eq!(lines[3], "99.5", "long-single");
    assert_eq!(lines[4], "99.75", "long-double");
    assert_eq!(lines[5], "3.25", "single-double");
}

#[test]
fn test_promotion_mul() {
    // Type promotion for multiplication
    let output = compile_and_run(
        r#"
A% = 10: B& = 20: PRINT A% * B&
A% = 4: B! = 2.5: PRINT A% * B!
A% = 3: B# = 2.5: PRINT A% * B#
A& = 100: B! = 0.5: PRINT A& * B!
A& = 100: B# = 0.25: PRINT A& * B#
A! = 2.5: B# = 4.0: PRINT A! * B#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "200", "int*long");
    assert_eq!(lines[1], "10", "int*single");
    assert_eq!(lines[2], "7.5", "int*double");
    assert_eq!(lines[3], "50", "long*single");
    assert_eq!(lines[4], "25", "long*double");
    assert_eq!(lines[5], "10", "single*double");
}

#[test]
fn test_promotion_div_intdiv_mod() {
    // Type promotion for division, integer division, and mod
    let output = compile_and_run(
        r#"
A% = 7: B& = 2: PRINT A% / B&
A% = 5: B! = 2.0: PRINT A% / B!
A& = 9: B! = 2.0: PRINT A& / B!
A& = 11: B# = 4.0: PRINT A& / B#
A! = 7.0: B# = 2.0: PRINT A! / B#
A% = 17: B& = 5: PRINT A% \ B&
A% = 17: B! = 5.0: PRINT A% \ B!
A& = 25: B# = 7.0: PRINT A& \ B#
A! = 100.0: B# = 30.0: PRINT A! \ B#
A% = 17: B& = 5: PRINT A% MOD B&
A% = 17: B! = 5.0: PRINT A% MOD B!
A& = 25: B# = 7.0: PRINT A& MOD B#
A! = 100.0: B# = 30.0: PRINT A! MOD B#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "3.5", "int/long");
    assert_eq!(lines[1], "2.5", "int/single");
    assert_eq!(lines[2], "4.5", "long/single");
    assert_eq!(lines[3], "2.75", "long/double");
    assert_eq!(lines[4], "3.5", "single/double");
    assert_eq!(lines[5], "3", "int\\long");
    assert_eq!(lines[6], "3", "int\\single");
    assert_eq!(lines[7], "3", "long\\double");
    assert_eq!(lines[8], "3", "single\\double");
    assert_eq!(lines[9], "2", "int mod long");
    assert_eq!(lines[10], "2", "int mod single");
    assert_eq!(lines[11], "4", "long mod double");
    assert_eq!(lines[12], "10", "single mod double");
}

#[test]
fn test_promotion_pow() {
    // Type promotion for power operation
    let output = compile_and_run(
        r#"
A% = 2: B& = 8: PRINT A% ^ B&
A% = 4: B! = 0.5: PRINT A% ^ B!
A% = 2: B# = 3.0: PRINT A% ^ B#
A& = 9: B! = 0.5: PRINT A& ^ B!
A& = 3: B# = 4.0: PRINT A& ^ B#
A! = 2.0: B# = 10.0: PRINT A! ^ B#
A% = 10: B& = 20: C! = 0.5: D# = 100.0: PRINT A% + B& * C! + D#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "256", "int^long");
    assert_eq!(lines[1], "2", "int^single");
    assert_eq!(lines[2], "8", "int^double");
    assert_eq!(lines[3], "3", "long^single");
    assert_eq!(lines[4], "81", "long^double");
    assert_eq!(lines[5], "1024", "single^double");
    assert_eq!(lines[6], "120", "mixed expression");
}
