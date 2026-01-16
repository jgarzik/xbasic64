//! Variable assignment and type suffix tests

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::{compile_and_run, normalize_output};

#[test]
fn test_variable_assignment() {
    let output = compile_and_run(
        r#"
X = 100
Y = 23
PRINT X + Y
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "123");
}

#[test]
fn test_integer_suffix() {
    let output = compile_and_run(
        r#"
X% = 32000
PRINT X%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "32000");
}

#[test]
fn test_long_suffix() {
    let output = compile_and_run(
        r#"
X& = 100000
PRINT X&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "100000");
}

#[test]
fn test_single_suffix() {
    // Test Single (!) type suffix - 32-bit float
    let output = compile_and_run(
        r#"
X! = 3.14159
PRINT X!
"#,
    )
    .unwrap();
    // Single has ~7 significant digits
    assert!(output.contains("3.14159"));
}

#[test]
fn test_single_arithmetic() {
    // Test arithmetic with Single type
    let output = compile_and_run(
        r#"
A! = 2.5
B! = 3.5
PRINT A! + B!
PRINT A! * B!
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "6");
    assert_eq!(lines[1], "8.75");
}

#[test]
fn test_string_variable() {
    let output = compile_and_run(
        r#"
X$ = "Hello"
Y$ = " World"
PRINT X$ + Y$
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "Hello World");
}

#[test]
fn test_rem_comment() {
    let output = compile_and_run(
        r#"
REM This is a comment
PRINT "before"
REM Another comment
PRINT "after"
"#,
    )
    .unwrap();
    assert_eq!(normalize_output(&output), "before\nafter");
}
