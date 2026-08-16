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
    // The promptless INPUT prints `? `, which lands on the first output line.
    assert_eq!(lines, vec!["? 10", "text", "77"]);
}

/// The separator after an INPUT prompt decides whether a question mark follows.
///
/// GW-BASIC prints `p? ` for `INPUT "p"; A` and `p` alone for `INPUT "p", A`,
/// and a promptless `INPUT A` prints a bare `? `. The parser accepted either
/// separator and discarded which, and nothing anywhere emitted a question mark,
/// so all three forms printed the prompt verbatim -- while LANGREF claimed
/// otherwise.
#[test]
fn test_input_prompt_separators() {
    let semi = compile_and_run_with_stdin("INPUT \"Name\"; A$\nPRINT A$\n", "Bob\n").unwrap();
    assert_eq!(
        semi.trim_end(),
        "Name? Bob",
        "a semicolon adds the question mark"
    );

    let comma = compile_and_run_with_stdin("INPUT \"Name\", A$\nPRINT A$\n", "Bob\n").unwrap();
    assert_eq!(comma.trim_end(), "NameBob", "a comma suppresses it");

    let bare = compile_and_run_with_stdin("INPUT A$\nPRINT A$\n", "Bob\n").unwrap();
    assert_eq!(bare.trim_end(), "? Bob", "no prompt still asks");
}

/// LINE INPUT never adds a question mark, whichever separator is used.
///
/// These compare `trim_end()` rather than the raw output: the prompt shares a
/// line with the echoed input, and the line ending that follows is LF on
/// System V and CRLF on Windows. Asserting the raw string passed on Linux and
/// failed the Windows job.
#[test]
fn test_line_input_never_adds_a_question_mark() {
    let semi = compile_and_run_with_stdin("LINE INPUT \"N: \"; A$\nPRINT A$\n", "Bob\n").unwrap();
    assert_eq!(semi.trim_end(), "N: Bob");

    let bare = compile_and_run_with_stdin("LINE INPUT A$\nPRINT A$\n", "Bob\n").unwrap();
    assert_eq!(
        bare.trim_end(),
        "Bob",
        "and prompts for nothing when none is given"
    );
}

/// A leading `;` is accepted on both statements.
///
/// It suppresses the newline echoed when the operator presses Return, which
/// here is the terminal's echo rather than anything the program prints -- so it
/// parses and has no effect. Refusing it would turn away a program for asking
/// about a difference this implementation cannot observe.
#[test]
fn test_input_leading_semicolon_is_accepted() {
    let out = compile_and_run_with_stdin("INPUT ; \"Name\"; A$\nPRINT A$\n", "Bob\n").unwrap();
    assert_eq!(out.trim_end(), "Name? Bob");

    let line = compile_and_run_with_stdin("LINE INPUT ; \"N: \"; A$\nPRINT A$\n", "Bob\n").unwrap();
    assert_eq!(line.trim_end(), "N: Bob");
}
