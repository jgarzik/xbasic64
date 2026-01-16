//! Print statement tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_print_combined() {
    // Test print with strings, numbers, and multiple statements
    let output = compile_and_run(
        r#"
PRINT "Hello, World!"
PRINT 42
PRINT "A"
PRINT "B"
PRINT "C"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "Hello, World!", "string");
    assert_eq!(lines[1], "42", "number");
    assert_eq!(lines[2], "A", "multi-a");
    assert_eq!(lines[3], "B", "multi-b");
    assert_eq!(lines[4], "C", "multi-c");
}
