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
