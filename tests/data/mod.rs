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
