//! Control flow tests

use crate::common::compile_and_run;

#[test]
fn test_for_loop() {
    let output = compile_and_run(
        r#"
FOR I = 1 TO 5
    PRINT I
NEXT I
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1", "2", "3", "4", "5"]);
}

#[test]
fn test_for_step_positive() {
    let output = compile_and_run(
        r#"
FOR I = 0 TO 10 STEP 2
    PRINT I
NEXT I
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["0", "2", "4", "6", "8", "10"]);
}

#[test]
fn test_for_step_negative() {
    let output = compile_and_run(
        r#"
FOR I = 5 TO 1 STEP -1
    PRINT I
NEXT I
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["5", "4", "3", "2", "1"]);
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
fn test_do_loop_while() {
    let output = compile_and_run(
        r#"
X = 1
DO WHILE X <= 3
    PRINT X
    X = X + 1
LOOP
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1", "2", "3"]);
}

#[test]
fn test_do_loop_until() {
    let output = compile_and_run(
        r#"
X = 1
DO UNTIL X > 3
    PRINT X
    X = X + 1
LOOP
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1", "2", "3"]);
}

#[test]
fn test_do_loop_while_post() {
    let output = compile_and_run(
        r#"
X = 1
DO
    PRINT X
    X = X + 1
LOOP WHILE X <= 3
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1", "2", "3"]);
}

#[test]
fn test_if_then_else() {
    let output = compile_and_run(
        r#"
X = 10
IF X > 5 THEN
    PRINT "big"
ELSE
    PRINT "small"
END IF
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "big");
}

#[test]
fn test_if_then_else_false() {
    let output = compile_and_run(
        r#"
X = 3
IF X > 5 THEN
    PRINT "big"
ELSE
    PRINT "small"
END IF
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "small");
}

#[test]
fn test_elseif() {
    let output = compile_and_run(
        r#"
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
    assert_eq!(output.trim(), "two");
}

#[test]
fn test_goto() {
    let output = compile_and_run(
        r#"
10 PRINT "A"
20 GOTO 40
30 PRINT "B"
40 PRINT "C"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["A", "C"]);
}

#[test]
fn test_gosub_return() {
    let output = compile_and_run(
        r#"
10 PRINT "start"
20 GOSUB 100
30 PRINT "end"
40 END
100 PRINT "in sub"
110 RETURN
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["start", "in sub", "end"]);
}

#[test]
fn test_on_goto() {
    let output = compile_and_run(
        r#"
10 X = 2
20 ON X GOTO 100, 200, 300
30 PRINT "none"
40 END
100 PRINT "first"
110 END
200 PRINT "second"
210 END
300 PRINT "third"
310 END
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "second");
}

#[test]
fn test_select_case() {
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
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "two");
}

#[test]
fn test_select_case_else() {
    let output = compile_and_run(
        r#"
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
    assert_eq!(output.trim(), "other");
}

#[test]
fn test_end_statement() {
    let output = compile_and_run(
        r#"
PRINT "before"
END
PRINT "after"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "before");
}

#[test]
fn test_stop_statement() {
    let output = compile_and_run(
        r#"
PRINT "before"
STOP
PRINT "after"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "before");
}
