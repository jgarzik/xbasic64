//! INPUT statement tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run_with_stdin;

#[test]
fn test_input() {
    // Test INPUT with number and string
    let output = compile_and_run_with_stdin(
        r#"
INPUT X
PRINT X * 2
"#,
        "21\n",
    )
    .unwrap();
    assert!(output.contains("42"), "number input");

    let output2 = compile_and_run_with_stdin(
        r#"
INPUT A$
PRINT "Hello, "; A$
"#,
        "World\n",
    )
    .unwrap();
    assert!(output2.contains("Hello, World"), "string input");
}

#[test]
fn test_line_input() {
    let output = compile_and_run_with_stdin(
        r#"
LINE INPUT A$
PRINT A$
"#,
        "Hello, World!\n",
    )
    .unwrap();
    assert!(output.contains("Hello, World!"));
}

/// INPUT and READ can target an array element. They previously held a bare
/// name, so `INPUT A(3)` and `READ A(I)` were impossible to express.
#[test]
fn test_input_and_read_into_array_elements() {
    let output = compile_and_run_with_stdin(
        r#"
DIM A(5)
DIM S$(2)
DATA 10, "text"
READ A(0), S$(1)
I = 2
INPUT A(I + 1)
PRINT A(0)
PRINT S$(1)
PRINT A(3)
"#,
        "77\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["10", "text", "77"]);
}
