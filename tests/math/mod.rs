//! Math function tests

use crate::common::compile_and_run;

#[test]
fn test_sqr_function() {
    let output = compile_and_run("PRINT SQR(16)").unwrap();
    assert_eq!(output.trim(), "4");
}

#[test]
fn test_abs_function() {
    let output = compile_and_run("PRINT ABS(-42)").unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_int_function() {
    let output = compile_and_run("PRINT INT(3.7)").unwrap();
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_fix_function() {
    // FIX truncates toward zero, INT floors
    let output = compile_and_run("PRINT FIX(-3.7)").unwrap();
    assert_eq!(output.trim(), "-3");
}

#[test]
fn test_sgn_function() {
    let output = compile_and_run(
        r#"
PRINT SGN(-5)
PRINT SGN(0)
PRINT SGN(5)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["-1", "0", "1"]);
}

#[test]
fn test_sin_cos() {
    let output = compile_and_run("PRINT INT(SIN(0) * 100), INT(COS(0) * 100)").unwrap();
    // sin(0) = 0, cos(0) = 1
    let values: Vec<&str> = output.split_whitespace().collect();
    assert_eq!(values, vec!["0", "100"]);
}

#[test]
fn test_tan_atn() {
    let output = compile_and_run("PRINT INT(TAN(0) * 100), INT(ATN(0) * 100)").unwrap();
    // tan(0) = 0, atn(0) = 0
    let values: Vec<&str> = output.split_whitespace().collect();
    assert_eq!(values, vec!["0", "0"]);
}

#[test]
fn test_exp_log() {
    let output = compile_and_run("PRINT INT(EXP(0)), INT(LOG(1))").unwrap();
    // exp(0) = 1, log(1) = 0
    let values: Vec<&str> = output.split_whitespace().collect();
    assert_eq!(values, vec!["1", "0"]);
}

#[test]
fn test_rnd_function() {
    // RND returns a value between 0 and 1
    let output = compile_and_run(
        r#"
X = RND(1)
IF X >= 0 AND X < 1 THEN PRINT "ok"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "ok");
}

#[test]
fn test_timer_function() {
    // TIMER returns seconds since midnight; just verify it returns a number
    let output = compile_and_run(
        r#"
T = TIMER
IF T >= 0 THEN PRINT "ok"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "ok");
}

// ============================================
// Math functions with Integer (%) input
// ============================================

#[test]
fn test_sqr_integer_input() {
    let output = compile_and_run(
        r#"
A% = 25
PRINT SQR(A%)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "5");
}

#[test]
fn test_abs_integer_input() {
    let output = compile_and_run(
        r#"
A% = -42
PRINT ABS(A%)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_sgn_integer_input() {
    let output = compile_and_run(
        r#"
A% = -5
B% = 0
C% = 5
PRINT SGN(A%)
PRINT SGN(B%)
PRINT SGN(C%)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["-1", "0", "1"]);
}

#[test]
fn test_sin_integer_input() {
    let output = compile_and_run(
        r#"
A% = 0
PRINT INT(SIN(A%) * 100)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_cos_integer_input() {
    let output = compile_and_run(
        r#"
A% = 0
PRINT INT(COS(A%) * 100)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "100");
}

#[test]
fn test_exp_integer_input() {
    let output = compile_and_run(
        r#"
A% = 0
PRINT INT(EXP(A%))
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "1");
}

#[test]
fn test_log_integer_input() {
    let output = compile_and_run(
        r#"
A% = 1
PRINT INT(LOG(A%))
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "0");
}

// ============================================
// Math functions with Long (&) input
// ============================================

#[test]
fn test_sqr_long_input() {
    let output = compile_and_run(
        r#"
A& = 10000
PRINT SQR(A&)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "100");
}

#[test]
fn test_abs_long_input() {
    let output = compile_and_run(
        r#"
A& = -100000
PRINT ABS(A&)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "100000");
}

#[test]
fn test_sgn_long_input() {
    let output = compile_and_run(
        r#"
A& = -50000
B& = 0
C& = 50000
PRINT SGN(A&)
PRINT SGN(B&)
PRINT SGN(C&)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["-1", "0", "1"]);
}

#[test]
fn test_sin_long_input() {
    let output = compile_and_run(
        r#"
A& = 0
PRINT INT(SIN(A&) * 100)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_cos_long_input() {
    let output = compile_and_run(
        r#"
A& = 0
PRINT INT(COS(A&) * 100)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "100");
}

// ============================================
// Math functions with Single (!) input
// ============================================

#[test]
fn test_sqr_single_input() {
    let output = compile_and_run(
        r#"
A! = 2.25
PRINT SQR(A!)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "1.5");
}

#[test]
fn test_abs_single_input() {
    let output = compile_and_run(
        r#"
A! = -3.14
PRINT ABS(A!)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3.14");
}

#[test]
fn test_int_single_input() {
    let output = compile_and_run(
        r#"
A! = 3.7
PRINT INT(A!)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_fix_single_input() {
    let output = compile_and_run(
        r#"
A! = -3.7
PRINT FIX(A!)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "-3");
}

#[test]
fn test_sgn_single_input() {
    let output = compile_and_run(
        r#"
A! = -2.5
B! = 0.0
C! = 2.5
PRINT SGN(A!)
PRINT SGN(B!)
PRINT SGN(C!)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["-1", "0", "1"]);
}

#[test]
fn test_sin_single_input() {
    let output = compile_and_run(
        r#"
A! = 0.0
PRINT INT(SIN(A!) * 100)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_cos_single_input() {
    let output = compile_and_run(
        r#"
A! = 0.0
PRINT INT(COS(A!) * 100)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "100");
}

#[test]
fn test_tan_single_input() {
    let output = compile_and_run(
        r#"
A! = 0.0
PRINT INT(TAN(A!) * 100)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_atn_single_input() {
    let output = compile_and_run(
        r#"
A! = 0.0
PRINT INT(ATN(A!) * 100)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_exp_single_input() {
    let output = compile_and_run(
        r#"
A! = 0.0
PRINT INT(EXP(A!))
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "1");
}

#[test]
fn test_log_single_input() {
    let output = compile_and_run(
        r#"
A! = 1.0
PRINT INT(LOG(A!))
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "0");
}

// ============================================
// Math functions with Double (#) input
// ============================================

#[test]
fn test_sqr_double_input() {
    let output = compile_and_run(
        r#"
A# = 2.25
PRINT SQR(A#)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "1.5");
}

#[test]
fn test_abs_double_input() {
    let output = compile_and_run(
        r#"
A# = -3.14159
PRINT ABS(A#)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3.14159");
}

#[test]
fn test_int_double_input() {
    let output = compile_and_run(
        r#"
A# = 3.7
PRINT INT(A#)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_fix_double_input() {
    let output = compile_and_run(
        r#"
A# = -3.7
PRINT FIX(A#)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "-3");
}

#[test]
fn test_sgn_double_input() {
    let output = compile_and_run(
        r#"
A# = -2.5
B# = 0.0
C# = 2.5
PRINT SGN(A#)
PRINT SGN(B#)
PRINT SGN(C#)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["-1", "0", "1"]);
}

#[test]
fn test_sin_double_input() {
    let output = compile_and_run(
        r#"
A# = 0.0
PRINT INT(SIN(A#) * 100)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_cos_double_input() {
    let output = compile_and_run(
        r#"
A# = 0.0
PRINT INT(COS(A#) * 100)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "100");
}

#[test]
fn test_tan_double_input() {
    let output = compile_and_run(
        r#"
A# = 0.0
PRINT INT(TAN(A#) * 100)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_atn_double_input() {
    let output = compile_and_run(
        r#"
A# = 0.0
PRINT INT(ATN(A#) * 100)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_exp_double_input() {
    let output = compile_and_run(
        r#"
A# = 0.0
PRINT INT(EXP(A#))
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "1");
}

#[test]
fn test_log_double_input() {
    let output = compile_and_run(
        r#"
A# = 1.0
PRINT INT(LOG(A#))
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "0");
}

// ============================================
// Type conversion functions with various inputs
// ============================================

#[test]
fn test_cint_integer_input() {
    let output = compile_and_run(
        r#"
A% = 42
PRINT CINT(A%)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_cint_long_input() {
    let output = compile_and_run(
        r#"
A& = 12345
PRINT CINT(A&)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "12345");
}

#[test]
fn test_cint_single_input() {
    let output = compile_and_run(
        r#"
A! = 3.7
PRINT CINT(A!)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "4");
}

#[test]
fn test_clng_single_input() {
    let output = compile_and_run(
        r#"
A! = 3.7
PRINT CLNG(A!)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "4");
}

#[test]
fn test_csng_integer_input() {
    let output = compile_and_run(
        r#"
A% = 42
B! = CSNG(A%)
PRINT B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_csng_long_input() {
    let output = compile_and_run(
        r#"
A& = 12345
B! = CSNG(A&)
PRINT B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "12345");
}

#[test]
fn test_cdbl_integer_input() {
    let output = compile_and_run(
        r#"
A% = 42
B# = CDBL(A%)
PRINT B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_cdbl_long_input() {
    let output = compile_and_run(
        r#"
A& = 12345
B# = CDBL(A&)
PRINT B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "12345");
}

#[test]
fn test_cdbl_single_input() {
    let output = compile_and_run(
        r#"
A! = 3.5
B# = CDBL(A!)
PRINT B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3.5");
}
