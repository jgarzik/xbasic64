//! Math function tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_sqr() {
    // SQR with various input types
    let output = compile_and_run(
        r#"
PRINT SQR(16)
A% = 25: PRINT SQR(A%)
A& = 10000: PRINT SQR(A&)
A! = 2.25: PRINT SQR(A!)
A# = 2.25: PRINT SQR(A#)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "4", "sqr literal");
    assert_eq!(lines[1], "5", "sqr integer");
    assert_eq!(lines[2], "100", "sqr long");
    assert_eq!(lines[3], "1.5", "sqr single");
    assert_eq!(lines[4], "1.5", "sqr double");
}

#[test]
fn test_abs() {
    // ABS with various input types
    let output = compile_and_run(
        r#"
PRINT ABS(-42)
A% = -42: PRINT ABS(A%)
A& = -100000: PRINT ABS(A&)
A! = -3.14: PRINT ABS(A!)
A# = -3.14159: PRINT ABS(A#)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "42", "abs literal");
    assert_eq!(lines[1], "42", "abs integer");
    assert_eq!(lines[2], "100000", "abs long");
    assert_eq!(lines[3], "3.14", "abs single");
    assert_eq!(lines[4], "3.14159", "abs double");
}

#[test]
fn test_int_fix() {
    // INT floors, FIX truncates toward zero
    let output = compile_and_run(
        r#"
PRINT INT(3.7)
A! = 3.7: PRINT INT(A!)
A# = 3.7: PRINT INT(A#)
PRINT FIX(-3.7)
A! = -3.7: PRINT FIX(A!)
A# = -3.7: PRINT FIX(A#)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "3", "int literal");
    assert_eq!(lines[1], "3", "int single");
    assert_eq!(lines[2], "3", "int double");
    assert_eq!(lines[3], "-3", "fix literal");
    assert_eq!(lines[4], "-3", "fix single");
    assert_eq!(lines[5], "-3", "fix double");
}

#[test]
fn test_sgn() {
    // SGN with various input types
    let output = compile_and_run(
        r#"
PRINT SGN(-5)
PRINT SGN(0)
PRINT SGN(5)
A% = -5: PRINT SGN(A%)
A& = -50000: PRINT SGN(A&)
A! = -2.5: PRINT SGN(A!)
A# = -2.5: PRINT SGN(A#)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "-1", "sgn neg");
    assert_eq!(lines[1], "0", "sgn zero");
    assert_eq!(lines[2], "1", "sgn pos");
    assert_eq!(lines[3], "-1", "sgn integer");
    assert_eq!(lines[4], "-1", "sgn long");
    assert_eq!(lines[5], "-1", "sgn single");
    assert_eq!(lines[6], "-1", "sgn double");
}

#[test]
fn test_trig_sin_cos() {
    // SIN and COS with various input types
    let output = compile_and_run(
        r#"
PRINT INT(SIN(0) * 100), INT(COS(0) * 100)
A% = 0: PRINT INT(SIN(A%) * 100)
A% = 0: PRINT INT(COS(A%) * 100)
A& = 0: PRINT INT(SIN(A&) * 100)
A& = 0: PRINT INT(COS(A&) * 100)
A! = 0.0: PRINT INT(SIN(A!) * 100)
A! = 0.0: PRINT INT(COS(A!) * 100)
A# = 0.0: PRINT INT(SIN(A#) * 100)
A# = 0.0: PRINT INT(COS(A#) * 100)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    let values: Vec<&str> = lines[0].split_whitespace().collect();
    assert_eq!(values, vec!["0", "100"], "sin/cos literals");
    assert_eq!(lines[1], "0", "sin integer");
    assert_eq!(lines[2], "100", "cos integer");
    assert_eq!(lines[3], "0", "sin long");
    assert_eq!(lines[4], "100", "cos long");
    assert_eq!(lines[5], "0", "sin single");
    assert_eq!(lines[6], "100", "cos single");
    assert_eq!(lines[7], "0", "sin double");
    assert_eq!(lines[8], "100", "cos double");
}

#[test]
fn test_trig_tan_atn() {
    // TAN and ATN with various input types
    let output = compile_and_run(
        r#"
PRINT INT(TAN(0) * 100), INT(ATN(0) * 100)
A! = 0.0: PRINT INT(TAN(A!) * 100)
A! = 0.0: PRINT INT(ATN(A!) * 100)
A# = 0.0: PRINT INT(TAN(A#) * 100)
A# = 0.0: PRINT INT(ATN(A#) * 100)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    let values: Vec<&str> = lines[0].split_whitespace().collect();
    assert_eq!(values, vec!["0", "0"], "tan/atn literals");
    assert_eq!(lines[1], "0", "tan single");
    assert_eq!(lines[2], "0", "atn single");
    assert_eq!(lines[3], "0", "tan double");
    assert_eq!(lines[4], "0", "atn double");
}

#[test]
fn test_exp_log() {
    // EXP and LOG with various input types
    let output = compile_and_run(
        r#"
PRINT INT(EXP(0)), INT(LOG(1))
A% = 0: PRINT INT(EXP(A%))
A% = 1: PRINT INT(LOG(A%))
A! = 0.0: PRINT INT(EXP(A!))
A! = 1.0: PRINT INT(LOG(A!))
A# = 0.0: PRINT INT(EXP(A#))
A# = 1.0: PRINT INT(LOG(A#))
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    let values: Vec<&str> = lines[0].split_whitespace().collect();
    assert_eq!(values, vec!["1", "0"], "exp/log literals");
    assert_eq!(lines[1], "1", "exp integer");
    assert_eq!(lines[2], "0", "log integer");
    assert_eq!(lines[3], "1", "exp single");
    assert_eq!(lines[4], "0", "log single");
    assert_eq!(lines[5], "1", "exp double");
    assert_eq!(lines[6], "0", "log double");
}

#[test]
fn test_rnd_timer() {
    // RND returns 0-1, TIMER returns seconds since midnight
    let output = compile_and_run(
        r#"
X = RND(1)
IF X >= 0 AND X < 1 THEN PRINT "rnd-ok"
Y = RND
IF Y >= 0 AND Y < 1 THEN PRINT "bare-rnd-ok"
IF X <> Y THEN PRINT "advances"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "rnd-ok");
    assert_eq!(
        lines[1], "bare-rnd-ok",
        "a bare RND is a call, not a variable"
    );
    assert_eq!(lines[2], "advances");
}

#[test]
fn test_type_conversions() {
    // CINT, CLNG, CSNG, CDBL with various inputs
    let output = compile_and_run(
        r#"
A% = 42: PRINT CINT(A%)
A& = 12345: PRINT CINT(A&)
A! = 3.7: PRINT CINT(A!)
A! = 3.7: PRINT CLNG(A!)
A% = 42: B! = CSNG(A%): PRINT B!
A& = 12345: B! = CSNG(A&): PRINT B!
A% = 42: B# = CDBL(A%): PRINT B#
A& = 12345: B# = CDBL(A&): PRINT B#
A! = 3.5: B# = CDBL(A!): PRINT B#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "42", "cint integer");
    assert_eq!(lines[1], "12345", "cint long");
    assert_eq!(lines[2], "4", "cint single");
    assert_eq!(lines[3], "4", "clng single");
    assert_eq!(lines[4], "42", "csng integer");
    assert_eq!(lines[5], "12345", "csng long");
    assert_eq!(lines[6], "42", "cdbl integer");
    assert_eq!(lines[7], "12345", "cdbl long");
    assert_eq!(lines[8], "3.5", "cdbl single");
}

/// TIMER returns the host's seconds since midnight, UTC.
///
/// Asserting only `TIMER >= 0` could not fail: a bare `TIMER` used to read an
/// uninitialized variable of that name, which is 0, and 0 passes that test.
/// Comparing against the clock this test can read itself is what makes the
/// assertion mean something -- and it is what caught the Win64 runtime
/// returning seconds since *boot* rather than since midnight.
#[test]
fn test_timer_matches_the_host_clock() {
    let before = seconds_since_midnight_utc();
    let output = compile_and_run("PRINT TIMER\nPRINT TIMER(0)\n").unwrap();
    let after = seconds_since_midnight_utc();

    for (i, line) in output.trim().lines().enumerate() {
        // TIMER counts fractional seconds, as GW-BASIC's does.
        let t: f64 = line
            .parse()
            .unwrap_or_else(|e| panic!("TIMER printed {line:?}: {e}"));
        assert!(
            (0.0..86_400.0).contains(&t),
            "TIMER must be a second-of-day, got {t}"
        );
        // Compiling and running takes a moment, and the day may roll over in
        // between, so compare modulo a day with a generous window.
        let skew = (t - before)
            .rem_euclid(86_400.0)
            .min((after - t).rem_euclid(86_400.0));
        assert!(
            skew < 120.0,
            "form {i}: TIMER said {t}, but the clock read {before}..{after}"
        );
    }
}

/// TIMER never goes backwards within a run.
#[test]
fn test_timer_does_not_go_backwards() {
    let output = compile_and_run(
        "A = TIMER\nFOR I = 1 TO 2000000\nX = X + 1\nNEXT I\nB = TIMER\nPRINT (B >= A)\nPRINT (B - A < 60)\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["-1", "-1"]);
}

/// Seconds since midnight UTC, the same quantity TIMER reports.
fn seconds_since_midnight_utc() -> f64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970");
    (now.as_secs() % 86_400) as f64 + f64::from(now.subsec_millis()) / 1000.0
}

/// `RANDOMIZE` reseeds, so a program does not replay the same numbers forever.
///
/// The generator's state was a hardcoded constant and `_rt_rnd` ignored its
/// argument outright, so every run of every program produced the identical
/// sequence — three runs of the same three-dice program printed `939913` each
/// time. A dice game, a shuffle and a maze were all the same on every play, and
/// no statement existed to change that.
#[test]
fn test_randomize_timer_differs_between_runs() {
    let source = "RANDOMIZE TIMER\nFOR I = 1 TO 5\nPRINT INT(RND * 1000);\nNEXT I\n";
    let a = compile_and_run(source).unwrap();
    let b = compile_and_run(source).unwrap();
    assert_ne!(a.trim(), b.trim(), "RANDOMIZE TIMER must reseed");
}

/// A named seed is reproducible, which is what makes a program debuggable.
#[test]
fn test_randomize_with_a_seed_is_reproducible() {
    let source = "RANDOMIZE 42\nFOR I = 1 TO 5\nPRINT INT(RND * 1000);\nNEXT I\n";
    let a = compile_and_run(source).unwrap();
    let b = compile_and_run(source).unwrap();
    assert_eq!(a.trim(), b.trim(), "the same seed must give the same run");

    let other =
        compile_and_run("RANDOMIZE 7\nFOR I = 1 TO 5\nPRINT INT(RND * 1000);\nNEXT I\n").unwrap();
    assert_ne!(a.trim(), other.trim(), "a different seed must differ");
}

/// GW-BASIC's `RND` argument selects between three behaviours.
#[test]
fn test_rnd_argument_semantics() {
    let output = compile_and_run(
        r#"
RANDOMIZE 1
A = RND
B = RND(0)
C = RND(0)
D = RND(1)
IF A = B THEN PRINT "zero-repeats" ELSE PRINT "zero-advanced"
IF B = C THEN PRINT "zero-stable" ELSE PRINT "zero-moved"
IF A = D THEN PRINT "positive-stuck" ELSE PRINT "positive-advances"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        &["zero-repeats", "zero-stable", "positive-advances"],
        "RND(0) repeats the last value; RND(positive) and bare RND advance"
    );
}

/// A negative argument reseeds from that value, so it is reproducible without
/// RANDOMIZE — the idiom `X = RND(-1)` at the top of a listing.
#[test]
fn test_rnd_negative_reseeds() {
    let source = "X = RND(-1)\nFOR I = 1 TO 3\nPRINT INT(RND * 1000);\nNEXT I\n";
    let a = compile_and_run(source).unwrap();
    let b = compile_and_run(source).unwrap();
    assert_eq!(a.trim(), b.trim(), "the same negative seed replays");

    let other =
        compile_and_run("X = RND(-99)\nFOR I = 1 TO 3\nPRINT INT(RND * 1000);\nNEXT I\n").unwrap();
    assert_ne!(a.trim(), other.trim(), "a different negative seed differs");
}

/// Values stay in [0, 1) whichever form is used.
#[test]
fn test_rnd_stays_in_range() {
    let output = compile_and_run(
        r#"
RANDOMIZE 5
BAD = 0
FOR I = 1 TO 200
  R = RND
  IF R < 0 OR R >= 1 THEN BAD = BAD + 1
NEXT I
PRINT BAD
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "0", "every value must be in [0, 1)");
}
