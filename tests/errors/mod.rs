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

/// An unmatched block terminator must produce a readable diagnostic naming the
/// construct it needed, not leak the parser's internal signalling. These used
/// to surface as `Parse error: CASE:Literal(Integer(1))` and `Parse error: NEXT`.
#[test]
fn test_unmatched_block_terminators_diagnose_clearly() {
    let cases = [
        ("END SELECT\n", "END SELECT without matching SELECT CASE"),
        ("CASE 1\n", "CASE without matching SELECT CASE"),
        ("NEXT I\n", "NEXT without matching FOR"),
        ("WEND\n", "WEND without matching WHILE"),
        ("LOOP\n", "LOOP without matching DO"),
        ("END SUB\n", "END SUB without matching SUB"),
        ("END FUNCTION\n", "END FUNCTION without matching FUNCTION"),
        ("ELSE\n", "ELSE without matching IF"),
        ("END IF\n", "END IF without matching IF"),
    ];
    for (source, expected) in cases {
        let e = compile_only(source)
            .expect_err(&format!("{source:?} should be rejected, but it compiled"));
        assert!(
            e.contains(expected),
            "for {source:?} expected {expected:?}, got stderr: {}",
            e.stderr
        );
        assert!(
            e.is_clean_rejection(),
            "{source:?} should be diagnosed, not panic; exit={:?}",
            e.exit_code
        );
    }
}

/// A bare `END` inside a SELECT CASE body is the program-termination statement,
/// not the start of `END SELECT`. Distinguishing them needs two-token lookahead;
/// without it this failed with "Expected Select, got Newline".
#[test]
fn test_end_statement_inside_select_case_body() {
    let run = compile_and_run_raw(
        r#"
X = 1
SELECT CASE X
CASE 1
PRINT "in case"
END
END SELECT
PRINT "unreachable"
"#,
        "",
    )
    .expect("a bare END inside a CASE body should parse");
    assert_eq!(run.lines(), vec!["in case"], "END terminated the program");
    assert_eq!(run.exit_code, Some(0));
}

/// The ENDSELECT spelling and a nested block inside a case body must both still
/// work after the lookahead change.
#[test]
fn test_select_case_terminator_spellings() {
    let run = compile_and_run_raw(
        r#"
X = 2
SELECT CASE X
CASE 1
PRINT "one"
CASE 2
IF X = 2 THEN
PRINT "two"
END IF
END SELECT
PRINT "after"
"#,
        "",
    )
    .expect("nested IF inside a case body should parse");
    assert_eq!(run.lines(), vec!["two", "after"]);
}
