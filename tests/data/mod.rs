//! DATA/READ/RESTORE tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_data_read_restore() {
    // Test DATA/READ and RESTORE
    let output = compile_and_run(
        r#"
DATA 10, 20, 30
READ A
READ B
READ C
PRINT A + B + C
DATA 5, 10
RESTORE
READ D
PRINT D
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "60", "data read sum");
    assert_eq!(lines[1], "10", "restore reads first data");
}

/// The DATA/READ example from LANGREF. Any string in a DATA statement used to
/// fail to link: the `_str_N` labels were emitted before DATA strings were
/// interned, so the trailing ones were referenced by the table but never
/// defined.
#[test]
fn test_string_data() {
    let output = compile_and_run(
        r#"
DATA 10, 20, 30, "Hello", "World"
READ A, B, C
READ X$, Y$
PRINT A; B; C
PRINT X$; " "; Y$
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "102030");
    assert_eq!(lines[1], "Hello World");
}

/// Adjacent string literals must not bleed into one another. DATA strings are
/// measured with strlen, so they need a terminator in the emitted assembly.
#[test]
fn test_string_data_lengths() {
    let output = compile_and_run(
        r#"
DATA "alpha", "beta", "gamma", ""
READ P$, Q$, R$, S$
PRINT P$; "/"; Q$; "/"; R$; "/"; S$
PRINT LEN(P$); LEN(Q$); LEN(R$); LEN(S$)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "alpha/beta/gamma/");
    assert_eq!(lines[1], "5450", "lengths of alpha, beta, gamma, empty");
}

/// RESTORE must work with string DATA too.
#[test]
fn test_string_data_restore() {
    let output = compile_and_run(
        r#"
DATA "one", "two"
READ A$
RESTORE
READ B$
PRINT A$; "/"; B$
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "one/one");
}

/// Mixed numeric and string DATA in one table.
#[test]
fn test_mixed_data_types() {
    let output = compile_and_run(
        r#"
DATA 1, "two", 3.5, "four"
READ A, B$, C, D$
PRINT A
PRINT B$
PRINT C
PRINT D$
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1", "two", "3.5", "four"]);
}

/// RESTORE with a line number resumes at that line's DATA. It used to accept
/// the argument and silently restart from the beginning.
#[test]
fn test_restore_to_line() {
    let output = compile_and_run(
        "100 DATA 1, 2\n110 DATA 3, 4\n120 READ A\n130 READ B\n140 RESTORE 110\n150 READ C\n160 PRINT A; B; C\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "123");
}

/// Bare RESTORE still restarts from the first DATA item.
#[test]
fn test_restore_to_start() {
    let output = compile_and_run(
        "100 DATA 1, 2\n110 DATA 3, 4\n120 READ A\n130 READ B\n140 RESTORE\n150 READ C\n160 PRINT A; B; C\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "121");
}

/// RESTORE also accepts a named label.
#[test]
fn test_restore_to_label() {
    let output = compile_and_run(
        "DATA 1, 2\nLater:\nDATA 3, 4\nREAD A\nRESTORE Later\nREAD B\nPRINT A; B\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "13");
}

/// DATA items need quotes only when they contain a comma, a colon, or
/// significant surrounding spaces -- GW-BASIC's rule.
///
/// The parser only accepted Integer, Float, String and a leading minus, so an
/// unquoted word ended the item list silently and the word itself was then
/// parsed as a fresh statement, producing an error about the *word* rather than
/// about DATA. Reassembling items from tokens would not have worked either: the
/// lexer uppercases identifiers, so `DATA hello` would have yielded "HELLO".
#[test]
fn test_unquoted_data_items() {
    let output = compile_and_run(
        r#"
DATA hello, World, MiXeD
READ A$, B$, C$
PRINT A$
PRINT B$
PRINT C$
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["hello", "World", "MiXeD"], "case is preserved");
}

/// Surrounding spaces are trimmed from an unquoted item and kept in a quoted
/// one, and a quoted item may contain the separators.
#[test]
fn test_data_quoting_rules() {
    let output = compile_and_run(
        r#"
DATA   spaced   , "  kept  ", "a,b", "c:d"
READ A$, B$, C$, D$
PRINT "["; A$; "]"
PRINT "["; B$; "]"
PRINT C$
PRINT D$
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        vec!["[spaced]", "[  kept  ]", "a,b", "c:d"],
        "unquoted items trim, quoted items do not"
    );
}

/// An omitted item reads as zero, or as the empty string.
#[test]
fn test_data_empty_items() {
    let output = compile_and_run(
        r#"
DATA 1,,3
READ A, B, C
PRINT A
PRINT B
PRINT C
DATA ,x
READ D$, E$
PRINT "["; D$; "]["; E$; "]"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1", "0", "3", "[][x]"]);
}

/// A colon still ends the DATA statement, so a statement may follow it on the
/// same line -- as it could before.
#[test]
fn test_data_ends_at_a_colon() {
    let output = compile_and_run(
        r#"
DATA 1, 2 : PRINT "after"
READ A, B
PRINT A + B
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["after", "3"]);
}
