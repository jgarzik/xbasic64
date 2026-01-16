//! Math function tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_sqr() {
    // SQR with various input types
    let output = compile_and_run(
        r#"
PRINT SQR(16)
A% = 25: PRINT SQR(A%)
A& = 10000: PRINT SQR(A&)
A! = 2.25: PRINT SQR(A!)
A# = 2.25: PRINT SQR(A#)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "4", "sqr literal");
    assert_eq!(lines[1], "5", "sqr integer");
    assert_eq!(lines[2], "100", "sqr long");
    assert_eq!(lines[3], "1.5", "sqr single");
    assert_eq!(lines[4], "1.5", "sqr double");
}

#[test]
fn test_abs() {
    // ABS with various input types
    let output = compile_and_run(
        r#"
PRINT ABS(-42)
A% = -42: PRINT ABS(A%)
A& = -100000: PRINT ABS(A&)
A! = -3.14: PRINT ABS(A!)
A# = -3.14159: PRINT ABS(A#)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "42", "abs literal");
    assert_eq!(lines[1], "42", "abs integer");
    assert_eq!(lines[2], "100000", "abs long");
    assert_eq!(lines[3], "3.14", "abs single");
    assert_eq!(lines[4], "3.14159", "abs double");
}

#[test]
fn test_int_fix() {
    // INT floors, FIX truncates toward zero
    let output = compile_and_run(
        r#"
PRINT INT(3.7)
A! = 3.7: PRINT INT(A!)
A# = 3.7: PRINT INT(A#)
PRINT FIX(-3.7)
A! = -3.7: PRINT FIX(A!)
A# = -3.7: PRINT FIX(A#)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "3", "int literal");
    assert_eq!(lines[1], "3", "int single");
    assert_eq!(lines[2], "3", "int double");
    assert_eq!(lines[3], "-3", "fix literal");
    assert_eq!(lines[4], "-3", "fix single");
    assert_eq!(lines[5], "-3", "fix double");
}

#[test]
fn test_sgn() {
    // SGN with various input types
    let output = compile_and_run(
        r#"
PRINT SGN(-5)
PRINT SGN(0)
PRINT SGN(5)
A% = -5: PRINT SGN(A%)
A& = -50000: PRINT SGN(A&)
A! = -2.5: PRINT SGN(A!)
A# = -2.5: PRINT SGN(A#)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "-1", "sgn neg");
    assert_eq!(lines[1], "0", "sgn zero");
    assert_eq!(lines[2], "1", "sgn pos");
    assert_eq!(lines[3], "-1", "sgn integer");
    assert_eq!(lines[4], "-1", "sgn long");
    assert_eq!(lines[5], "-1", "sgn single");
    assert_eq!(lines[6], "-1", "sgn double");
}

#[test]
fn test_trig_sin_cos() {
    // SIN and COS with various input types
    let output = compile_and_run(
        r#"
PRINT INT(SIN(0) * 100), INT(COS(0) * 100)
A% = 0: PRINT INT(SIN(A%) * 100)
A% = 0: PRINT INT(COS(A%) * 100)
A& = 0: PRINT INT(SIN(A&) * 100)
A& = 0: PRINT INT(COS(A&) * 100)
A! = 0.0: PRINT INT(SIN(A!) * 100)
A! = 0.0: PRINT INT(COS(A!) * 100)
A# = 0.0: PRINT INT(SIN(A#) * 100)
A# = 0.0: PRINT INT(COS(A#) * 100)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    let values: Vec<&str> = lines[0].split_whitespace().collect();
    assert_eq!(values, vec!["0", "100"], "sin/cos literals");
    assert_eq!(lines[1], "0", "sin integer");
    assert_eq!(lines[2], "100", "cos integer");
    assert_eq!(lines[3], "0", "sin long");
    assert_eq!(lines[4], "100", "cos long");
    assert_eq!(lines[5], "0", "sin single");
    assert_eq!(lines[6], "100", "cos single");
    assert_eq!(lines[7], "0", "sin double");
    assert_eq!(lines[8], "100", "cos double");
}

#[test]
fn test_trig_tan_atn() {
    // TAN and ATN with various input types
    let output = compile_and_run(
        r#"
PRINT INT(TAN(0) * 100), INT(ATN(0) * 100)
A! = 0.0: PRINT INT(TAN(A!) * 100)
A! = 0.0: PRINT INT(ATN(A!) * 100)
A# = 0.0: PRINT INT(TAN(A#) * 100)
A# = 0.0: PRINT INT(ATN(A#) * 100)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    let values: Vec<&str> = lines[0].split_whitespace().collect();
    assert_eq!(values, vec!["0", "0"], "tan/atn literals");
    assert_eq!(lines[1], "0", "tan single");
    assert_eq!(lines[2], "0", "atn single");
    assert_eq!(lines[3], "0", "tan double");
    assert_eq!(lines[4], "0", "atn double");
}

#[test]
fn test_exp_log() {
    // EXP and LOG with various input types
    let output = compile_and_run(
        r#"
PRINT INT(EXP(0)), INT(LOG(1))
A% = 0: PRINT INT(EXP(A%))
A% = 1: PRINT INT(LOG(A%))
A! = 0.0: PRINT INT(EXP(A!))
A! = 1.0: PRINT INT(LOG(A!))
A# = 0.0: PRINT INT(EXP(A#))
A# = 1.0: PRINT INT(LOG(A#))
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    let values: Vec<&str> = lines[0].split_whitespace().collect();
    assert_eq!(values, vec!["1", "0"], "exp/log literals");
    assert_eq!(lines[1], "1", "exp integer");
    assert_eq!(lines[2], "0", "log integer");
    assert_eq!(lines[3], "1", "exp single");
    assert_eq!(lines[4], "0", "log single");
    assert_eq!(lines[5], "1", "exp double");
    assert_eq!(lines[6], "0", "log double");
}

#[test]
fn test_rnd_timer() {
    // RND returns 0-1, TIMER returns seconds since midnight
    let output = compile_and_run(
        r#"
X = RND(1)
IF X >= 0 AND X < 1 THEN PRINT "rnd-ok"
T = TIMER
IF T >= 0 THEN PRINT "timer-ok"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "rnd-ok");
    assert_eq!(lines[1], "timer-ok");
}

#[test]
fn test_type_conversions() {
    // CINT, CLNG, CSNG, CDBL with various inputs
    let output = compile_and_run(
        r#"
A% = 42: PRINT CINT(A%)
A& = 12345: PRINT CINT(A&)
A! = 3.7: PRINT CINT(A!)
A! = 3.7: PRINT CLNG(A!)
A% = 42: B! = CSNG(A%): PRINT B!
A& = 12345: B! = CSNG(A&): PRINT B!
A% = 42: B# = CDBL(A%): PRINT B#
A& = 12345: B# = CDBL(A&): PRINT B#
A! = 3.5: B# = CDBL(A!): PRINT B#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "42", "cint integer");
    assert_eq!(lines[1], "12345", "cint long");
    assert_eq!(lines[2], "4", "cint single");
    assert_eq!(lines[3], "4", "clng single");
    assert_eq!(lines[4], "42", "csng integer");
    assert_eq!(lines[5], "12345", "csng long");
    assert_eq!(lines[6], "42", "cdbl integer");
    assert_eq!(lines[7], "12345", "cdbl long");
    assert_eq!(lines[8], "3.5", "cdbl single");
}
