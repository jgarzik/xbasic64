//! Arithmetic and operator tests

use crate::common::compile_and_run;

#[test]
fn test_arithmetic_add() {
    let output = compile_and_run("PRINT 10 + 5").unwrap();
    assert_eq!(output.trim(), "15");
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
