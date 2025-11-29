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
