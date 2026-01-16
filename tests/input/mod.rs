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
