//! Control flow tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::{compile_and_run, compile_only};

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

/// A FOR control variable of any type must actually count.
///
/// The loop machinery used to store and increment the control variable as a
/// raw Double whatever its declared type was, while every read of it went
/// through that type's own load -- so `movsx eax, WORD PTR` on the bit pattern
/// of 1.0 gave 0, and `FOR I% = 1 TO 3` printed 0 three times. Nothing in the
/// suite used a suffixed loop variable, so all 253 tests stayed green.
#[test]
fn test_for_loop_control_variable_types() {
    let output = compile_and_run(
        r#"
FOR A% = 1 TO 3: PRINT A%: NEXT A%
FOR B& = 1 TO 3: PRINT B&: NEXT B&
FOR C! = 1 TO 3: PRINT C!: NEXT C!
FOR D# = 1 TO 3: PRINT D#: NEXT D#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(&lines[0..3], &["1", "2", "3"], "INTEGER counter");
    assert_eq!(&lines[3..6], &["1", "2", "3"], "LONG counter");
    assert_eq!(&lines[6..9], &["1", "2", "3"], "SINGLE counter");
    assert_eq!(&lines[9..12], &["1", "2", "3"], "DOUBLE counter");
}

/// The same for a control variable whose type came from `AS`, which lives in
/// different storage again.
#[test]
fn test_for_loop_control_variable_declared_as() {
    let output = compile_and_run(
        r#"
DIM E AS INTEGER
DIM F AS LONG
FOR E = 1 TO 3: PRINT E: NEXT E
FOR F = 5 TO 7: PRINT F: NEXT F
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(&lines[0..3], &["1", "2", "3"], "AS INTEGER counter");
    assert_eq!(&lines[3..6], &["5", "6", "7"], "AS LONG counter");
}

/// A typed counter must still step, count down, and survive its bounds being
/// expressions rather than literals.
#[test]
fn test_for_loop_typed_counter_steps_and_bounds() {
    let output = compile_and_run(
        r#"
FOR I% = 0 TO 6 STEP 2: PRINT I%: NEXT I%
FOR I% = 3 TO 1 STEP -1: PRINT I%: NEXT I%
N% = 3
FOR I% = 1 TO N% * 2 STEP N%: PRINT I%: NEXT I%
FOR I! = 0 TO 1 STEP 0.5: PRINT I!: NEXT I!
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(&lines[0..4], &["0", "2", "4", "6"], "INTEGER step +2");
    assert_eq!(&lines[4..7], &["3", "2", "1"], "INTEGER step -1");
    assert_eq!(&lines[7..9], &["1", "4"], "INTEGER computed bounds");
    assert_eq!(&lines[9..12], &["0", "0.5", "1"], "SINGLE fractional step");
}

/// A step that is only known at run time still picks the right direction.
///
/// This is the one shape that has to test the step's sign on every iteration,
/// because the compiler cannot know it; both signs must work, and a loop whose
/// step runs away from its limit must not execute at all.
#[test]
fn test_for_loop_runtime_step_direction() {
    let output = compile_and_run(
        r#"
S% = 2
FOR I% = 0 TO 6 STEP S%: PRINT I%: NEXT I%
S% = -1
FOR I% = 3 TO 1 STEP S%: PRINT I%: NEXT I%
S% = -1
FOR I% = 1 TO 3 STEP S%: PRINT "never": NEXT I%
D# = 0.5
FOR X# = 0 TO 1 STEP D#: PRINT X#: NEXT X#
PRINT "done"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(&lines[0..4], &["0", "2", "4", "6"], "runtime step +2");
    assert_eq!(&lines[4..7], &["3", "2", "1"], "runtime step -1");
    assert_eq!(&lines[7..10], &["0", "0.5", "1"], "runtime DOUBLE step");
    assert_eq!(lines[10], "done", "a backwards step ran zero times");
}

/// The control variable keeps its final value after the loop, as GW-BASIC
/// leaves it: one step past the limit.
#[test]
fn test_for_loop_typed_counter_survives_the_loop() {
    let output = compile_and_run(
        r#"
FOR I% = 1 TO 3: NEXT I%
PRINT I%
FOR J% = 1 TO 5
  IF J% = 3 THEN EXIT FOR
NEXT J%
PRINT J%
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "4", "value after a completed loop");
    assert_eq!(lines[1], "3", "value after EXIT FOR");
}

/// A typed counter indexes an array correctly -- the shape that first exposed
/// this, since a wrong counter silently reads element 0 every time.
#[test]
fn test_for_loop_typed_counter_indexes_arrays() {
    let output = compile_and_run(
        r#"
DIM A%(10)
T% = 0
FOR I% = 0 TO 9
  A%(I%) = I% * 2 + 1
NEXT I%
FOR I% = 0 TO 9
  T% = T% + A%(I%)
NEXT I%
PRINT T%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "100", "sum of the first ten odd numbers");
}

/// Nested loops with typed counters must not share state.
#[test]
fn test_for_loop_typed_counters_nest() {
    let output = compile_and_run(
        r#"
T% = 0
FOR I% = 1 TO 3
  FOR J% = 1 TO 4
    T% = T% + I% * J%
  NEXT J%
NEXT I%
PRINT T%
PRINT I%
PRINT J%
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "60", "(1+2+3) * (1+2+3+4)");
    assert_eq!(lines[1], "4", "outer counter after the loop");
    assert_eq!(lines[2], "5", "inner counter after the loop");
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

/// SWAP must follow a record field path, not just a variable name.
///
/// Reading a target dropped its field path entirely, so both sides read the
/// record's base word and the exchange wrote zeros back.
#[test]
fn test_swap_record_fields() {
    let output = compile_and_run(
        "TYPE P\nX AS INTEGER\nY AS INTEGER\nEND TYPE\nDIM A AS P\nA.X = 7\nA.Y = 3\nSWAP A.X, A.Y\nPRINT A.X; A.Y\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "37");
}

/// The same for a string field, whose type comes from the field rather than
/// from the record variable's (suffix-less) name.
#[test]
fn test_swap_record_string_fields() {
    let output = compile_and_run(
        "TYPE P\nN AS STRING * 8\nEND TYPE\nDIM A AS P\nDIM B AS P\nA.N = \"aa\"\nB.N = \"bb\"\nSWAP A.N, B.N\nPRINT A.N; \" \"; B.N\n",
    )
    .unwrap();
    // Both fields are `STRING * 8`, so each is padded to eight characters;
    // `output.trim()` removes only the trailing pad of the second.
    assert_eq!(output.trim(), "bb       aa");
}

/// And for a field of an array element, whose address is only known at run
/// time.
#[test]
fn test_swap_array_record_fields() {
    let output = compile_and_run(
        "TYPE P\nX AS INTEGER\nEND TYPE\nDIM A(3) AS P\nA(0).X = 1\nA(1).X = 2\nSWAP A(0).X, A(1).X\nPRINT A(0).X; A(1).X\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "21");
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

/// LANGREF documents four CASE forms; only single values worked. Ranges are
/// inclusive at both ends.
#[test]
fn test_case_ranges() {
    let output = compile_and_run(
        "FOR G = 95 TO 65 STEP -10\nSELECT CASE G\nCASE 90 TO 100\nPRINT \"A\";\nCASE 80 TO 89\nPRINT \"B\";\nCASE 70 TO 79\nPRINT \"C\";\nCASE ELSE\nPRINT \"F\";\nEND SELECT\nNEXT G\nPRINT \"\"\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "ABCF");
}

/// A CASE may list several alternatives, and mix values with ranges.
#[test]
fn test_case_lists() {
    let output = compile_and_run(
        "FOR G = 1 TO 5\nSELECT CASE G\nCASE 1, 2\nPRINT \"low\";\nCASE 3, 4\nPRINT \"mid\";\nCASE ELSE\nPRINT \"hi\";\nEND SELECT\nNEXT G\nPRINT \"\"\nG = 7\nSELECT CASE G\nCASE 1, 5 TO 9, 20\nPRINT \"in\"\nCASE ELSE\nPRINT \"out\"\nEND SELECT\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["lowlowmidmidhi", "in"]);
}

/// CASE IS compares the selector rather than matching it.
#[test]
fn test_case_is_comparison() {
    let output = compile_and_run(
        "FOR G = 1 TO 3\nSELECT CASE G\nCASE IS > 2\nPRINT \"big\";\nCASE IS < 2\nPRINT \"small\";\nCASE ELSE\nPRINT \"two\";\nEND SELECT\nNEXT G\nPRINT \"\"\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "smalltwobig");
}

/// A string selector, which used to abort the compiler.
#[test]
fn test_case_on_strings() {
    let output = compile_and_run(
        "S$ = \"b\"\nSELECT CASE S$\nCASE \"a\"\nPRINT \"is a\"\nCASE \"b\"\nPRINT \"is b\"\nEND SELECT\nT$ = \"cat\"\nSELECT CASE T$\nCASE \"a\" TO \"m\"\nPRINT \"first half\"\nCASE ELSE\nPRINT \"second half\"\nEND SELECT\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["is b", "first half"]);
}

// ---------------------------------------------------------------------------
// ON ... GOSUB
//
// The property that separates it from ON ... GOTO is that control comes back,
// and comes back to the same place whichever subroutine ran. The property that
// separates it from GOSUB is that a selector matching nothing must leave the
// return stack exactly as it found it.

/// The selector picks the nth subroutine, and RETURN resumes after the
/// statement rather than at the target's caller.
#[test]
fn test_on_gosub_dispatches_and_returns() {
    let output = compile_and_run(
        r#"
FOR I = 1 TO 3
    Hit$ = "none"
    ON I GOSUB One, Two, Three
    PRINT Hit$; "/back"
NEXT I
END
One:
Hit$ = "one"
RETURN
Two:
Hit$ = "two"
RETURN
Three:
Hit$ = "three"
RETURN
"#,
    )
    .unwrap();
    assert_eq!(
        output.trim().lines().collect::<Vec<_>>(),
        vec!["one/back", "two/back", "three/back"]
    );
}

/// A selector outside the list runs nothing and continues, as in GW-BASIC --
/// zero, past the end, and negative alike.
#[test]
fn test_on_gosub_out_of_range_falls_through() {
    let output = compile_and_run(
        r#"
Hit$ = "none"
ON 0 GOSUB Only
PRINT Hit$
ON 5 GOSUB Only
PRINT Hit$
ON -3 GOSUB Only
PRINT Hit$
ON 1 GOSUB Only
PRINT Hit$
END
Only:
Hit$ = "ran"
RETURN
"#,
    )
    .unwrap();
    assert_eq!(
        output.trim().lines().collect::<Vec<_>>(),
        vec!["none", "none", "none", "ran"]
    );
}

/// The return stack is left balanced, so an ordinary GOSUB still works after
/// an ON ... GOSUB -- including after one that matched nothing.
#[test]
fn test_on_gosub_leaves_the_return_stack_balanced() {
    let output = compile_and_run(
        r#"
Count = 0
ON 1 GOSUB Bump
ON 9 GOSUB Bump
GOSUB Bump
ON 0 GOSUB Bump
GOSUB Bump
PRINT Count
END
Bump:
Count = Count + 1
RETURN
"#,
    )
    .unwrap();
    assert_eq!(
        output.trim(),
        "3",
        "only the three that should have run, ran"
    );
}

/// A fall-through must not push a return address it never pops. The GOSUB
/// stack holds 64K entries, so 200000 unmatched dispatches would overflow it
/// if even one address leaked per statement.
#[test]
fn test_on_gosub_fallthrough_does_not_leak_return_addresses() {
    let output = compile_and_run(
        r#"
FOR I = 1 TO 100000
    ON 0 GOSUB Never
    ON 7 GOSUB Never
NEXT I
GOSUB Never
PRINT Hit$
END
Never:
Hit$ = "survived"
RETURN
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "survived");
}

/// The selector is an expression, not just a variable, and is truncated to an
/// integer the way ON ... GOTO's is.
#[test]
fn test_on_gosub_selector_is_an_expression() {
    let output = compile_and_run(
        r#"
X = 4
ON X / 2 GOSUB One, Two
PRINT Hit$
ON 1.9 GOSUB One, Two
PRINT Hit$
END
One:
Hit$ = "one"
RETURN
Two:
Hit$ = "two"
RETURN
"#,
    )
    .unwrap();
    assert_eq!(
        output.trim().lines().collect::<Vec<_>>(),
        vec!["two", "one"],
        "4/2 selects the second; 1.9 truncates to 1"
    );
}

/// Targets may be line numbers as well as named labels, and the two may be
/// mixed in one statement.
#[test]
fn test_on_gosub_targets_may_be_line_numbers() {
    let output = compile_and_run(
        r#"
ON 1 GOSUB 100, Named
PRINT Hit$
ON 2 GOSUB 100, Named
PRINT Hit$
END
100 Hit$ = "line"
110 RETURN
Named:
Hit$ = "label"
RETURN
"#,
    )
    .unwrap();
    assert_eq!(
        output.trim().lines().collect::<Vec<_>>(),
        vec!["line", "label"]
    );
}

/// A subroutine reached by ON ... GOSUB may itself GOSUB, so the two share one
/// return stack correctly.
#[test]
fn test_on_gosub_nests() {
    let output = compile_and_run(
        r#"
ON 1 GOSUB Outer
PRINT Trace$
END
Outer:
Trace$ = Trace$ + "outer("
GOSUB Inner
Trace$ = Trace$ + ")"
RETURN
Inner:
Trace$ = Trace$ + "inner"
RETURN
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "outer(inner)");
}

/// An unknown target is refused, naming the statement, rather than reaching
/// the assembler as a missing label.
#[test]
fn test_on_gosub_unknown_target_is_diagnosed() {
    let err = crate::common::compile_only("ON 1 GOSUB NoSuchLabel\n")
        .expect_err("an undefined target must be refused");
    assert!(
        err.contains("ON ... GOSUB"),
        "the diagnostic should name the statement: {}",
        err.stderr
    );
    assert!(err.is_clean_rejection());
}

/// `ON` must be followed by one of the two keywords, and says so.
#[test]
fn test_on_without_goto_or_gosub_is_diagnosed() {
    let err = crate::common::compile_only("ON 1 PRINT 2\n")
        .expect_err("ON with neither GOTO nor GOSUB must be refused");
    assert!(
        err.contains("GOTO or GOSUB"),
        "the diagnostic should name both: {}",
        err.stderr
    );
}

/// Every colon-separated statement after `THEN` belongs to the THEN branch.
///
/// The parser used to take exactly one statement, so the rest of the line
/// escaped the conditional and ran unconditionally: with `X = 0`, the program
/// below printed `B`. Nothing in the suite used the form, so it stayed green.
#[test]
fn test_single_line_if_takes_every_statement_after_then() {
    let output = compile_and_run(
        r#"
X = 0
IF X = 1 THEN PRINT "A" : PRINT "B"
PRINT "done"
X = 1
IF X = 1 THEN PRINT "C" : PRINT "D"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, &["done", "C", "D"], "the whole tail is conditional");
}

/// The same, inside a loop, where the leak was loudest.
///
/// With the trailing statement unconditional this printed `x two x x` across
/// three iterations instead of `two x` on the second alone.
#[test]
fn test_single_line_if_inside_a_loop() {
    let output = compile_and_run(
        r#"
FOR I = 1 TO 3
IF I = 2 THEN PRINT "two" : PRINT "x"
NEXT I
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, &["two", "x"], "only the matching iteration prints");
}

/// A bare line number after THEN or ELSE is an implied GOTO.
///
/// This is how GW-BASIC spells the commonest branch of all, and the form
/// appears throughout published listings. It was rejected outright with
/// "Unexpected token: Integer(30)", since a statement cannot otherwise begin
/// with a number.
#[test]
fn test_if_then_line_number_is_an_implied_goto() {
    let output = compile_and_run(
        r#"
10 IF 1 = 1 THEN 30
20 PRINT "skipped"
30 PRINT "target"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "target", "THEN <linenum> branches");
}

/// The same after ELSE, and mixed with an ordinary statement.
#[test]
fn test_if_then_else_line_numbers() {
    let output = compile_and_run(
        r#"
10 X = 0
20 IF X = 1 THEN 40 ELSE 60
40 PRINT "then"
50 GOTO 70
60 PRINT "else"
70 IF X = 0 THEN 90 ELSE PRINT "no"
80 PRINT "unreachable"
90 PRINT "done"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        &["else", "done"],
        "both branches accept a line number"
    );
}

/// `ELSE` ends the THEN branch and opens its own colon-separated list.
///
/// This form did not merely misbehave, it failed to compile: the second
/// statement became a sibling of the IF, so the `ELSE` that followed it
/// reached the top level and was rejected as "ELSE without matching IF".
#[test]
fn test_single_line_if_else_both_take_statement_lists() {
    let output = compile_and_run(
        r#"
X = 9
IF X = 1 THEN PRINT "a" : PRINT "b" ELSE PRINT "c" : PRINT "d"
PRINT "end"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, &["c", "d", "end"], "the ELSE branch takes the tail");
}

/// `NEXT` names the loop it closes, and the name is checked.
///
/// The control variable was parsed and thrown away, so crossed NEXTs compiled
/// into a loop nesting nobody wrote:
///
///     FOR I = 1 TO 2
///       FOR J = 1 TO 2
///       NEXT I          ' actually closed the J loop
///     NEXT J            ' actually closed the I loop
///
/// GW-BASIC rejects that as "NEXT without FOR".
#[test]
fn test_next_variable_must_match_its_for() {
    for source in [
        "FOR I = 1 TO 2\nPRINT I\nNEXT J\n",
        "FOR I = 1 TO 2\nFOR J = 1 TO 2\nPRINT I\nNEXT I\nNEXT J\n",
    ] {
        let e = crate::common::compile_only(source)
            .expect_err(&format!("{source:?} should be refused"));
        assert!(
            e.contains("NEXT"),
            "the diagnostic should name NEXT: {}",
            e.stderr
        );
        assert!(e.is_clean_rejection());
    }
}

/// A bare `NEXT` still closes the innermost loop, and a matching name is fine.
#[test]
fn test_next_bare_and_matching_still_work() {
    let output = compile_and_run(
        r#"
FOR I = 1 TO 2
  FOR J = 1 TO 2
    PRINT I; J
  NEXT J
NEXT I
FOR K = 1 TO 2
  FOR L = 1 TO 2
  NEXT
NEXT
PRINT "done"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, &["11", "12", "21", "22", "done"]);
}

/// One `NEXT` may close several loops, innermost name first.
#[test]
fn test_next_closes_several_loops() {
    let output = compile_and_run(
        r#"
FOR I = 1 TO 2
  FOR J = 1 TO 2
    PRINT I; J
NEXT J, I
PRINT "done"
FOR A = 1 TO 2
  FOR B = 1 TO 2
    FOR C = 1 TO 2
      T = T + 1
NEXT C, B, A
PRINT T
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, &["11", "12", "21", "22", "done", "8"]);
}

/// The names in a multi-loop `NEXT` are checked in order, and a name with no
/// loop left to close is refused rather than silently dropped.
#[test]
fn test_next_list_is_checked() {
    for source in [
        // Wrong order: J is the inner loop, so `NEXT I, J` is backwards.
        "FOR I = 1 TO 2\nFOR J = 1 TO 2\nNEXT I, J\n",
        // One name too many.
        "FOR I = 1 TO 2\nNEXT I, J\n",
    ] {
        let e = crate::common::compile_only(source)
            .expect_err(&format!("{source:?} should be refused"));
        assert!(e.is_clean_rejection(), "stderr: {}", e.stderr);
    }
}

/// `IF cond THEN` followed by only colons opens a block, not a single-line IF.
///
/// The single-line test treated anything other than end-of-line as the start of
/// a statement, so the colons made this a one-line IF and the END IF below was
/// then unmatched.
#[test]
fn test_if_then_trailing_colon_is_still_a_block() {
    let output = compile_and_run(
        r#"
X = 1
IF X = 1 THEN :
PRINT "in"
END IF
PRINT "after"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, &["in", "after"]);
}

/// `SYSTEM` ends the program, as `END` does.
#[test]
fn test_system_ends_the_program() {
    let run = crate::common::compile_and_run_raw("PRINT \"before\"\nSYSTEM\nPRINT \"after\"\n", "")
        .expect("should compile");
    assert_eq!(run.lines(), vec!["before"], "SYSTEM stops the program");
    assert_eq!(run.exit_code, Some(0));
}

/// `BEEP` writes the bell character.
#[test]
fn test_beep_rings_the_bell() {
    let run = crate::common::compile_and_run_raw("BEEP\n", "").expect("should compile");
    assert!(
        run.stdout.contains('\u{7}'),
        "BEEP writes BEL: {:?}",
        run.stdout
    );
}

/// `ERASE` releases an array so it can be dimensioned again.
#[test]
fn test_erase_allows_a_second_dim() {
    let output = compile_and_run(
        r#"
DIM A(3)
A(1) = 7
PRINT A(1)
ERASE A
DIM A(10)
PRINT A(1)
A(9) = 5
PRINT A(9)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        &["7", "0", "5"],
        "the new array starts zeroed and is larger"
    );
}

/// The same, under `DEFINT A-Z`.
///
/// The default-type pass renames every unsuffixed name that a DEF* range
/// covers, so `DIM A(3)` declares `A%` -- but ERASE carried its names outside
/// an expression and outside the list of statements the pass rewrote, so it
/// went on naming `A`. Nothing matched, ERASE quietly did nothing, and the
/// second DIM was rejected as a redeclaration of an array the program had
/// just asked to be rid of.
#[test]
fn test_erase_under_a_def_type_default() {
    let output = compile_and_run(
        r#"
DEFINT A-Z
DIM A(3)
A(1) = 7
PRINT A(1)
ERASE A
DIM A(10)
A(9) = 5
PRINT A(9)
"#,
    )
    .unwrap();
    assert_eq!(output.trim().lines().collect::<Vec<_>>(), &["7", "5"]);
}

/// ERASE of something that is not an array is a mistake, not a no-op.
///
/// Codegen skips a name it cannot resolve to an array, on the grounds that
/// sema has already complained -- which sema did not do, so `ERASE TOTLA` for
/// `ERASE TOTAL` compiled clean and erased nothing.
#[test]
fn test_erase_of_a_non_array_is_diagnosed() {
    for source in [
        "ERASE NOSUCH\n",
        "X = 5\nERASE X\n",
        "DIM A(3)\nERASE A, B\n",
    ] {
        let err = compile_only(source).expect_err("ERASE of a non-array must be refused");
        assert!(
            err.contains("not a declared array"),
            "expected an explanation, got: {}",
            err.stderr
        );
        assert!(err.is_clean_rejection());
    }
}

/// A procedure's own array is erasable, and a module-level one stays visible
/// from inside a procedure -- the same two-step lookup every array use gets.
#[test]
fn test_erase_resolves_like_any_other_array_use() {
    let output = compile_and_run(
        r#"
DIM G(3)
SUB Wipe
  DIM L(2)
  L(0) = 1
  ERASE L
  ERASE G
  DIM L(4)
  DIM G(9)
  PRINT "ok"
END SUB
CALL Wipe
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "ok");
}
