//! Variable assignment and type suffix tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::{compile_and_run, normalize_output};

#[test]
fn test_variable_types() {
    // Test variable assignment and type suffixes (%, &, !, #)
    let output = compile_and_run(
        r#"
X = 100: Y = 23: PRINT X + Y
X% = 32000: PRINT X%
X& = 100000: PRINT X&
X! = 3.14159: PRINT X!
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "123", "default vars");
    assert_eq!(lines[1], "32000", "integer suffix");
    assert_eq!(lines[2], "100000", "long suffix");
    assert!(lines[3].contains("3.14159"), "single suffix");
}

#[test]
fn test_variable_misc() {
    // Test single arithmetic, string variables, and comments
    let output = compile_and_run(
        r#"
A! = 2.5: B! = 3.5: PRINT A! + B!
A! = 2.5: B! = 3.5: PRINT A! * B!
X$ = "Hello": Y$ = " World": PRINT X$ + Y$
REM This is a comment
PRINT "before"
REM Another comment
PRINT "after"
"#,
    )
    .unwrap();
    let normalized = normalize_output(&output);
    let lines: Vec<&str> = normalized.lines().collect();
    assert_eq!(lines[0], "6", "single add");
    assert_eq!(lines[1], "8.75", "single mul");
    assert_eq!(lines[2], "Hello World", "string concat");
    assert_eq!(lines[3], "before", "before comment");
    assert_eq!(lines[4], "after", "after comment");
}
