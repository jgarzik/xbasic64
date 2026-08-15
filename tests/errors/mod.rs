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

/// Helper: a program must be rejected with a diagnostic containing `expected`,
/// and must be rejected cleanly rather than by panicking.
fn expect_rejected(source: &str, expected: &str) {
    let e = compile_only(source).expect_err(&format!(
        "expected rejection of:\n{source}\nbut it compiled"
    ));
    assert!(
        e.contains(expected),
        "expected {expected:?} in diagnostics for:\n{source}\ngot stderr:\n{}",
        e.stderr
    );
    assert!(
        e.is_clean_rejection(),
        "should be diagnosed, not panic; exit={:?} stderr:\n{}",
        e.exit_code,
        e.stderr
    );
}

/// Unknown names used to reach the linker as `_proc_<NAME>`, producing
/// `ld: undefined reference to _proc_PRIN`. They are now diagnosed by name.
#[test]
fn test_unknown_names_are_diagnosed() {
    expect_rejected("PRIN \"hello\"\n", "unknown subroutine 'PRIN'");
    expect_rejected("PRIN \"hello\"\n", "did you mean 'PRINT'?");
    expect_rejected("NoSuchSub(1)\n", "unknown subroutine 'NOSUCHSUB'");
    expect_rejected(
        "PRINT NOSUCHFN(1)\n",
        "unknown function or array 'NOSUCHFN'",
    );
    expect_rejected("PRINT SQRT(4)\n", "did you mean 'SQR'?");
}

/// Using an array without DIM used to abort the compiler with a Rust panic
/// ("Array not declared"), as did a subscript-count mismatch.
#[test]
fn test_undeclared_array_is_diagnosed() {
    expect_rejected("A(0) = 1\n", "array 'A' is used but never declared");
    expect_rejected(
        "DIM M(3,3)\nPRINT M(1)\n",
        "array 'M' has 2 dimensions, but 1 subscript was given",
    );
}

/// Argument counts are checked for both user procedures and builtins.
#[test]
fn test_argument_count_is_checked() {
    expect_rejected(
        "SUB T(A, B)\nPRINT A\nEND SUB\nT(1)\n",
        "'T' expects 2 arguments, but 1 was given",
    );
    expect_rejected(
        "SUB T(A)\nPRINT A\nEND SUB\nT(1, 2)\n",
        "'T' expects 1 argument, but 2 were given",
    );
    expect_rejected(
        "PRINT LEFT$(\"abc\")\n",
        "'LEFT$' expects 2 arguments, but 1 was given",
    );
}

/// Mixing strings and numbers used to panic in gen_coercion.
#[test]
fn test_type_mismatches_are_diagnosed() {
    expect_rejected("A = \"hello\"\n", "cannot assign string value to numeric");
    expect_rejected("A$ = 42\n", "cannot assign numeric value to string");
    expect_rejected(
        "PRINT 1 + \"a\"\n",
        "cannot mix string and numeric operands",
    );
    expect_rejected(
        "PRINT \"a\" * \"b\"\n",
        "operator * cannot be applied to strings",
    );
}

/// A SUB has no value, and a FUNCTION's result cannot be silently discarded.
#[test]
fn test_procedure_kind_is_checked() {
    expect_rejected(
        "FUNCTION F(N)\nF = N\nEND FUNCTION\nF(1)\n",
        "is a FUNCTION",
    );
    expect_rejected(
        "SUB S(N)\nPRINT N\nEND SUB\nPRINT S(1)\n",
        "is a SUB and has no value",
    );
}

/// Branch targets must exist. Previously an undefined one became a link error.
#[test]
fn test_branch_targets_are_checked() {
    expect_rejected(
        "GOTO Nowhere\nPRINT \"x\"\n",
        "target 'NOWHERE' is not defined",
    );
    expect_rejected("GOTO 999\nPRINT \"x\"\n", "target line 999 does not exist");
    expect_rejected(
        "GOTO Finsh\nFinish:\nPRINT \"done\"\n",
        "did you mean 'FINISH'?",
    );
    expect_rejected("Foo:\nFoo:\nPRINT \"x\"\n", "duplicate label 'FOO'");
}

/// Diagnostics carry the source line of the offending statement.
#[test]
fn test_diagnostics_report_source_line() {
    let e = compile_only("PRINT 1\nPRINT 2\nPRIN 3\n").expect_err("should be rejected");
    assert!(
        e.contains(":3: error:"),
        "expected the error on line 3, got:\n{}",
        e.stderr
    );
}
