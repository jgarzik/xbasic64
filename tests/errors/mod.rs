//! Tests for programs the compiler must reject, and for runtime abort behavior.
//!
//! These use the negative-testing helpers in `common`: `compile_only` for
//! "must be rejected", and `compile_and_run_raw` for "produced this output,
//! then aborted with this exit code".

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::{compile_and_run, compile_and_run_flags, compile_and_run_raw, compile_only};

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
        // The promptless INPUT prints `? `, which shares the line with the
        // PRINT that follows it.
        vec!["? read"],
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

/// Helper: a program must run, produce `stdout_lines`, then abort with the
/// given message on stderr and a nonzero exit code.
fn expect_runtime_error(source: &str, stdout_lines: &[&str], message: &str) {
    let run = compile_and_run_raw(source, "").expect("should compile");
    assert_eq!(run.lines(), stdout_lines, "output before the abort");
    assert!(
        run.stderr.contains(message),
        "expected {message:?} on stderr, got {:?}",
        run.stderr
    );
    assert_eq!(
        run.exit_code,
        Some(1),
        "a runtime error exits 1; stderr was {:?}",
        run.stderr
    );
}

/// An out-of-range subscript used to corrupt memory or segfault.
#[test]
fn test_array_bounds_are_checked() {
    expect_runtime_error(
        "DIM A(5)\nPRINT \"before\"\nA(99) = 1\nPRINT \"after\"\n",
        &["before"],
        "Subscript out of range",
    );
    // A negative index is caught by the same unsigned compare.
    expect_runtime_error(
        "DIM A(5)\nI = -1\nPRINT A(I)\n",
        &[],
        "Subscript out of range",
    );
    expect_runtime_error(
        "DIM M(2,2)\nPRINT \"before\"\nPRINT M(1,9)\n",
        &["before"],
        "Subscript out of range",
    );
}

/// Indexing right at the declared bound is legal: DIM A(5) has 6 elements.
#[test]
fn test_array_bounds_allow_the_declared_bound() {
    let run = compile_and_run_raw("DIM A(5)\nA(5) = 42\nA(0) = 1\nPRINT A(5)\n", "")
        .expect("should compile");
    assert_eq!(run.lines(), vec!["42"]);
    assert_eq!(run.exit_code, Some(0));
}

/// Integer division and MOD by zero used to raise SIGFPE; float division
/// yielded infinity and carried on.
#[test]
fn test_division_by_zero_is_checked() {
    expect_runtime_error(
        "PRINT \"before\"\nPRINT 1 \\ 0\n",
        &["before"],
        "Division by zero",
    );
    expect_runtime_error(
        "PRINT \"before\"\nPRINT 1 MOD 0\n",
        &["before"],
        "Division by zero",
    );
    expect_runtime_error(
        "PRINT \"before\"\nPRINT 1 / 0\n",
        &["before"],
        "Division by zero",
    );
    expect_runtime_error("Z = 0\nPRINT 5 / Z\n", &[], "Division by zero");
}

/// SQR of a negative and LOG of a non-positive argument used to yield NaN or
/// -inf, which printed as a meaningless huge integer.
#[test]
fn test_math_domain_is_checked() {
    expect_runtime_error("PRINT SQR(-1)\n", &[], "Illegal function call");
    expect_runtime_error("PRINT LOG(0)\n", &[], "Illegal function call");
    expect_runtime_error("PRINT LOG(-1)\n", &[], "Illegal function call");
}

/// An array whose DIM has not executed yet has a null element pointer.
#[test]
fn test_use_before_dim_is_checked() {
    expect_runtime_error(
        "SUB T\nPRINT A(0)\nEND SUB\nT\nDIM A(3)\n",
        &[],
        "Array used before DIM",
    );
}

/// The diagnostic names the BASIC line the fault occurred on.
#[test]
fn test_runtime_error_reports_line() {
    let run = compile_and_run_raw("DIM A(2)\nPRINT \"x\"\nA(9) = 1\n", "").expect("should compile");
    assert!(
        run.stderr.contains("in 3"),
        "expected the failing line in the message, got {:?}",
        run.stderr
    );
}

/// `ERROR n` raises the numbered error, with GW-BASIC's own numbering.
///
/// The numbers are the interface: a listing writes `IF ERR = 53 THEN` and
/// means "file not found". Raising by number is the half of that which works
/// without a handler, and it is what pins the table.
#[test]
fn test_error_statement_raises_by_number() {
    for (source, message) in [
        ("ERROR 5\n", "Illegal function call"),
        ("ERROR 6\n", "Overflow"),
        ("ERROR 7\n", "Out of memory"),
        ("ERROR 9\n", "Subscript out of range"),
        ("ERROR 11\n", "Division by zero"),
        ("ERROR 50\n", "FIELD overflow"),
        ("ERROR 52\n", "Bad file number"),
        ("ERROR 53\n", "File not found"),
        ("ERROR 54\n", "Bad file mode"),
        ("ERROR 55\n", "File already open"),
        ("ERROR 62\n", "Input past end of file"),
        ("ERROR 70\n", "Permission denied"),
    ] {
        let run = compile_and_run_raw(source, "").expect("should compile");
        assert!(
            run.stderr.contains(message),
            "ERROR should raise {message:?}, got {:?}",
            run.stderr
        );
        assert_eq!(run.exit_code, Some(1), "a raised error still aborts");
    }
}

/// A number the table does not know is still an error, and says so.
#[test]
fn test_error_statement_with_an_unknown_number() {
    let run = compile_and_run_raw("ERROR 200\n", "").expect("should compile");
    assert!(
        run.stderr.contains("Unprintable error"),
        "GW-BASIC's own wording for a code it has no message for: {:?}",
        run.stderr
    );
    assert_eq!(run.exit_code, Some(1));
}

/// The raised error carries the line, like any other.
#[test]
fn test_error_statement_reports_its_line() {
    let run = compile_and_run_raw("10 PRINT \"x\"\n20 ERROR 11\n", "").expect("should compile");
    assert!(
        run.stderr.contains("in 20"),
        "expected the BASIC line, got {:?}",
        run.stderr
    );
}

/// The operand is evaluated, not just a literal.
#[test]
fn test_error_statement_takes_an_expression() {
    let run = compile_and_run_raw("N = 50\nERROR N + 3\n", "").expect("should compile");
    assert!(
        run.stderr.contains("File not found"),
        "53 should come out of the expression: {:?}",
        run.stderr
    );
}

/// GW-BASIC restricts the code to 1..255, and so does this.
#[test]
fn test_error_statement_rejects_a_code_out_of_range() {
    for source in ["ERROR 0\n", "ERROR 256\n", "ERROR -1\n"] {
        let run = compile_and_run_raw(source, "").expect("should compile");
        assert!(
            run.stderr.contains("Illegal function call"),
            "{source:?} should be refused at run time, got {:?}",
            run.stderr
        );
    }
}

/// A bad numeric operand is diagnosed, not a compiler panic.
///
/// `check_stmt` has a wildcard, so nothing forced an arm for a new statement
/// and the operands of four of them were never checked at all. `ERROR
/// NoSuchFn(1)` reached the codegen line that asserts "sema checked the array
/// is declared" -- it had not -- and `LOCATE A$, 1` reached the
/// implicit-String-conversion panic. A builtin's arguments were always
/// checked, which is why `SQR(A$)` says so properly; a statement's were not.
/// Turning panics into diagnostics is the point of that pass.
#[test]
fn test_numeric_operands_are_checked_not_panicked_on() {
    for (source, expect) in [
        ("A$ = \"x\"\nERROR A$\n", "ERROR needs a number"),
        ("ERROR \"boom\"\n", "ERROR needs a number"),
        ("ERROR NoSuchFn(1)\n", "unknown function or array"),
        // The same hole, and the same fix, for every statement that takes a
        // bare numeric operand. All four were added in the last two batches.
        ("A$ = \"x\"\nLOCATE A$, 1\n", "LOCATE needs a number"),
        ("A$ = \"x\"\nCOLOR 1, A$\n", "COLOR needs a number"),
        ("A$ = \"x\"\nRANDOMIZE A$\n", "RANDOMIZE needs a number"),
        ("LOCATE NoSuchFn(1), 1\n", "unknown function or array"),
        ("RANDOMIZE NoSuchFn(1)\n", "unknown function or array"),
    ] {
        let err =
            compile_only(source).expect_err(&format!("{} must be refused", source.escape_debug()));
        assert!(
            err.is_clean_rejection(),
            "must be diagnosed, not panic: {}",
            err.stderr
        );
        assert!(
            err.contains(expect),
            "expected {expect:?} in the diagnostic, got: {}",
            err.stderr
        );
    }
}

/// Any expression may be the error number, including one that starts with NOT.
///
/// The token test that decides whether `ERROR` leads a statement listed the
/// tokens an expression can start with, and missed `NOT` and a string literal.
/// `ERROR NOT 0` was then refused as though the statement did not exist.
#[test]
fn test_error_statement_accepts_any_expression_shape() {
    // NOT 0 is -1, which is out of the 1..255 range, so the run reports the
    // range rather than the parse -- which is the point: it parsed.
    let run = compile_and_run_raw("ERROR NOT 0\n", "").expect("should compile");
    assert!(
        run.stderr.contains("Illegal function call"),
        "NOT 0 should reach the range check, got {:?}",
        run.stderr
    );

    let run = compile_and_run_raw("N = 10\nERROR NOT N\n", "").expect("should compile");
    assert!(
        run.stderr.contains("Illegal function call"),
        "{:?}",
        run.stderr
    );

    let run = compile_and_run_raw("ERROR (5)\n", "").expect("should compile");
    assert!(
        run.stderr.contains("Illegal function call"),
        "{:?}",
        run.stderr
    );
}

/// `ERROR` is still not a value, and `ON ERROR` is still refused.
#[test]
fn test_error_is_a_statement_not_a_name() {
    // It stays in the UNSUPPORTED table for exactly this: recognised as a
    // statement only when an operand follows it, so every other mention still
    // gets a reason rather than becoming a variable that reads as zero.
    for source in ["PRINT ERROR\n", "ERROR\n", "X = ERROR + 1\n"] {
        let err = compile_only(source).expect_err("ERROR is not a value");
        assert!(
            err.contains("not supported"),
            "{source:?} got: {}",
            err.stderr
        );
        assert!(err.is_clean_rejection());
    }
}

/// A line-numbered listing reports its BASIC line number, not the source line.
///
/// These are two different numbering systems and the error path used the wrong
/// one: `current_line` is the lexer's physical line, while the `_line_NNN`
/// labels a program branches to come from `StmtKind::Label(n)`. A listing whose
/// line 110 failed said "in 4", so the number in the message named nothing the
/// programmer could see, and disagreed with LANGREF's own example.
#[test]
fn test_error_reports_the_basic_line_number() {
    let run = compile_and_run_raw(
        "REM a header comment\nREM and another\n100 DIM A(3)\n110 PRINT A(99)\n",
        "",
    )
    .expect("should compile");
    assert!(
        run.stderr.contains("in 110"),
        "expected the BASIC line 110, got {:?}",
        run.stderr
    );
}

/// A program written without line numbers still reports its source line.
///
/// GW-BASIC has nothing else to say here and reports nothing; a source line is
/// what a programmer can act on, and it is the style everything in examples/
/// is written in.
#[test]
fn test_error_reports_the_source_line_without_line_numbers() {
    let run = compile_and_run_raw("DIM A(2)\nPRINT \"x\"\nA(9) = 1\n", "").expect("should compile");
    assert!(
        run.stderr.contains("in 3"),
        "expected the source line 3, got {:?}",
        run.stderr
    );
}

/// A statement ahead of the first line number has no BASIC line to report.
#[test]
fn test_error_before_the_first_line_number() {
    let run = compile_and_run_raw("DIM A(2)\nA(9) = 1\n100 END\n", "").expect("should compile");
    assert!(
        run.stderr.contains("in 2"),
        "nothing numbered has run yet, so the source line stands: {:?}",
        run.stderr
    );
}

/// Runtime diagnostics go to stderr, leaving the program's own output clean.
#[test]
fn test_runtime_errors_go_to_stderr() {
    let run =
        compile_and_run_raw("DIM A(2)\nPRINT \"out\"\nA(9) = 1\n", "").expect("should compile");
    assert_eq!(run.lines(), vec!["out"], "stdout has only program output");
    assert!(run.stderr.contains("Subscript out of range"));
}

/// Correct programs are unaffected by the checks.
#[test]
fn test_checks_do_not_disturb_correct_programs() {
    let run = compile_and_run_raw(
        r#"
DIM A(9)
FOR I = 0 TO 9
A(I) = I * 2
NEXT I
S = 0
FOR I = 0 TO 9
S = S + A(I)
NEXT I
PRINT S
PRINT 10 / 4
PRINT 10 \ 4
PRINT 10 MOD 4
PRINT SQR(16)
PRINT LOG(1)
"#,
        "",
    )
    .expect("should compile");
    assert_eq!(run.lines(), vec!["90", "2.5", "2", "2", "4", "0"]);
    assert_eq!(run.exit_code, Some(0));
}

/// The new statements are checked for misuse.
#[test]
fn test_new_statement_misuse_is_diagnosed() {
    expect_rejected("EXIT FOR\n", "EXIT outside of a FOR loop");
    expect_rejected("EXIT DO\n", "EXIT outside of a DO or WHILE loop");
    expect_rejected("EXIT SUB\n", "EXIT SUB/FUNCTION outside of a procedure");
    expect_rejected(
        "A = 1\nB$ = \"x\"\nSWAP A, B$\n",
        "SWAP requires both values to be the same type",
    );
    expect_rejected("X = 1\nCONST C = X\n", "must have a constant value");
    expect_rejected(
        "F$ = \"##\"\nPRINT USING F$; 1\n",
        "PRINT USING requires a literal format string",
    );
}

/// OPTION BASE placement and value are checked, and subscript 0 becomes out of
/// range once base 1 is in effect.
#[test]
fn test_option_base_rules() {
    expect_rejected("DIM A(3)\nOPTION BASE 1\n", "must come before any DIM");
    expect_rejected("OPTION BASE 1\nOPTION BASE 0\n", "may appear only once");
    expect_rejected("OPTION BASE 2\n", "OPTION BASE takes 0 or 1");
    expect_rejected("DEF ABC(X) = X\n", "must begin with FN");

    let run = compile_and_run_raw("OPTION BASE 1\nDIM A(3)\nPRINT A(0)\n", "").unwrap();
    assert!(run.stderr.contains("Subscript out of range"));
    assert_eq!(run.exit_code, Some(1));
}

/// REDIM must keep the array's rank, and PRESERVE may only change the last
/// dimension -- QuickBASIC's own rule, since any other change would need the
/// elements remapped rather than the block simply grown.
#[test]
fn test_redim_rules() {
    expect_rejected("DIM A(2,2)\nREDIM A(3)\n", "must keep its 2 dimension(s)");
    expect_rejected(
        "DIM A(2,2)\nREDIM PRESERVE A(2,3)\n",
        "may only change its last dimension",
    );
}

/// TYPE declarations and uses are checked.
#[test]
fn test_type_rules() {
    expect_rejected(
        "TYPE P\nX AS INTEGER\nEND TYPE\nDIM Q AS P\nPRINT Q.Z\n",
        "TYPE 'P' has no field 'Z'",
    );
    expect_rejected("DIM Q AS NoSuch\n", "undefined TYPE 'NOSUCH'");
    expect_rejected("A = 1\nPRINT A.X\n", "is not a record variable");
    expect_rejected(
        "TYPE P\nX AS INTEGER\nX AS LONG\nEND TYPE\n",
        "is declared twice",
    );
    expect_rejected("TYPE P\nQ AS P\nEND TYPE\n", "cannot contain itself");
    expect_rejected(
        "TYPE P\nX AS INTEGER\nEND TYPE\nTYPE Q\nY AS INTEGER\nEND TYPE\nDIM A AS P\nDIM B AS Q\nA = B\n",
        "can only be assigned another P record",
    );
    // A record-returning FUNCTION would need the caller to provide storage;
    // rejected rather than miscompiled.
    expect_rejected(
        "TYPE P\nX AS INTEGER\nEND TYPE\nFUNCTION Make AS P\nEND FUNCTION\n",
        "cannot return the record type",
    );
}

/// Inputs that used to abort the compiler with a Rust panic instead of a
/// diagnostic. Every rejection here must be clean (exit 1, not 101).
#[test]
fn test_type_confusion_is_diagnosed_not_panicked() {
    expect_rejected("DIM A(3)\nA(0) = \"x\"\n", "cannot assign string value");
    expect_rejected("DIM S$(3)\nS$(0) = 1\n", "cannot assign numeric value");
    expect_rejected(
        "SUB T(N)\nPRINT N\nEND SUB\nT(\"x\")\n",
        "argument 1 of 'T' is numeric, but a string value was given",
    );
    expect_rejected(
        "SUB T(S$)\nPRINT S$\nEND SUB\nT(1)\n",
        "argument 1 of 'T' is string, but a numeric value was given",
    );
    expect_rejected("PRINT SQR(\"x\")\n", "'SQR' takes a numeric argument");
    expect_rejected(
        "OPEN \"f\" FOR OUTPUT AS #\"x\"\n",
        "a file number must be numeric",
    );
    expect_rejected("FOR I$ = 1 TO 3\nNEXT I$\n", "must be numeric");
    expect_rejected("DIM A(3)\nPRINT A(\"x\")\n", "subscript must be numeric");
    expect_rejected("DIM A()\n", "needs at least one dimension");
}

/// A whole record is not a value, though it may be assigned or passed.
#[test]
fn test_whole_record_is_not_a_value() {
    let ty = "TYPE P\nX AS INTEGER\nEND TYPE\nDIM Q AS P\n";
    expect_rejected(&format!("{ty}PRINT Q\n"), "has no value");
    expect_rejected(&format!("{ty}PRINT Q + 1\n"), "has no value");
}

/// An input target is checked the way a value of the same shape would be.
///
/// Nothing checked these at all: `LINE INPUT N` compiled, and since codegen
/// reads a string and stores it as it stands, N was handed a pointer to
/// reinterpret as a double -- it printed 4.3e-315. A whole record, or a field
/// that does not exist, went the same way unnoticed.
#[test]
fn test_input_targets_are_checked() {
    let ty = "TYPE P\nX AS INTEGER\nEND TYPE\nDIM R AS P\n";
    expect_rejected("LINE INPUT N\n", "LINE INPUT needs a string variable");
    expect_rejected(
        "OPEN \"f\" FOR INPUT AS #1\nLINE INPUT #1, N\n",
        "LINE INPUT needs a string variable",
    );
    expect_rejected(&format!("{ty}INPUT R\n"), "has no value");
    expect_rejected(&format!("{ty}READ R\n"), "has no value");
    expect_rejected(&format!("{ty}INPUT R.NOSUCH\n"), "has no field 'NOSUCH'");
}

/// ...and the forms that were always legal still are.
#[test]
fn test_valid_input_targets_still_compile() {
    let ty = "TYPE P\nX AS INTEGER\nEND TYPE\nDIM R AS P\n";
    for source in [
        "LINE INPUT S$\n",
        "INPUT N\n",
        "INPUT A$, B\n",
        "DIM A(3)\nINPUT A(1)\n",
        &format!("{ty}INPUT R.X\n"),
        &format!("{ty}DATA 1\nREAD R.X\n"),
    ] {
        compile_only(source).unwrap_or_else(|e| panic!("{source:?} should compile:\n{}", e.stderr));
    }
}

/// A file number outside 1-15 is refused rather than indexing off the table.
///
/// The runtime's handle table is 16 slots; nothing checked the index, so
/// `AS #900000` wrote a FILE* far outside it and the program died with a
/// segfault instead of a diagnostic. Slot 0 is stdout, so it is not a
/// program's to open either.
#[test]
fn test_file_number_out_of_range() {
    expect_rejected("OPEN \"a\" FOR OUTPUT AS #0\n", "file number");
    expect_rejected("OPEN \"a\" FOR OUTPUT AS #16\n", "file number");
    expect_rejected("CLOSE #99\n", "file number");
    expect_rejected("PRINT EOF(0)\n", "file number");
}

/// The same check at run time, when the number is not a constant.
#[test]
fn test_file_number_out_of_range_at_runtime() {
    for source in [
        "PRINT \"before\"\nN = 900000\nCLOSE #N\n",
        "PRINT \"before\"\nN = 900000\nPRINT EOF(N)\n",
        "PRINT \"before\"\nN = 900000\nPRINT LOF(N)\n",
        "PRINT \"before\"\nN = 0\nOPEN \"a\" FOR OUTPUT AS #N\n",
    ] {
        let run = compile_and_run_raw(source, "").unwrap();
        assert!(
            run.stderr.contains("Bad file number"),
            "expected a diagnostic for {:?}, got {:?} / {:?}",
            source,
            run.stdout,
            run.stderr
        );
        assert_eq!(run.exit_code, Some(1), "for {:?}", source);
    }
}

/// ...including on the way to a file.
///
/// Sema's `PrintFile` arm is a copy of the `Print` one that never called
/// `reject_record_value`, so `PRINT #1, Q` compiled and wrote garbage while
/// `PRINT Q` was correctly refused.
#[test]
fn test_whole_record_is_not_a_value_to_a_file() {
    let ty = "TYPE P\nX AS INTEGER\nEND TYPE\nDIM Q AS P\n";
    expect_rejected(
        &format!("{ty}OPEN \"r.txt\" FOR OUTPUT AS #1\nPRINT #1, Q\n"),
        "has no value",
    );
}

/// Using an array before its DIM has executed is a runtime error, not a
/// compiler panic: the descriptor exists from the start, with a null element
/// pointer until the DIM runs.
#[test]
fn test_use_before_dim_runs() {
    let run = compile_and_run_raw("PRINT A(0)\nDIM A(3)\n", "").expect("should compile");
    assert!(
        run.stderr.contains("Array used before DIM"),
        "expected a runtime diagnostic, got {:?}",
        run.stderr
    );
    assert_eq!(run.exit_code, Some(1));
}

/// Anything that is not a record lvalue must be rejected where a record
/// parameter is expected, rather than crashing at run time.
#[test]
fn test_record_parameter_rejects_non_records() {
    let header = "TYPE P\nX AS INTEGER\nEND TYPE\nSUB Show(V AS P)\nPRINT V.X\nEND SUB\n";

    for arg in ["1", "N", "\"s\""] {
        let source = format!("{header}Show {arg}\n");
        let err = compile_only(&source).expect_err("should be rejected");
        assert!(err.is_clean_rejection(), "{}", err.stderr);
        assert!(
            err.stderr.contains("must be TYPE P"),
            "wrong message for {arg}: {}",
            err.stderr
        );
    }
}

/// A record of the wrong type names both types in the diagnostic.
#[test]
fn test_record_parameter_rejects_wrong_type() {
    let err = compile_only(
        "TYPE P\nX AS INTEGER\nEND TYPE\nTYPE R\nY AS INTEGER\nEND TYPE\nSUB Show(V AS P)\nPRINT V.X\nEND SUB\nDIM W AS R\nShow W\n",
    )
    .expect_err("should be rejected");
    assert!(err.is_clean_rejection(), "{}", err.stderr);
    assert!(
        err.stderr
            .contains("is TYPE P, but a TYPE R value was given"),
        "{}",
        err.stderr
    );
}

/// A declared result type that contradicts the name's suffix is ambiguous.
#[test]
fn test_function_type_conflicts_with_suffix() {
    let err = compile_only("FUNCTION M$(X) AS INTEGER\nM$ = \"a\"\nEND FUNCTION\nPRINT M$(1)\n")
        .expect_err("should be rejected");
    assert!(err.is_clean_rejection(), "{}", err.stderr);
    assert!(err.stderr.contains("suffix"), "{}", err.stderr);
}

/// A SUB has no result, so an AS clause on one describes nothing.
#[test]
fn test_sub_cannot_be_declared_as_a_type() {
    let err = compile_only("SUB T(X) AS INTEGER\nPRINT X\nEND SUB\nT 1\n")
        .expect_err("should be rejected");
    assert!(err.is_clean_rejection(), "{}", err.stderr);
    assert!(err.stderr.contains("no return value"), "{}", err.stderr);
}

/// A dimension the array does not have is an error, not a read past the
/// descriptor.
#[test]
fn test_bounds_dimension_out_of_range() {
    let err = compile_only("DIM A(4)\nPRINT UBOUND(A, 3)\n").expect_err("should be rejected");
    assert!(err.is_clean_rejection(), "{}", err.stderr);
    assert!(
        err.stderr.contains("dimension 3 does not exist"),
        "{}",
        err.stderr
    );
}

/// The same check at run time, when the dimension is computed.
#[test]
fn test_bounds_computed_dimension_out_of_range() {
    let run = compile_and_run_raw("DIM A(4)\nK = 3\nPRINT UBOUND(A, K)\n", "").unwrap();
    assert_eq!(run.exit_code, Some(1));
    assert!(
        run.stderr.contains("Subscript out of range"),
        "{}",
        run.stderr
    );
}

/// LBOUND/UBOUND need an array, and a dimension that is a number.
#[test]
fn test_bounds_argument_diagnostics() {
    let err = compile_only("PRINT UBOUND(Z)\n").expect_err("should be rejected");
    assert!(
        err.stderr.contains("not a declared array"),
        "{}",
        err.stderr
    );

    let err = compile_only("DIM A(4)\nPRINT UBOUND(A, \"x\")\n").expect_err("should be rejected");
    assert!(err.stderr.contains("must be numeric"), "{}", err.stderr);
}

/// LET still needs an assignment, not a bare call.
#[test]
fn test_let_requires_an_assignment() {
    let err = compile_only("SUB T\nPRINT 1\nEND SUB\nLET T\n").expect_err("should be rejected");
    assert!(err.is_clean_rejection(), "{}", err.stderr);
    assert!(
        err.stderr.contains("LET needs an assignment"),
        "{}",
        err.stderr
    );
}

/// Exhausting the GOSUB return stack must abort cleanly.
///
/// This is the one runtime helper no test reached: the GOSUB stack holds 64K
/// entries, so nothing short of unbounded recursion touches it. It is also the
/// path whose message length was once hand-counted wrong, and the last
/// diagnostic that went to stdout instead of stderr.
#[test]
fn test_gosub_stack_overflow() {
    let run = compile_and_run_raw("PRINT \"before\"\n100 GOSUB 100\nRETURN\n", "").unwrap();
    assert_eq!(run.exit_code, Some(1));
    // Output produced before the abort survives, and the diagnostic does not
    // pollute it.
    assert_eq!(run.stdout.trim(), "before");
    assert!(
        run.stderr.contains("GOSUB stack overflow"),
        "stderr was {:?}",
        run.stderr
    );
}

/// `--unsafe` removes the runtime checks, and the program still works.
///
/// The flag's entire effect is on emitted code, so without a test that runs
/// something built with it, it could stop working -- or stop removing anything
/// -- and nothing would notice.
#[test]
fn test_unsafe_flag_still_produces_correct_programs() {
    let source = "DIM A(5)\nFOR I = 0 TO 5\nA(I) = I * I\nNEXT I\nPRINT A(3); A(5)\nPRINT 7 \\ 2\n";

    let checked = compile_and_run_raw(source, "").unwrap();
    let unchecked = compile_and_run_flags(source, "", &["--unsafe"]).unwrap();

    let expected: Vec<&str> = vec!["925", "3"];
    assert_eq!(checked.stdout.trim().lines().collect::<Vec<_>>(), expected);
    assert_eq!(
        unchecked.stdout.trim(),
        checked.stdout.trim(),
        "--unsafe must not change a correct program's output"
    );
    assert_eq!(unchecked.exit_code, Some(0));
}

/// A bounds violation is caught with checks on, and is not with `--unsafe`.
#[test]
fn test_unsafe_flag_removes_the_bounds_check() {
    let source = "DIM A(2)\nPRINT \"start\"\nA(99) = 1\nPRINT \"end\"\n";

    let checked = compile_and_run_raw(source, "").unwrap();
    assert_eq!(checked.exit_code, Some(1));
    assert!(checked.stderr.contains("Subscript out of range"));

    // Without the check the write goes through; the program is in undefined
    // behaviour, so only the absence of the diagnostic is assertable.
    let unchecked = compile_and_run_flags(source, "", &["--unsafe"]).unwrap();
    assert!(
        !unchecked.stderr.contains("Subscript out of range"),
        "--unsafe still emitted the check: {:?}",
        unchecked.stderr
    );
}

/// A built-in that takes no argument cannot double as a variable name.
///
/// A bare `TIMER` is a call, so allowing `TIMER = 5` would make the write and
/// the read refer to different things.
#[test]
fn test_zero_arg_builtins_are_not_assignable() {
    for name in ["TIMER", "RND"] {
        let err =
            compile_only(&format!("{name} = 5\nPRINT {name}\n")).expect_err("should be rejected");
        assert!(err.is_clean_rejection(), "{}", err.stderr);
        assert!(
            err.stderr.contains("cannot be assigned to"),
            "{name}: {}",
            err.stderr
        );
    }
}

// ---------------------------------------------------------------------------
// Unimplemented GW-BASIC names
//
// These matter more than a typo diagnostic does. An unrecognised name is
// ordinarily just a new variable, so before the compiler knew these words,
// `PRINT DATE$` printed an empty string and `ON ERROR GOTO 100` compiled into
// a computed GOTO on a variable that is always zero -- falling straight
// through the error handler a program was relying on. A GW-BASIC listing
// compiled clean and then quietly did the wrong thing.

/// The whole point: every one of these is refused rather than silently read as
/// an empty variable.
#[test]
fn test_unimplemented_gwbasic_names_are_diagnosed() {
    let cases = [
        ("A$ = INKEY$\n", "INKEY$"),
        ("PRINT CSRLIN\n", "CSRLIN"),
        ("A$ = INPUT$(3)\n", "INPUT$"),
        ("PRINT PEEK(0)\n", "PEEK"),
        ("POKE 0, 1\n", "POKE"),
        ("SCREEN 13\n", "SCREEN"),
        ("PLAY \"cde\"\n", "PLAY"),
        ("LPRINT \"x\"\n", "LPRINT"),
        ("CHAIN \"other\"\n", "CHAIN"),
    ];

    for (source, name) in cases {
        let err = compile_only(source)
            .expect_err(&format!("{} must be refused, not silently accepted", name));
        assert!(
            err.contains("not supported"),
            "{} was refused, but without saying why: {}",
            name,
            err.stderr
        );
        assert!(
            err.is_clean_rejection(),
            "{} should be diagnosed, not panic on: {}",
            name,
            err.stderr
        );
    }
}

/// `ON ERROR GOTO` still cannot fall through silently.
///
/// It once parsed as a computed GOTO on a variable named ERROR, which is
/// always zero, so the handler never ran and nothing said so. It is a real
/// statement now, and the property that mattered is unchanged: a handler
/// naming a line the program does not have is refused, not ignored.
#[test]
fn test_on_error_goto_a_missing_line_is_refused() {
    let err = compile_only("ON ERROR GOTO 100\nPRINT \"x\"\nEND\n")
        .expect_err("a handler must name a line that exists");
    assert!(
        err.contains("ON ERROR GOTO"),
        "expected the statement named, got: {}",
        err.stderr
    );
    assert!(err.is_clean_rejection());
}

/// Assigning to one of these names is a GW-BASIC statement, not the creation
/// of a variable that happens to be called TIME$ or INKEY$.
#[test]
fn test_assignment_to_an_unimplemented_name_is_refused() {
    // Still unimplemented: the refusal explains itself.
    let err = compile_only("INKEY$ = \"x\"\n").expect_err("INKEY$ = ... must be refused");
    assert!(err.contains("not supported"), "got: {}", err.stderr);

    // Implemented as a function: GW-BASIC's `DATE$ = ...` sets the system
    // clock, which this does not do, so it is refused for a different reason.
    let err = compile_only("DATE$ = \"01-01-2026\"\n").expect_err("DATE$ = ... must be refused");
    assert!(err.contains("built-in function"), "got: {}", err.stderr);
}

/// The diagnostic says why, and distinguishes "not yet" from "not ever".
#[test]
fn test_unsupported_diagnostics_explain_themselves() {
    let err = compile_only("PRINT PEEK(0)\n").expect_err("PEEK must be refused");
    assert!(
        err.contains("direct memory access"),
        "a permanent non-goal should say so: {}",
        err.stderr
    );

    let err = compile_only("PRINT INKEY$\n").expect_err("INKEY$ must be refused");
    assert!(
        err.contains("not implemented yet"),
        "a planned feature should say so: {}",
        err.stderr
    );

    // Sound was once refused as "not supported", which read as a decision
    // rather than a queue position. These are deferred, not declined.
    for source in ["SOUND 440, 5\n", "PLAY \"cde\"\n", "LPRINT \"x\"\n"] {
        let err = compile_only(source).expect_err("must still be refused");
        assert!(
            err.contains("not implemented yet"),
            "a deferred feature should say so: {}",
            err.stderr
        );
    }
}

/// A `DEF*` default must not rename a name the compiler refuses.
///
/// `apply_default_types` rewrites every unsuffixed name a `DEF*` range covers,
/// and `unsupported_reason` matches the unsuffixed spelling -- so under
/// `DEFINT A-Z` the table was consulted for `ERROR%` and missed. That is not a
/// cosmetic gap: `ON ERROR GOTO 100` went back to compiling as a computed GOTO
/// on a variable that is always zero, which is the precise silent
/// fall-through the table was written to prevent, restored by a feature added
/// four commits later.
#[test]
fn test_a_def_type_does_not_defeat_the_refusals() {
    // Every shape the refusal is reached through: a bare name in an
    // expression, a statement-position call, and the ON ERROR special case.
    let cases = [
        ("DEFINT A-Z\nPRINT ERROR\n", "ERROR"),
        ("DEFINT A-Z\nPRINT CSRLIN\n", "CSRLIN"),
        ("DEFSTR A-Z\nPRINT INKEY$\n", "INKEY$"),
        ("DEFLNG A-Z\nPRINT CSRLIN\n", "CSRLIN"),
    ];

    for (source, name) in cases {
        let err = compile_only(source)
            .expect_err(&format!("{} must stay refused under a DEF* default", name));
        assert!(
            err.contains("not supported"),
            "{name} was accepted under a DEF* default: {}",
            err.stderr
        );
        assert!(err.is_clean_rejection());
    }
}

/// `NAME` was documented as unimplemented but missing from the table, so it
/// fell through to "unknown subroutine" -- the generic message the table exists
/// to replace. Nothing else distinguishes a name this compiler knows about and
/// has not written from one the program simply misspelled.
#[test]
fn test_documented_unimplemented_names_are_all_in_the_table() {
    for source in ["NAME \"a\"\n", "KILL \"a\"\n", "FILES\n", "SHELL \"ls\"\n"] {
        let err = compile_only(source).expect_err("must be refused");
        assert!(
            err.contains("not implemented yet"),
            "expected the table's message, got: {}",
            err.stderr
        );
        assert!(
            !err.contains("unknown subroutine"),
            "fell through to the generic message: {}",
            err.stderr
        );
    }
}

/// A name cannot be both an array and a procedure, or an array and a builtin.
///
/// `A(1)` has to resolve to one thing. Sema resolved such a clash in favour of
/// the array and codegen in favour of the procedure, and the parser's DIM-order
/// heuristic hid the disagreement for as long as it lasted -- both of these
/// compiled silently. Nobody writes this on purpose, and either resolution
/// surprises somebody, so the program is refused instead.
#[test]
fn test_array_and_procedure_name_collisions_are_diagnosed() {
    expect_rejected(
        "DIM F(5)\nF(1) = 7\nFUNCTION F(X)\nF = X * 2\nEND FUNCTION\n",
        "declared both as an array and as a FUNCTION",
    );
    expect_rejected(
        "DIM S(5)\nSUB S(X)\nPRINT X\nEND SUB\n",
        "declared both as an array and as a SUB",
    );
    expect_rejected(
        "DIM LEN(5)\nLEN(1) = 7\n",
        "'LEN' is the name of a built-in function",
    );
}

/// Every syntax error in a program is reported, not just the first.
///
/// Sema has always returned a Vec<Diagnostic>, so five undefined names cost one
/// compile. The parser stopped at the first error, so five typos cost five.
#[test]
fn test_parser_reports_every_error() {
    let e = compile_only("X = )\nGOTO +\nY = *\nPRINT \"ok\"\n")
        .expect_err("three bad statements must be refused");
    for expected in [
        "unexpected ) in an expression",
        "expected a line number or label, got +",
        "unexpected * in an expression",
    ] {
        assert!(
            e.contains(expected),
            "expected {expected:?} among the diagnostics: {}",
            e.stderr
        );
    }
    assert!(
        e.contains("3 errors"),
        "the tally should say 3: {}",
        e.stderr
    );
}

/// One error is "1 error", not "1 errors".
#[test]
fn test_single_error_tally_is_singular() {
    let e = compile_only("X = )\n").expect_err("must be refused");
    assert!(e.contains("1 error\n") || e.stderr.trim_end().ends_with("1 error"));
}

/// Recovery happens inside blocks too, so one bad statement costs that
/// statement and not the block around it.
///
/// Recovering only at the top level would let the error escape the SUB, strand
/// its END SUB, and produce a cascade of complaints about a SUB that was
/// perfectly well closed.
#[test]
fn test_recovery_inside_a_block_does_not_cascade() {
    let e = compile_only("SUB Foo\nX = )\nPRINT 1\nEND SUB\nPRINT 2\n")
        .expect_err("the bad statement must be refused");
    assert!(
        e.contains("1 error"),
        "only the bad statement should be reported: {}",
        e.stderr
    );
    assert!(
        !e.contains("without matching") && !e.contains("missing its"),
        "the SUB was closed correctly and must not be blamed: {}",
        e.stderr
    );
}

/// Errors found before a block that never closes are kept, not discarded.
#[test]
fn test_hard_error_keeps_the_errors_found_before_it() {
    let e = compile_only("X = )\nFOR I = 1 TO 10\nPRINT I\n")
        .expect_err("an unclosed FOR must be refused");
    assert!(
        e.contains("unexpected ) in an expression"),
        "the earlier error must survive: {}",
        e.stderr
    );
    assert!(
        e.contains("FOR is missing its NEXT"),
        "the unclosed block must be reported: {}",
        e.stderr
    );
}

/// Diagnostics quote BASIC, not Rust.
///
/// Errors fell back to `{:?}` on the token, so they showed the lexer's variant
/// names: "Expected To, got Integer(2)" for a missing TO, and `EndSelect`,
/// `LParen` and `Ne` at programmers who had written `END SELECT`, `(` and `<>`.
/// Every fixed token now carries the spelling it was written with.
#[test]
fn test_diagnostics_quote_source_spelling() {
    let cases = [
        ("FOR I = 1 2 3\nNEXT\n", "expected TO, got 2"),
        ("IF THEN\n", "unexpected THEN in an expression"),
        ("GOTO +\n", "expected a line number or label, got +"),
        ("X = )\n", "unexpected ) in an expression"),
        (
            "OPEN \"f\" FOR BOGUS AS #1\n",
            "expected INPUT, OUTPUT, APPEND or RANDOM, got identifier 'BOGUS'",
        ),
    ];
    for (source, expected) in cases {
        expect_rejected(source, expected);
    }
}

/// No diagnostic may leak a Rust variant name.
///
/// A cheap guard over the whole set: these are the spellings `{:?}` produced,
/// and none of them is a thing anyone can type in BASIC.
#[test]
fn test_diagnostics_never_show_rust_variant_names() {
    let sources = [
        "FOR I = 1 2 3\nNEXT\n",
        "X = )\n",
        "IF THEN\n",
        "GOTO +\n",
        "X = 1 <> \n",
        "SELECT CASE\n",
    ];
    for source in sources {
        let Err(e) = compile_only(source) else {
            continue;
        };
        for leaked in [
            "EndSelect",
            "LParen",
            "RParen",
            "Integer(",
            "Ident(",
            "Newline",
            "ElseIf",
        ] {
            assert!(
                !e.stderr.contains(leaked),
                "{source:?} leaked the Rust name {leaked:?}: {}",
                e.stderr
            );
        }
    }
}

/// An unclosed block names the construct and the line that opened it.
///
/// None of the seven hand-written body loops checked for end of file. They
/// stopped only because an unrecognised token became "Unexpected token: Eof",
/// so every one of these reported that against the last line of the file and
/// named nothing at all -- the reader was told where the parser gave up rather
/// than where the mistake was.
#[test]
fn test_unclosed_blocks_name_their_opener() {
    let cases = [
        (
            "PRINT 1\nFOR I = 1 TO 10\nPRINT I\n",
            "FOR is missing its NEXT",
        ),
        (
            "PRINT 1\nSUB Foo\nPRINT 1\n",
            "SUB 'FOO' is missing its END SUB",
        ),
        (
            "FUNCTION Bar(X)\nBar = X\n",
            "FUNCTION 'BAR' is missing its END FUNCTION",
        ),
        ("WHILE X < 3\nX = X + 1\n", "WHILE is missing its WEND"),
        ("DO\nX = X + 1\n", "DO is missing its LOOP"),
        ("IF X = 1 THEN\nPRINT 1\n", "IF is missing its END IF"),
        (
            "SELECT CASE X\nCASE 1\nPRINT 1\n",
            "SELECT CASE is missing its END SELECT",
        ),
    ];
    for (source, expected) in cases {
        expect_rejected(source, expected);
    }
}

/// The opener's line is the one reported, not end of file.
#[test]
fn test_unclosed_block_reports_the_opening_line() {
    let e = compile_only("PRINT 1\nPRINT 2\nFOR I = 1 TO 10\nPRINT I\nPRINT I\n")
        .expect_err("an unclosed FOR must be refused");
    assert!(
        e.contains(":3: error:"),
        "should blame the FOR on line 3, not end of file: {}",
        e.stderr
    );
}

/// A block closed by the wrong terminator says which one it wanted.
#[test]
fn test_mismatched_block_terminator_names_both() {
    expect_rejected(
        "FOR I = 1 TO 3\nPRINT I\nWEND\n",
        "FOR needs NEXT to close it, but WEND came first",
    );
    expect_rejected(
        "WHILE X < 3\nX = X + 1\nNEXT\n",
        "WHILE needs WEND to close it, but NEXT came first",
    );
    expect_rejected(
        "DO\nX = X + 1\nEND SUB\n",
        "DO needs LOOP to close it, but END SUB came first",
    );
}

/// Pathological nesting is diagnosed, not fatal.
///
/// The expression parser recursed without a bound, so 50,000 nested parens --
/// or 50,000 unary minuses, which descend through the same path -- aborted the
/// process with "fatal runtime error: stack overflow" and exit code 134. A
/// compiler may refuse its input; it may not die on it, and 134 is outside the
/// contract `is_clean_rejection` describes.
#[test]
fn test_deeply_nested_expressions_are_diagnosed_not_fatal() {
    let n = 50_000;
    expect_rejected(
        &format!("X = {}1{}\n", "(".repeat(n), ")".repeat(n)),
        "nesting is too deep",
    );
    expect_rejected(&format!("X = {}1\n", "-".repeat(n)), "nesting is too deep");
    expect_rejected(
        &format!("X = {}1\n", "NOT ".repeat(n)),
        "nesting is too deep",
    );
}

/// Deeply nested blocks descend through the same statement path.
#[test]
fn test_deeply_nested_blocks_are_diagnosed_not_fatal() {
    let n = 50_000;
    let source = format!(
        "{}PRINT 1\n{}",
        "IF 1 = 1 THEN\n".repeat(n),
        "END IF\n".repeat(n)
    );
    expect_rejected(&source, "nesting is too deep");
}

/// A long run of statement separators must not consume stack either.
///
/// `parse_statement_kind` recursed once per separator to skip it. That is a
/// tail call, so a release build optimized it away and only a debug build
/// overflowed on 200,000 colons -- which is the worst way to hold a bug, since
/// CI runs `cargo test --release`. Skipping them in a loop costs no stack in
/// any profile. A separator run is legal, so this is accepted, not diagnosed.
#[test]
fn test_a_long_run_of_separators_costs_no_stack() {
    let source = format!("X = 1 {}\nPRINT X\n", ":".repeat(200_000));
    let run = compile_and_run(&source).expect("a run of separators is legal, if pointless");
    assert_eq!(run.trim(), "1");
}

/// A line number too large to represent is an error, not a silent zero.
///
/// The lexer parsed it with `unwrap_or(0)`, so `99999999999 PRINT "hi"`
/// compiled as a definition of label 0 -- and any GOTO written to reach it
/// failed separately, because past LONG range the same digits lex as a Double.
/// Every other numeric form in the lexer already refuses to guess.
#[test]
fn test_line_number_out_of_range_is_diagnosed() {
    expect_rejected("99999999999 PRINT \"hi\"\n", "line number");
    // The largest representable one still works.
    compile_only("4294967295 PRINT \"ok\"\n").expect("u32::MAX is a valid line number");
}

/// A DO loop tests its condition at one end or the other, never both.
///
/// The two conditions used to be merged with `condition.or(end_condition)`, so
/// the one on the LOOP was silently discarded: the loop below ran three times
/// and printed 3, with `UNTIL I > 100` having no effect whatever. Writing both
/// is a mistake about which test is being applied, and saying so beats picking
/// one.
#[test]
fn test_do_loop_rejects_a_condition_at_both_ends() {
    expect_rejected(
        "I = 0\nDO WHILE I < 3\nI = I + 1\nLOOP UNTIL I > 100\n",
        "only one end",
    );
    expect_rejected(
        "I = 0\nDO UNTIL I > 3\nI = I + 1\nLOOP WHILE I < 100\n",
        "only one end",
    );
}

/// Each single-ended form still compiles, so the check above is not simply
/// rejecting every DO loop.
#[test]
fn test_do_loop_single_condition_forms_still_compile() {
    for source in [
        "I = 0\nDO WHILE I < 3\nI = I + 1\nLOOP\n",
        "I = 0\nDO UNTIL I > 3\nI = I + 1\nLOOP\n",
        "I = 0\nDO\nI = I + 1\nLOOP WHILE I < 3\n",
        "I = 0\nDO\nI = I + 1\nLOOP UNTIL I > 3\n",
        "I = 0\nDO\nI = I + 1\nIF I > 3 THEN EXIT DO\nLOOP\n",
    ] {
        compile_only(source).unwrap_or_else(|e| {
            panic!("{:?} should compile, but: {}", source, e.stderr);
        });
    }
}

/// The random-access statement names are recognised only in statement
/// position, so a program may still use them for its own variables and
/// procedures -- which GW-BASIC would not allow, but costs nothing to keep.
#[test]
fn test_random_access_keywords_are_not_reserved() {
    for source in [
        "GET = 5\nPRINT GET\n",
        "PUT = 5\nPRINT PUT\n",
        "FIELD = 5\nPRINT FIELD\n",
        "LSET = 5\nPRINT LSET\n",
        "RANDOM = 5\nPRINT RANDOM\n",
        "PRINT LEN(\"abc\")\n",
    ] {
        compile_only(source).unwrap_or_else(|e| {
            panic!("{:?} should still compile, but: {}", source, e.stderr);
        });
    }
}

/// The front end must never panic, whatever it is fed.
///
/// A compiler may reject its input; it may not die on it. The stack overflow on
/// deeply nested expressions was exactly this class of bug and survived 314
/// tests, because every one of them fed the compiler a program someone had
/// thought about. This feeds it token soup instead: every result is acceptable
/// except a panic or an abort, which `is_clean_rejection` distinguishes by exit
/// code (1 = diagnosed, 101 = Rust panic, 134 = abort).
#[test]
fn test_front_end_never_panics_on_token_soup() {
    // Deterministic, so a failure is reproducible from the seed alone.
    let pieces = [
        "PRINT",
        "IF",
        "THEN",
        "ELSE",
        "END",
        "SUB",
        "FUNCTION",
        "FOR",
        "NEXT",
        "WHILE",
        "WEND",
        "DO",
        "LOOP",
        "UNTIL",
        "SELECT",
        "CASE",
        "DIM",
        "TYPE",
        "AS",
        "GOTO",
        "GOSUB",
        "RETURN",
        "MID$",
        "LEN",
        "(",
        ")",
        ",",
        ";",
        ":",
        "#",
        ".",
        "=",
        "<>",
        "+",
        "-",
        "*",
        "/",
        "^",
        "\"s\"",
        "1",
        "1.5",
        "&HFF",
        "A",
        "B$",
        "C%",
        "\n",
        "REM x",
        "'c",
        "LINE INPUT",
        "SWAP",
        "CONST",
        "EXIT",
        "OPTION",
        "BASE",
        "REDIM",
        "PRESERVE",
        "DATA",
        "READ",
        "RESTORE",
        "OPEN",
        "FIELD",
        "LSET",
        "GET",
        "PUT",
        "LOCK",
        "STEP",
        "TO",
        "NOT",
        "AND",
        "OR",
        "XOR",
        "MOD",
    ];
    // xorshift, so the corpus is fixed without pulling in a rng crate.
    let mut state: u64 = 0x9E3779B97F4A7C15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for case in 0..300 {
        let len = 1 + (next() % 40) as usize;
        let mut source = String::new();
        for _ in 0..len {
            source.push_str(pieces[(next() % pieces.len() as u64) as usize]);
            source.push(' ');
        }
        source.push('\n');

        if let Err(e) = compile_only(&source) {
            assert!(
                e.is_clean_rejection(),
                "case {case} must be diagnosed, not crash; exit={:?}\nsource: {source:?}\nstderr: {}",
                e.exit_code,
                e.stderr
            );
        }
    }
}

/// A number too large to represent is an error, not infinity.
///
/// `parse::<f64>()` returns `inf` for an overflowing literal rather than Err,
/// so the "malformed number" arm never fired and `1e400` compiled to a silent
/// infinity. Every other numeric form in this lexer refuses to guess.
#[test]
fn test_numeric_overflow_is_diagnosed() {
    expect_rejected("PRINT 1e400\n", "too large");
    expect_rejected("X# = 1.5D400\n", "too large");
    // The largest representable double still works.
    compile_only("PRINT 1.7976931348623157E+308\n").expect("near the maximum is fine");
}

/// A failed block header must not also blame its own terminator.
///
/// `FOR I = 1 2 3` fails, and the NEXT that follows is then orphaned -- so the
/// reader was told "NEXT without matching FOR" about a FOR sitting one line
/// above. Once anything has gone wrong, stray terminators say nothing useful.
#[test]
fn test_a_failed_block_header_does_not_cascade() {
    let e = compile_only("FOR I = 1 2 3\nPRINT I\nNEXT\n").expect_err("must be refused");
    assert!(
        e.contains("expected TO"),
        "the real error must survive: {}",
        e.stderr
    );
    assert!(
        !e.contains("without matching"),
        "the orphaned NEXT must not be reported: {}",
        e.stderr
    );
    assert!(e.contains("1 error"), "exactly one: {}", e.stderr);
}

/// A stray terminator in an otherwise clean program is still reported.
#[test]
fn test_cascade_suppression_only_applies_after_an_error() {
    expect_rejected("PRINT 1\nNEXT\n", "NEXT without matching FOR");
}

/// `RETURN` with no `GOSUB` anywhere is a compile error, not a linker error.
///
/// codegen only defines the GOSUB return stack when it has seen a GOSUB, so a
/// lone RETURN emitted a reference to `_gosub_sp` that nothing defined and the
/// user was shown `ld: undefined reference to _gosub_sp`. Turning that into a
/// diagnostic is the whole reason sema exists.
#[test]
fn test_return_without_gosub_is_diagnosed() {
    expect_rejected("PRINT \"x\"\nRETURN\n", "RETURN");
    expect_rejected("IF 1 = 1 THEN\nRETURN\nEND IF\n", "RETURN");
    // A program that does use GOSUB is unaffected.
    compile_only("GOSUB 100\nEND\n100 PRINT 1\nRETURN\n").expect("GOSUB/RETURN pairs compile");
}

/// SWAP's type check must look at what the operands actually are, not at the
/// suffix of the variable they hang off.
///
/// It compared `a.name.ends_with('$')`, which for `P.N` reads *P* -- a record,
/// carrying no suffix. So swapping a string field with a numeric one passed the
/// check, and codegen then read a string into rax/rdx and stored it into an
/// INTEGER slot: SIGSEGV, exit 139.
#[test]
fn test_swap_of_mismatched_record_fields_is_diagnosed() {
    let ty = "TYPE R\n  N AS STRING * 4\n  V AS INTEGER\nEND TYPE\nDIM P AS R\n";
    expect_rejected(&format!("{ty}SWAP P.N, P.V\n"), "same type");
    expect_rejected(&format!("{ty}SWAP P.V, P.N\n"), "same type");
    // Matching fields still swap.
    compile_only("TYPE R\n  A AS INTEGER\n  B AS INTEGER\nEND TYPE\nDIM P AS R\nSWAP P.A, P.B\n")
        .expect("two numeric fields are a legal SWAP");
}

/// Out-of-range arguments to the string builtins are refused.
///
/// Each of these returned something plausible instead. The negative-length
/// cases were the worst: `_rt_left` compares the count against the length
/// unsigned, so -1 read as enormous, clamped to the length, and returned the
/// *whole string*. `MID$`'s negative count is worse still -- it is the
/// compiler's own sentinel for the two-argument form, so a program writing one
/// explicitly got "the rest of the string" from a value GW-BASIC rejects.
#[test]
fn test_string_builtin_arguments_are_range_checked() {
    for (source, what) in [
        ("PRINT LEFT$(\"abc\", -1)\n", "LEFT$ negative count"),
        ("PRINT RIGHT$(\"abc\", -1)\n", "RIGHT$ negative count"),
        ("PRINT MID$(\"abc\", 0, 2)\n", "MID$ zero start"),
        ("PRINT MID$(\"abc\", -1, 2)\n", "MID$ negative start"),
        ("PRINT MID$(\"abc\", 1, -1)\n", "MID$ negative count"),
        ("PRINT ASC(\"\")\n", "ASC of the empty string"),
    ] {
        let run = crate::common::compile_and_run_raw(source, "").expect("should compile");
        assert_eq!(run.exit_code, Some(1), "{what}: stderr={}", run.stderr);
        assert!(
            run.stderr.contains("Illegal function call"),
            "{what}: stderr={}",
            run.stderr
        );
    }
}

/// The legal forms of the same calls keep working, including MID$ with the
/// length omitted -- which is what the negative sentinel exists for.
#[test]
fn test_string_builtin_legal_arguments_still_work() {
    let output = compile_and_run(
        r#"
PRINT LEFT$("abcdef", 3)
PRINT LEFT$("abc", 0)
PRINT LEFT$("abc", 99)
PRINT RIGHT$("abcdef", 2)
PRINT MID$("abcdef", 3)
PRINT MID$("abcdef", 3, 2)
PRINT MID$("abc", 4)
PRINT ASC("A")
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, &["abc", "", "abc", "ef", "cdef", "cd", "", "65"]);
}

/// A negative base with a fractional exponent has no real result. It used to
/// print `-nan`.
#[test]
fn test_fractional_power_of_a_negative_is_refused() {
    let run =
        crate::common::compile_and_run_raw("A = -8\nPRINT A ^ 0.5\n", "").expect("should compile");
    assert_eq!(run.exit_code, Some(1), "stderr: {}", run.stderr);
    assert!(
        run.stderr.contains("Illegal function call"),
        "stderr: {}",
        run.stderr
    );
}

/// Integral exponents of a negative base are fine, and so is everything else
/// that has a real answer.
#[test]
fn test_powers_that_have_real_answers_still_work() {
    let output = compile_and_run(
        r#"
A = -2
PRINT A ^ 3
PRINT A ^ 2
PRINT 4 ^ 0.5
PRINT 2 ^ -2
PRINT 0 ^ 0
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, &["-8", "4", "2", "0.25", "1"]);
}

/// CHR$, SPACE$ and STRING$ refuse counts and codes they cannot represent.
///
/// CHR$ kept only the low byte, so CHR$(256) was CHR$(0) and CHR$(-1) was
/// CHR$(255). SPACE$ and STRING$ clamped a negative count to zero and returned
/// the empty string. GW-BASIC calls all four an illegal function call.
#[test]
fn test_character_and_count_arguments_are_range_checked() {
    for (source, what) in [
        ("PRINT CHR$(256)\n", "CHR$ above 255"),
        ("PRINT CHR$(-1)\n", "CHR$ below 0"),
        ("PRINT SPACE$(-1)\n", "SPACE$ negative"),
        ("PRINT STRING$(-1, \"x\")\n", "STRING$ negative"),
    ] {
        let run = crate::common::compile_and_run_raw(source, "").expect("should compile");
        assert_eq!(run.exit_code, Some(1), "{what}: stderr={}", run.stderr);
        assert!(
            run.stderr.contains("Illegal function call"),
            "{what}: stderr={}",
            run.stderr
        );
    }
}

/// The whole legal range still works, both ends included.
#[test]
fn test_character_and_count_legal_arguments() {
    let output = compile_and_run(
        r#"
PRINT ASC(CHR$(0))
PRINT ASC(CHR$(255))
PRINT ASC(CHR$(65))
PRINT "["; SPACE$(0); "]"
PRINT "["; SPACE$(3); "]"
PRINT "["; STRING$(0, "x"); "]"
PRINT "["; STRING$(3, "x"); "]"
PRINT HEX$(255)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        &["0", "255", "65", "[]", "[   ]", "[]", "[xxx]", "FF"]
    );
}

/// A `DIM ... AS` type must agree with any suffix on the name.
///
/// The identical check has always existed for a FUNCTION's declared result
/// type, and LANGREF states the rule -- but DIM accepted the contradiction
/// silently, leaving no way to tell which of the two the program meant.
#[test]
fn test_dim_suffix_must_agree_with_its_as_clause() {
    expect_rejected("DIM X$ AS INTEGER\n", "different type");
    expect_rejected("DIM X% AS DOUBLE\nX% = 1\n", "different type");
    expect_rejected("DIM A%(3) AS STRING * 4\n", "different type");
    // Agreeing, and unsuffixed, are both fine.
    compile_only("DIM X% AS INTEGER\nDIM Y AS DOUBLE\nDIM S$ AS STRING * 4\nDIM N AS LONG\n")
        .expect("a suffix that agrees, or none at all, is legal");
}

// ---------------------------------------------------------------------------
// Error trapping: ON ERROR GOTO, ERR, ERL
//
// The mechanism is a longjmp in all but name. `_rt_error` never returned, and
// is reached from thirteen places inside each runtime tree with live frames --
// some two deep, some holding a lock structure on the stack. Trapping abandons
// all of them, so what the handler starts with has to be captured up front.

/// The handler runs, and ERR and ERL say what happened and where.
#[test]
fn test_on_error_traps_and_reports() {
    let out = compile_and_run(
        r#"
10 ON ERROR GOTO 100
20 PRINT "before"
30 PRINT 1 / 0
40 PRINT "not reached"
50 END
100 PRINT "handled"; ERR; ERL
110 END
"#,
    )
    .unwrap();
    assert_eq!(
        out.trim().lines().collect::<Vec<_>>(),
        &["before", "handled1130"]
    );
}

/// An error raised from deep inside a runtime helper is trapped too.
///
/// This is the case the whole design is for: `ON ERROR` round an `OPEN` is the
/// commonest vintage idiom, and those errors come from hand-written assembly
/// several frames down, not from a codegen trampoline.
#[test]
fn test_on_error_traps_an_error_raised_inside_a_helper() {
    let out = compile_and_run(
        r#"
10 ON ERROR GOTO 100
20 OPEN "no-such-file-here.txt" FOR INPUT AS #1
30 PRINT "not reached"
40 END
100 PRINT "trapped"; ERR
110 END
"#,
    )
    .unwrap();
    assert_eq!(out.trim(), "trapped53");
}

/// `ERROR n` is trapped like any other error, which is how a program tests its
/// own handler.
#[test]
fn test_on_error_traps_the_error_statement() {
    let out =
        compile_and_run("10 ON ERROR GOTO 100\n20 ERROR 62\n30 END\n100 PRINT ERR\n110 END\n")
            .unwrap();
    assert_eq!(out.trim(), "62");
}

/// `ON ERROR GOTO 0` puts the fatal path back.
#[test]
fn test_on_error_goto_zero_disarms() {
    let run = compile_and_run_raw(
        "10 ON ERROR GOTO 100\n20 ON ERROR GOTO 0\n30 PRINT 1 / 0\n40 END\n100 PRINT \"no\"\n110 END\n",
        "",
    )
    .expect("should compile");
    assert_eq!(run.exit_code, Some(1), "disarmed, so the error is fatal");
    assert!(run.stderr.contains("Division by zero"), "{:?}", run.stderr);
    assert!(!run.stdout.contains("no"), "the handler must not run");
}

/// An error inside the handler is fatal rather than looping through it.
#[test]
fn test_error_inside_the_handler_is_fatal() {
    let run = compile_and_run_raw(
        "10 ON ERROR GOTO 100\n20 PRINT 1 / 0\n30 END\n100 PRINT \"in handler\"\n110 PRINT 1 / 0\n120 END\n",
        "",
    )
    .expect("should compile");
    assert_eq!(run.exit_code, Some(1));
    assert!(
        run.stdout.contains("in handler"),
        "the handler did run once"
    );
    assert!(run.stderr.contains("Division by zero"));
}

/// The handler can carry on with the program, and the variables it reads are
/// the ones the program actually wrote.
///
/// The unwind abandons every intervening frame, so anything the compiler was
/// holding in a register has to be back in memory by then.
#[test]
fn test_state_survives_the_unwind() {
    let out = compile_and_run(
        r#"
10 ON ERROR GOTO 200
20 T = 0
30 FOR I = 1 TO 5
40   T = T + I
50 NEXT I
60 PRINT 1 / 0
70 END
200 PRINT "T="; T; "I="; I
210 END
"#,
    )
    .unwrap();
    assert_eq!(out.trim(), "T=15I=6", "the loop's own variables are intact");
}

/// An error inside a SUB unwinds to the module-level handler.
#[test]
fn test_error_inside_a_procedure_is_trapped() {
    let out = compile_and_run(
        r#"
10 ON ERROR GOTO 100
20 CALL Boom
30 PRINT "not reached"
40 END
100 PRINT "trapped"; ERR
110 END
SUB Boom
  PRINT 1 / 0
END SUB
"#,
    )
    .unwrap();
    assert_eq!(out.trim(), "trapped11");
}

/// A program that never traps carries none of the machinery.
#[test]
fn test_no_cost_without_on_error() {
    let asm = crate::common::compile_to_asm("PRINT 1\nFOR I = 1 TO 3\nPRINT I\nNEXT I\n").unwrap();
    for symbol in ["_err_handler", "_err_ctx", "_err_line", "_rt_trap_capture"] {
        assert!(
            !asm.contains(symbol),
            "{symbol} should not appear in a program with no ON ERROR"
        );
    }
}

/// `--unsafe` removes the checks a handler exists to catch, so the pair is
/// refused rather than leaving a handler that looks right and never runs.
#[test]
fn test_on_error_with_unsafe_is_refused() {
    let err = crate::common::compile_only_flags(
        "10 ON ERROR GOTO 100\n20 PRINT 1 / 0\n30 END\n100 END\n",
        &["--unsafe"],
    )
    .expect_err("ON ERROR plus --unsafe must be refused");
    assert!(
        err.contains("--unsafe"),
        "expected the reason, got: {}",
        err.stderr
    );
    assert!(err.is_clean_rejection());
}

/// The handler must be module-level code, not a line inside a procedure.
#[test]
fn test_on_error_scope_is_checked() {
    let err = compile_only("SUB S\n10 ON ERROR GOTO 20\n20 END\nEND SUB\nCALL S\n")
        .expect_err("ON ERROR inside a procedure must be refused");
    assert!(err.is_clean_rejection(), "got: {}", err.stderr);

    let err = compile_only("10 ON ERROR GOTO 900\n20 END\nSUB S\n900 PRINT 1\nEND SUB\n")
        .expect_err("a handler inside a procedure must be refused");
    assert!(err.is_clean_rejection(), "got: {}", err.stderr);
}

/// A GOSUB interrupted by a trapped error still returns correctly.
///
/// The GOSUB stack is a separate software stack, independent of `rsp`, so the
/// unwind does not disturb it -- deliberately, because that is what lets a
/// handler carry on from a subroutine the error interrupted, as GW-BASIC does.
#[test]
fn test_gosub_survives_a_trap() {
    let out = compile_and_run(
        r#"
10 ON ERROR GOTO 200
20 GOSUB 100
30 PRINT "back"
40 END
100 PRINT "in sub"
110 PRINT 1 / 0
120 RETURN
200 PRINT "trapped"
210 RETURN
"#,
    )
    .unwrap();
    assert_eq!(
        out.trim().lines().collect::<Vec<_>>(),
        &["in sub", "trapped", "back"],
        "the handler's RETURN goes back to the statement after the GOSUB"
    );
}

/// ...but a GOSUB inside a procedure cannot, so it is refused.
///
/// Its return address is a label in a frame the unwind discards, so a later
/// RETURN would jump into dead code with main's frame pointer.
#[test]
fn test_gosub_inside_a_procedure_is_refused_when_trapping() {
    let src = "10 ON ERROR GOTO 100\n20 CALL S\n30 END\n100 END\n\
               SUB S\n  GOSUB 500\n  EXIT SUB\n500 PRINT 1\n  RETURN\nEND SUB\n";
    let err = compile_only(src).expect_err("GOSUB in a procedure must be refused when trapping");
    assert!(err.contains("GOSUB inside"), "got: {}", err.stderr);
    assert!(err.is_clean_rejection());

    // The same program without ON ERROR is fine: nothing unwinds.
    let ok = "10 CALL S\n20 END\nSUB S\n  GOSUB 500\n  EXIT SUB\n500 PRINT 1\n  RETURN\nEND SUB\n";
    compile_only(ok).expect("a GOSUB in a procedure is fine when nothing traps");
}

/// A trapping program keeps its loop variables in memory.
///
/// FOR-loop register promotion is switched off when a program traps, and that
/// is load-bearing rather than a concession: a promoted counter's register is
/// gone after the unwind and its memory copy is written back only at the
/// loop's exit label, so a handler would read a stale value. The promotion's
/// stack saves are also the one thing that moves `rsp` across a statement
/// boundary, which is what lets the trap restore a single captured `rsp`.
#[test]
fn test_trapping_disables_register_promotion() {
    let hot = "FOR I% = 1 TO 10\nS% = S% + I%\nNEXT I%\nPRINT S%\n";
    let plain = crate::common::compile_to_asm(hot).unwrap();
    assert!(
        plain.contains("save a counter register"),
        "the loop should promote when nothing traps"
    );

    let trapping =
        crate::common::compile_to_asm(&format!("10 ON ERROR GOTO 100\n{hot}90 END\n100 END\n"))
            .unwrap();
    assert!(
        !trapping.contains("save a counter register"),
        "a trapping program must keep its counter in memory"
    );
}
