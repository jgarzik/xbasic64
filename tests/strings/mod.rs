//! String function tests

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_len_function() {
    let output = compile_and_run(r#"PRINT LEN("Hello")"#).unwrap();
    assert_eq!(output.trim(), "5");
}

#[test]
fn test_left_right() {
    let output = compile_and_run(
        r#"
PRINT LEFT$("Hello", 2)
PRINT RIGHT$("Hello", 2)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["He", "lo"]);
}

#[test]
fn test_mid_function() {
    let output = compile_and_run(r#"PRINT MID$("Hello", 2, 3)"#).unwrap();
    assert_eq!(output.trim(), "ell");
}

#[test]
fn test_chr_asc() {
    let output = compile_and_run(
        r#"
PRINT CHR$(65)
PRINT ASC("A")
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["A", "65"]);
}

#[test]
fn test_val_str() {
    let output = compile_and_run(
        r#"
X = VAL("42")
PRINT X + 8
PRINT STR$(100)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["50", "100"]);
}

#[test]
fn test_instr_function() {
    let output = compile_and_run(r#"PRINT INSTR("Hello World", "World")"#).unwrap();
    assert_eq!(output.trim(), "7");
}

#[test]
fn test_left_with_nested_call() {
    // Test LEFT$ with nested function call in count argument
    let output = compile_and_run(
        r#"
A$ = "HELLO"
B$ = "WORLD"
PRINT LEFT$(A$ + B$, LEN(A$))
"#,
    )
    .unwrap();
    // A$+B$ = "HELLOWORLD", LEN(A$) = 5, LEFT$ takes first 5 chars
    assert_eq!(output.trim(), "HELLO");
}

#[test]
fn test_right_with_nested_call() {
    // Test RIGHT$ with nested function call in count argument
    let output = compile_and_run(
        r#"
A$ = "HELLO"
B$ = "WORLD"
PRINT RIGHT$(A$ + B$, LEN(B$))
"#,
    )
    .unwrap();
    // A$+B$ = "HELLOWORLD", LEN(B$) = 5, RIGHT$ takes last 5 chars
    assert_eq!(output.trim(), "WORLD");
}

#[test]
fn test_mid_with_nested_calls() {
    // Test MID$ with nested function calls in position and count arguments
    let output = compile_and_run(
        r#"
FUNCTION GetStart()
    GetStart = 2
END FUNCTION

FUNCTION GetLen()
    GetLen = 3
END FUNCTION

PRINT MID$("ABCDEF", GetStart(), GetLen())
"#,
    )
    .unwrap();
    // MID$("ABCDEF", 2, 3) = "BCD"
    assert_eq!(output.trim(), "BCD");
}

#[test]
fn test_string_concat_multiple() {
    // Test string concatenation with multiple operands
    let output = compile_and_run(
        r#"
A$ = "Hello"
B$ = " "
C$ = "World"
PRINT A$ + B$ + C$
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "Hello World");
}
