//! String function tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_string_functions() {
    // Test LEN, LEFT$, RIGHT$, MID$, CHR$, ASC, VAL, STR$, INSTR
    let output = compile_and_run(
        r#"
PRINT LEN("Hello")
PRINT LEFT$("Hello", 2)
PRINT RIGHT$("Hello", 2)
PRINT MID$("Hello", 2, 3)
PRINT CHR$(65)
PRINT ASC("A")
X = VAL("42"): PRINT X + 8
PRINT STR$(100)
PRINT INSTR("Hello World", "World")
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "5", "len");
    assert_eq!(lines[1], "He", "left$");
    assert_eq!(lines[2], "lo", "right$");
    assert_eq!(lines[3], "ell", "mid$");
    assert_eq!(lines[4], "A", "chr$");
    assert_eq!(lines[5], "65", "asc");
    assert_eq!(lines[6], "50", "val");
    assert_eq!(lines[7], "100", "str$");
    assert_eq!(lines[8], "7", "instr");
}

#[test]
fn test_nested_string_calls() {
    // Test LEFT$, RIGHT$, MID$ with nested function calls
    let output = compile_and_run(
        r#"
FUNCTION GetStart()
    GetStart = 2
END FUNCTION

FUNCTION GetLen()
    GetLen = 3
END FUNCTION

A$ = "HELLO"
B$ = "WORLD"
PRINT LEFT$(A$ + B$, LEN(A$))
PRINT RIGHT$(A$ + B$, LEN(B$))
PRINT MID$("ABCDEF", GetStart(), GetLen())
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "HELLO", "left$ with len()");
    assert_eq!(lines[1], "WORLD", "right$ with len()");
    assert_eq!(lines[2], "BCD", "mid$ with functions");
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
