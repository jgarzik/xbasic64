//! Tests for programs the compiler must reject, and for runtime abort behavior.
//!
//! These use the negative-testing helpers in `common`: `compile_only` for
//! "must be rejected", and `compile_and_run_raw` for "produced this output,
//! then aborted with this exit code".

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::{compile_and_run_raw, compile_only};

/// The harness itself must be able to tell a rejected program from an accepted one.
#[test]
fn test_harness_detects_rejection() {
    // A syntactically valid program compiles.
    compile_only("PRINT 1\n").expect("valid program should compile");

    // A malformed one does not, and the diagnostic is available for assertions.
    let err = compile_only("IF THEN\n").expect_err("malformed program should be rejected");
    assert!(
        err.contains("error") || err.contains("Error"),
        "expected a diagnostic, got stdout={:?} stderr={:?}",
        err.stdout,
        err.stderr
    );
    assert!(
        err.is_clean_rejection(),
        "expected a clean rejection (exit 1), not a panic; exit={:?} stderr={}",
        err.exit_code,
        err.stderr
    );
}

/// The harness must expose stdout, stderr, and the exit code independently.
#[test]
fn test_harness_captures_exit_code_and_streams() {
    let run = compile_and_run_raw("PRINT \"before\"\n", "").expect("should compile");
    assert_eq!(run.lines(), vec!["before"], "stdout captured");
    assert_eq!(run.exit_code, Some(0), "normal termination is exit 0");
    assert!(
        run.stderr.is_empty(),
        "a successful program writes nothing to stderr, got {:?}",
        run.stderr
    );
}

/// A program reading stdin must see a clean EOF when no input is supplied,
/// rather than blocking or inheriting the terminal.
#[test]
fn test_harness_supplies_empty_stdin() {
    let run = compile_and_run_raw("INPUT N\nPRINT \"read\"\n", "").expect("should compile");
    assert_eq!(
        run.lines(),
        vec!["read"],
        "program ran to completion at EOF"
    );
    assert_eq!(run.exit_code, Some(0));
}

/// GOSUB inside SELECT CASE must link: `preprocess` has to see the nested body
/// to know the GOSUB return stack is needed. Regression test for the
/// `_gosub_sp` undefined-reference bug.
#[test]
fn test_gosub_inside_select_case() {
    let run = compile_and_run_raw(
        r#"
X = 1
SELECT CASE X
CASE 1
GOSUB 100
END SELECT
END
100 PRINT "in subroutine"
RETURN
"#,
        "",
    )
    .expect("GOSUB inside SELECT CASE should compile and link");
    assert_eq!(run.lines(), vec!["in subroutine"], "gosub body ran");
    assert_eq!(run.exit_code, Some(0));
}

/// DATA inside SELECT CASE must be collected into the DATA table, exactly as it
/// is when nested inside IF.
#[test]
fn test_data_inside_select_case() {
    let run = compile_and_run_raw(
        r#"
X = 1
SELECT CASE X
CASE 1
DATA 42
END SELECT
READ A
PRINT A
"#,
        "",
    )
    .expect("DATA inside SELECT CASE should compile");
    assert_eq!(
        run.lines(),
        vec!["42"],
        "DATA in SELECT CASE must be collected"
    );
}

/// GOSUB and DATA nested inside a SELECT CASE that is itself nested inside
/// another block: the statement walker must recurse all the way down.
#[test]
fn test_nested_select_case_collection() {
    let run = compile_and_run_raw(
        r#"
X = 1
IF X = 1 THEN
SELECT CASE X
CASE 1
GOSUB 200
DATA 7
END SELECT
END IF
READ V
PRINT V
END
200 PRINT "nested"
RETURN
"#,
        "",
    )
    .expect("nested SELECT CASE contents should be collected");
    assert_eq!(run.lines(), vec!["nested", "7"]);
}
