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
