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

/// An unassigned variable reads as 0 (or the empty string), not whatever was
/// left in memory. Module-level variables get this from .bss; the check is
/// repeated because the old behavior was stack garbage that was intermittently
/// zero by luck.
#[test]
fn test_uninitialized_variables_are_zero() {
    for _ in 0..20 {
        let output = compile_and_run(
            r#"
PRINT X
PRINT X%
PRINT X&
PRINT X!
PRINT "["; X$; "]"
"#,
        )
        .unwrap();
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines, vec!["0", "0", "0", "0", "[]"], "unassigned defaults");
    }
}

/// Reading an unassigned variable in an expression must behave as 0, and a
/// running total must not pick up stack garbage.
#[test]
fn test_uninitialized_in_expressions() {
    let output = compile_and_run(
        r#"
Y = X + 1
PRINT Y
FOR I = 1 TO 3
S = S + I
NEXT I
PRINT S
PRINT LEN(Z$)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1", "6", "0"]);
}

/// String variables must not alias one another. The length used to be
/// scavenged from the neighbouring slot rather than reserved with the pointer.
#[test]
fn test_string_variables_do_not_alias() {
    let output = compile_and_run(
        r#"
A$ = "first"
B$ = "second"
C$ = "third"
PRINT A$; "/"; B$; "/"; C$
PRINT LEN(A$); LEN(B$); LEN(C$)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "first/second/third");
    // LEN of "first", "second", "third", printed adjacently by `;`
    assert_eq!(lines[1], "565");
}

/// A variable may be named after a keyword when it carries a type suffix:
/// keywords have no type, so `Line$` is a string variable. Stripping the
/// suffix before the keyword lookup made LANGREF's own `LINE INPUT #1, Line$`
/// example fail to parse.
#[test]
fn test_keyword_named_variables() {
    let output = compile_and_run(
        "Line$ = \"a\"\nPrint$ = \"b\"\nData$ = \"c\"\nEnd$ = \"d\"\nNext$ = \"e\"\nPRINT Line$; Print$; Data$; End$; Next$\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "abcde");
}

/// A float literal may start with the decimal point, as LANGREF documents.
#[test]
fn test_leading_dot_literals() {
    let output = compile_and_run("PRINT .5\nPRINT .25 + .25\nPRINT .5E1\n").unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["0.5", "0.5", "5"]);
}

/// LET accepts every assignment form the bare syntax does.
///
/// The LET path was a cut-down parser that knew only `name` and `name(i)`, so
/// `LET Q.X = 3` and `LET MID$(S$,1,2) = "HE"` were both rejected.
#[test]
fn test_let_accepts_every_assignment_form() {
    let output = compile_and_run(
        "TYPE P\nX AS INTEGER\nEND TYPE\nDIM Q AS P\nDIM A(3)\nDIM R(3) AS P\nLET X = 5\nLET A(1) = 7\nLET Q.X = 3\nLET R(1).X = 4\nS$ = \"xxllo\"\nLET MID$(S$,1,2) = \"HE\"\nPRINT X; A(1); Q.X; R(1).X\nPRINT S$\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["5734", "HEllo"]);
}
