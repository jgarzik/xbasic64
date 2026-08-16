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
        ("PRINT DATE$\n", "DATE$"),
        ("PRINT TIME$\n", "TIME$"),
        ("A$ = INKEY$\n", "INKEY$"),
        ("PRINT ERR\n", "ERR"),
        ("PRINT ERL\n", "ERL"),
        ("PRINT CSRLIN\n", "CSRLIN"),
        ("PRINT FRE(0)\n", "FRE"),
        ("A$ = INPUT$(3)\n", "INPUT$"),
        ("LOCATE 1, 1\n", "LOCATE"),
        ("COLOR 7\n", "COLOR"),
        ("RANDOMIZE 5\n", "RANDOMIZE"),
        ("DEFINT A-Z\n", "DEFINT"),
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

/// `ON ERROR GOTO` is the dangerous one: it parses as a computed GOTO on a
/// variable named ERROR, which is always zero, so the handler never runs and
/// nothing says so.
#[test]
fn test_on_error_goto_is_refused() {
    let err = compile_only("ON ERROR GOTO 100\nPRINT \"x\"\nEND\n100 END\n")
        .expect_err("ON ERROR GOTO must be refused rather than falling through");
    assert!(
        err.contains("not supported"),
        "expected an explanation, got: {}",
        err.stderr
    );
    assert!(err.is_clean_rejection());
}

/// Assigning to one of these names is a GW-BASIC statement, not the creation
/// of a variable that happens to be called DATE$.
#[test]
fn test_assignment_to_an_unimplemented_name_is_refused() {
    let err = compile_only("DATE$ = \"01-01-2026\"\n").expect_err("DATE$ = ... must be refused");
    assert!(err.contains("not supported"), "got: {}", err.stderr);
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

    let err = compile_only("RANDOMIZE 5\n").expect_err("RANDOMIZE must be refused");
    assert!(
        err.contains("not implemented yet"),
        "a planned feature should say so: {}",
        err.stderr
    );
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
