//! Control flow tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_for_loops() {
    // Test FOR loop, STEP positive, STEP negative
    let output = compile_and_run(
        r#"
FOR I = 1 TO 3: PRINT I: NEXT I
FOR I = 0 TO 6 STEP 2: PRINT I: NEXT I
FOR I = 3 TO 1 STEP -1: PRINT I: NEXT I
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(&lines[0..3], &["1", "2", "3"], "for basic");
    assert_eq!(&lines[3..7], &["0", "2", "4", "6"], "for step+");
    assert_eq!(&lines[7..10], &["3", "2", "1"], "for step-");
}

#[test]
fn test_while_loop() {
    let output = compile_and_run(
        r#"
X = 1
WHILE X <= 3
    PRINT X
    X = X + 1
WEND
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1", "2", "3"]);
}

#[test]
fn test_do_loops() {
    // Test DO WHILE, DO UNTIL, DO...LOOP WHILE
    let output = compile_and_run(
        r#"
X = 1
DO WHILE X <= 3
    PRINT X
    X = X + 1
LOOP
X = 1
DO UNTIL X > 3
    PRINT X
    X = X + 1
LOOP
X = 1
DO
    PRINT X
    X = X + 1
LOOP WHILE X <= 3
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(&lines[0..3], &["1", "2", "3"], "do while");
    assert_eq!(&lines[3..6], &["1", "2", "3"], "do until");
    assert_eq!(&lines[6..9], &["1", "2", "3"], "do...loop while");
}

#[test]
fn test_if_statements() {
    // Test IF/THEN/ELSE and ELSEIF
    let output = compile_and_run(
        r#"
X = 10
IF X > 5 THEN
    PRINT "big"
ELSE
    PRINT "small"
END IF
X = 3
IF X > 5 THEN
    PRINT "big"
ELSE
    PRINT "small"
END IF
X = 2
IF X = 1 THEN
    PRINT "one"
ELSEIF X = 2 THEN
    PRINT "two"
ELSEIF X = 3 THEN
    PRINT "three"
ELSE
    PRINT "other"
END IF
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "big", "if true");
    assert_eq!(lines[1], "small", "if false");
    assert_eq!(lines[2], "two", "elseif");
}

#[test]
fn test_goto_gosub() {
    // Test GOTO, GOSUB/RETURN, ON GOTO
    let output = compile_and_run(
        r#"
10 PRINT "A"
20 GOTO 40
30 PRINT "B"
40 PRINT "C"
50 GOSUB 100
60 PRINT "end"
70 X = 2
80 ON X GOTO 200, 300, 400
90 PRINT "none"
95 END
100 PRINT "in sub"
110 RETURN
200 PRINT "first"
210 END
300 PRINT "second"
310 END
400 PRINT "third"
410 END
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "A", "before goto");
    assert_eq!(lines[1], "C", "after goto");
    assert_eq!(lines[2], "in sub", "gosub");
    assert_eq!(lines[3], "end", "after return");
    assert_eq!(lines[4], "second", "on goto");
}

#[test]
fn test_select_case() {
    // Test SELECT CASE and CASE ELSE
    let output = compile_and_run(
        r#"
X = 2
SELECT CASE X
    CASE 1
        PRINT "one"
    CASE 2
        PRINT "two"
    CASE 3
        PRINT "three"
    CASE ELSE
        PRINT "other"
END SELECT
X = 99
SELECT CASE X
    CASE 1
        PRINT "one"
    CASE 2
        PRINT "two"
    CASE ELSE
        PRINT "other"
END SELECT
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "two", "case match");
    assert_eq!(lines[1], "other", "case else");
}

#[test]
fn test_end_stop() {
    // Test END and STOP statements
    let output = compile_and_run(
        r#"
PRINT "before"
END
PRINT "after"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "before", "end");

    let output2 = compile_and_run(
        r#"
PRINT "before"
STOP
PRINT "after"
"#,
    )
    .unwrap();
    assert_eq!(output2.trim(), "before", "stop");
}

#[test]
fn test_gosub_stress() {
    // Test GOSUB with many calls and nested calls
    let output = compile_and_run(
        r#"
X = 0
FOR I = 1 TO 500
    GOSUB 100
NEXT I
PRINT X
GOSUB 200
PRINT "done"
END

100 X = X + 1
RETURN

200 PRINT "L1 start"
GOSUB 300
PRINT "L1 end"
RETURN

300 PRINT "L2 start"
GOSUB 400
PRINT "L2 end"
RETURN

400 PRINT "L3"
RETURN
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "500", "many gosub");
    assert_eq!(
        &lines[1..7],
        &["L1 start", "L2 start", "L3", "L2 end", "L1 end", "done"],
        "nested gosub"
    );
}

/// END and STOP must terminate the program from any frame. Inside a SUB they
/// used to emit a bare `leave; ret`, which merely returned to the caller and
/// let execution continue.
#[test]
fn test_end_terminates_from_within_procedure() {
    let output = compile_and_run(
        r#"
SUB Quit
PRINT "before"
END
PRINT "after"
END SUB
PRINT "start"
Quit
PRINT "unreachable"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["start", "before"], "END ends the program");
}

/// Same for STOP, and from inside a FUNCTION.
#[test]
fn test_stop_terminates_from_within_function() {
    let output = compile_and_run(
        r#"
FUNCTION Half(N)
PRINT "computing"
STOP
Half = N / 2
END FUNCTION
PRINT "start"
PRINT Half(8)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["start", "computing"], "STOP ends the program");
}

/// LANGREF documents GOTO/GOSUB targets as "line number or label", but named
/// labels were never implemented: `GOTO Finish` emitted a jump to `_label_FINISH`
/// that nothing defined, and a bare `Finish:` was parsed as a procedure call, so
/// both failed at link time.
#[test]
fn test_named_labels() {
    let output = compile_and_run(
        r#"
GOTO Finish
PRINT "skipped"
Finish:
PRINT "done"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "done");
}

/// GOSUB to a named label, and RETURN back.
#[test]
fn test_gosub_named_label() {
    let output = compile_and_run(
        r#"
PRINT "start"
GOSUB Helper
PRINT "back"
END
Helper:
PRINT "in helper"
RETURN
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["start", "in helper", "back"]);
}

/// `Name:` is only a label at the start of a line, and never when `Name` is a
/// declared procedure -- otherwise calling a parameterless SUB as the first
/// statement of a multi-statement line would be misread as a label.
#[test]
fn test_procedure_call_is_not_mistaken_for_label() {
    let output = compile_and_run(
        r#"
SUB Greet
PRINT "called"
END SUB
Greet : PRINT "after"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["called", "after"]);
}

/// SWAP exchanges two values. Both are read before either is written, so
/// swapping two elements of the same array is correct even when the subscripts
/// alias.
#[test]
fn test_swap() {
    let output = compile_and_run(
        "A = 1\nB = 2\nSWAP A, B\nPRINT A; B\nX$ = \"x\"\nY$ = \"yy\"\nSWAP X$, Y$\nPRINT X$; Y$\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["21", "yyx"]);
}

/// SWAP in anger: a sort that exchanges array elements.
#[test]
fn test_swap_array_elements() {
    let output = compile_and_run(
        "DIM A(4)\nA(0)=5\nA(1)=3\nA(2)=4\nA(3)=1\nA(4)=2\nFOR I = 0 TO 3\nFOR J = 0 TO 3 - I\nIF A(J) > A(J+1) THEN SWAP A(J), A(J+1)\nNEXT J\nNEXT I\nFOR I = 0 TO 4\nPRINT A(I);\nNEXT I\nPRINT \"\"\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "12345");
}

/// CONST is folded at compile time and substituted wherever the name is used,
/// including as an array bound.
#[test]
fn test_const() {
    let output = compile_and_run(
        "CONST MAX = 10\nCONST HALF = MAX / 2\nCONST NAME$ = \"hi\"\nPRINT MAX\nPRINT HALF\nPRINT NAME$\nDIM A(MAX)\nA(MAX) = 7\nPRINT A(10)\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["10", "5", "hi", "7"]);
}

/// WRITE separates values with commas and quotes strings.
#[test]
fn test_write() {
    let output = compile_and_run("WRITE \"a\", 1, \"b\"\nWRITE 1, 2.5\n").unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["\"a\",1,\"b\"", "1,2.5"]);
}

/// LBOUND and UBOUND report an array's declared bounds.
#[test]
fn test_lbound_ubound() {
    let output = compile_and_run(
        "DIM A(5)\nDIM M(3,7)\nPRINT LBOUND(A); UBOUND(A)\nPRINT UBOUND(M, 1); UBOUND(M, 2)\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["05", "37"]);
}

/// EXIT leaves the innermost matching loop, or returns from a procedure.
#[test]
fn test_exit_statements() {
    let output = compile_and_run(
        "FOR I = 1 TO 10\nIF I = 3 THEN EXIT FOR\nNEXT I\nPRINT I\nJ = 0\nDO\nJ = J + 1\nIF J = 4 THEN EXIT DO\nLOOP\nPRINT J\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["3", "4"]);
}

/// EXIT SUB returns early without running the rest of the procedure.
#[test]
fn test_exit_sub() {
    let output = compile_and_run(
        "SUB T(N)\nIF N = 0 THEN EXIT SUB\nPRINT N\nEND SUB\nT(0)\nT(5)\nPRINT \"done\"\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["5", "done"], "the N=0 call printed nothing");
}

/// EXIT FOR only leaves a FOR loop, and the nesting is respected.
#[test]
fn test_exit_leaves_innermost_matching_loop() {
    let output = compile_and_run(
        "FOR I = 1 TO 2\nFOR J = 1 TO 10\nIF J = 2 THEN EXIT FOR\nNEXT J\nPRINT I; J\nNEXT I\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        vec!["12", "22"],
        "inner loop exited, outer continued"
    );
}
