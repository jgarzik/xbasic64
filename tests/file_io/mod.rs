//! File I/O tests

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run_with_files;
use std::fs;

#[test]
fn test_file_write() {
    let source = r#"
OPEN "output.txt" FOR OUTPUT AS #1
PRINT #1, "Hello, File!"
PRINT #1, 42
CLOSE #1
PRINT "done"
"#;

    let (output, tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(output.trim(), "done");

    // Verify file contents
    let file_contents = fs::read_to_string(tmp.path().join("output.txt")).unwrap();
    let lines: Vec<&str> = file_contents.lines().collect();
    assert_eq!(lines, vec!["Hello, File!", "42"]);
}

#[test]
fn test_file_read() {
    let source = r#"
OPEN "input.txt" FOR INPUT AS #1
INPUT #1, X
INPUT #1, Y
CLOSE #1
PRINT X + Y
"#;

    let (output, _tmp) = compile_and_run_with_files(source, |path| {
        fs::write(path.join("input.txt"), "10\n20\n").map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(output.trim(), "30");
}

#[test]
fn test_file_append() {
    let source = r#"
OPEN "data.txt" FOR APPEND AS #2
PRINT #2, "Line 3"
CLOSE #2
PRINT "appended"
"#;

    let (output, tmp) = compile_and_run_with_files(source, |path| {
        fs::write(path.join("data.txt"), "Line 1\nLine 2\n").map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(output.trim(), "appended");

    // Verify file contents
    let file_contents = fs::read_to_string(tmp.path().join("data.txt")).unwrap();
    let lines: Vec<&str> = file_contents.lines().collect();
    assert_eq!(lines, vec!["Line 1", "Line 2", "Line 3"]);
}
