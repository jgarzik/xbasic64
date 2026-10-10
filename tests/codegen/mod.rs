//! Assertions on the Cranelift IR the compiler generates, plus behavioural
//! tests of code paths that once had special cases in the old assembly
//! backend.
//!
//! The IR assertions cover decisions the compiler makes itself, before
//! Cranelift sees the function: a check it can settle at compile time, or a
//! loop it can count in integers. A program whose divide-by-zero test stops
//! being folded computes the same answers, so every other suite would stay
//! green; these would not. They are pattern assertions, not golden files:
//! naming the one symbol or instruction that must or must not appear says what
//! the test is about, where a golden `.clif` would be regenerated rather than
//! read.
//!
//! The behavioural tests run programs through constant operands, comparisons
//! used as conditions and as values, array element widths, FOR-loop counters
//! and accumulators, and array accesses inside loops. Register allocation and
//! instruction selection are Cranelift's now, but each of those paths was once
//! hand-optimized and got wrong in its own way, so the programs that pinned
//! them down still earn their place.
//!
//! [`compile_to_clif`] returns only the generated functions -- the runtime is a
//! prebuilt object linked beside them.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::{compile_to_clif, uses_symbol};

/// Dividing by a constant zero is decided at compile time, not re-tested.
///
/// The runtime test (`fcmp eq` against 0.0) exists to catch a divisor only
/// known at run time. When the divisor is written into the program the branch
/// is not a branch, and the trap is unconditional -- but it is still a trap,
/// and running the program must still stop with Division by zero.
#[test]
fn test_constant_zero_divisor_traps_without_testing() {
    let src = "Y# = 5\nX# = Y# / 0\nPRINT X#\n";
    let clif = compile_to_clif(src).expect("the program must compile");
    assert!(
        uses_symbol(&clif, "_err_div0") && uses_symbol(&clif, "_rt_error"),
        "a constant zero divisor must still raise:\n{}",
        clif
    );
    assert!(
        !clif.contains("fcmp eq"),
        "a constant divisor needs no run-time zero test:\n{}",
        clif
    );

    let run = crate::common::compile_and_run_raw(src, "").expect("the program must compile");
    assert_eq!(run.exit_code, Some(1));
    assert!(
        run.stderr.contains("Division by zero") || run.stdout.contains("Division by zero"),
        "expected a division-by-zero abort, got {:?} / {:?}",
        run.stdout,
        run.stderr
    );
}

/// ...and a nonzero constant divisor is not tested at all.
#[test]
fn test_constant_nonzero_divisor_is_not_tested() {
    let clif = compile_to_clif("Y = 5\nX = Y / 4\nPRINT X\n").expect("the program must compile");
    assert!(
        !uses_symbol(&clif, "_err_div0"),
        "a nonzero constant divisor cannot divide by zero:\n{}",
        clif
    );
    assert!(
        !clif.contains("fcmp eq"),
        "no zero test expected:\n{}",
        clif
    );
}

/// A divisor only known at run time is still tested, so the two tests above
/// are not passing merely because no division is ever checked.
#[test]
fn test_variable_divisor_is_tested() {
    let clif =
        compile_to_clif("D = 3\nY = 5\nX = Y / D\nPRINT X\n").expect("the program must compile");
    assert!(
        uses_symbol(&clif, "_err_div0") && clif.contains("fcmp eq"),
        "a variable divisor must be checked:\n{}",
        clif
    );
}

/// Every operator with a constant right operand must still compute correctly.
#[test]
fn test_constant_right_operand_arithmetic_is_correct() {
    let src = "\
Y% = 7
PRINT Y% MOD 3
PRINT Y% \\ 2
PRINT Y% + 5
PRINT Y% - 2
PRINT Y% * 3
Z# = 2.5
PRINT Z# ^ 2
PRINT Z# / 2
PRINT -Z#
PRINT ABS(-Z#)
PRINT (Y% > 3)
PRINT (Z# < 1)
PRINT (Y% = 7)
PRINT (Z# >= 2.5)
";
    let out = crate::common::compile_and_run(src).expect("the program must run");
    let got: Vec<&str> = out.lines().map(str::trim).collect();
    assert_eq!(
        got,
        vec![
            "1", "3", "12", "5", "21", "6.25", "1.25", "-2.5", "2.5", "-1", "0", "-1", "-1"
        ]
    );
}

/// A CONST operand computes the same as the literal it names.
#[test]
fn test_named_constants_take_the_constant_path() {
    let src = "CONST THREE = 3\nY# = 2\nX# = Y# * THREE\nPRINT X#\n";
    let out = crate::common::compile_and_run(src).expect("the program must run");
    assert_eq!(out.trim(), "6");
}

/// Each loop form runs the right number of times.
///
/// The post-test forms branch backwards, so their senses are inverted against
/// the pre-test ones -- the easiest thing about a loop to get backwards.
#[test]
fn test_every_loop_form_iterates_correctly() {
    for (src, expected) in [
        ("I% = 0\nWHILE I% < 3\nI% = I% + 1\nWEND\nPRINT I%\n", "3"),
        (
            "I% = 0\nDO WHILE I% < 4\nI% = I% + 1\nLOOP\nPRINT I%\n",
            "4",
        ),
        (
            "I% = 0\nDO UNTIL I% >= 5\nI% = I% + 1\nLOOP\nPRINT I%\n",
            "5",
        ),
        (
            "I% = 0\nDO\nI% = I% + 1\nLOOP WHILE I% < 6\nPRINT I%\n",
            "6",
        ),
        (
            "I% = 0\nDO\nI% = I% + 1\nLOOP UNTIL I% >= 7\nPRINT I%\n",
            "7",
        ),
        // A post-test loop runs its body at least once, however false the
        // condition was to begin with.
        (
            "I% = 9\nDO\nI% = I% + 1\nLOOP WHILE I% < 3\nPRINT I%\n",
            "10",
        ),
    ] {
        let out = crate::common::compile_and_run(src).expect("the program must run");
        assert_eq!(out.trim(), expected, "for:\n{}", src);
    }
}

/// A comparison that is *not* a condition still produces a -1/0 word.
///
/// A comparison used as a condition can branch directly; one that is assigned
/// or printed needs a value, and BASIC's true is -1.
#[test]
fn test_comparison_as_a_value_is_still_materialized() {
    let src = "A% = 5\nB% = 3\nN% = (A% > B%)\nPRINT N%\n";
    let out = crate::common::compile_and_run(src).expect("the program must run");
    assert_eq!(out.trim(), "-1");
}

/// A bitwise AND of two comparisons needs both as words, and must still work.
#[test]
fn test_and_of_comparisons_is_correct() {
    let src = "\
A% = 5
B% = 3
X# = 1.5
IF (A% > B%) AND (X# < 2) THEN PRINT \"both\"
IF (A% < B%) AND (X# < 2) THEN PRINT \"wrong\" ELSE PRINT \"neither\"
";
    let out = crate::common::compile_and_run(src).expect("the program must run");
    let got: Vec<&str> = out.lines().map(str::trim).collect();
    assert_eq!(got, vec!["both", "neither"]);
}

/// A string array element, sixteen bytes wide, is written and read back.
#[test]
fn test_string_array_keeps_the_multiply() {
    let src = "DIM A$(10)\nI% = 3\nA$(I%) = \"x\"\nPRINT A$(I%)\n";
    let out = crate::common::compile_and_run(src).expect("the program must run");
    assert_eq!(out.trim(), "x");
}

/// So is a record element, whose size is eight times its word count.
#[test]
fn test_record_array_keeps_the_multiply() {
    let src = "\
TYPE Pair
  A AS DOUBLE
  B AS DOUBLE
END TYPE
DIM R(4) AS Pair
R(2).A = 1
R(2).B = 2
PRINT R(2).A + R(2).B
";
    let out = crate::common::compile_and_run(src).expect("the program must run");
    assert_eq!(out.trim(), "3");
}

/// Every scalar element width round-trips what it holds.
#[test]
fn test_array_elements_round_trip_through_every_type() {
    let src = "\
DIM I%(4)
DIM L&(4)
DIM S!(4)
DIM D#(4)
DIM T$(4)
FOR K = 0 TO 4
  I%(K) = K * 2
  L&(K) = K * 1000
  S!(K) = K
  D#(K) = K / 2
  T$(K) = \"v\"
NEXT K
PRINT I%(3), L&(3), S!(3), D#(3), T$(3)
PRINT I%(0), L&(4), D#(1)
";
    let out = crate::common::compile_and_run(src).expect("the program must run");
    // Split on the print zones rather than pinning their width: what this
    // test is about is that every element type round-trips, and
    // `test_comma_print_zones` owns the layout.
    let got: Vec<Vec<&str>> = crate::common::lines(&out)
        .iter()
        .map(|l| l.split_whitespace().collect())
        .collect();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0], ["6", "3000", "3", "1.5", "v"]);
    assert_eq!(got[1], ["0", "4000", ".5"]);
}

/// Bounds checking catches a subscript past the end.
#[test]
fn test_scaled_index_is_still_bounds_checked() {
    let src = "DIM A#(4)\nI% = 9\nPRINT A#(I%)\n";
    let run = crate::common::compile_and_run_raw(src, "").expect("the program must compile");
    assert_eq!(run.exit_code, Some(1));
    assert!(
        run.stderr.contains("Subscript out of range") || run.stdout.contains("Subscript"),
        "expected a subscript abort, got {:?} / {:?}",
        run.stdout,
        run.stderr
    );
}

/// A multi-dimensional element is addressed by its row-major index.
#[test]
fn test_multidimensional_array_scales_too() {
    let src = "\
DIM A#(3, 3)
A#(1, 2) = 7
PRINT A#(1, 2)
";
    let out = crate::common::compile_and_run(src).expect("the program must run");
    assert_eq!(out.trim(), "7");
}

/// An INTEGER counter counts in integers, not in doubles.
///
/// The loop machinery used to work entirely in Double whatever the control
/// variable's type was, which was both slower and wrong -- see
/// `control::test_for_loop_control_variable_types`. The step is a sixteen-bit
/// `sadd_overflow`, so that stepping past INTEGER's range is caught rather than
/// silently wrapping, and no floating-point add or compare drives the loop.
#[test]
fn test_integer_for_loop_counts_in_integers() {
    let clif = compile_to_clif("FOR I% = 1 TO 3\nPRINT I%\nNEXT I%\n").expect("must compile");
    for needle in ["sadd_overflow", "ireduce.i16", "icmp sgt"] {
        assert!(
            clif.contains(needle),
            "expected `{}` in the loop:\n{}",
            needle,
            clif
        );
    }
    for needle in ["fadd", "fcmp"] {
        assert!(
            !clif.contains(needle),
            "did not expect `{}` in an INTEGER loop:\n{}",
            needle,
            clif
        );
    }
}

/// Stepping an INTEGER counter past its range is reported, not wrapped.
///
/// Wrapping means the counter never passes its limit, so the loop runs
/// forever. GW-BASIC reports Overflow here and so does this.
#[test]
fn test_integer_counter_overflow_is_reported() {
    let src = "FOR I% = 32764 TO 32767\nPRINT I%\nNEXT I%\n";
    let run = crate::common::compile_and_run_raw(src, "").expect("the program must compile");
    assert_eq!(run.exit_code, Some(1));
    assert!(
        run.stdout.contains("32767"),
        "the in-range iterations must run first, got {:?}",
        run.stdout
    );
    assert!(
        run.stderr.contains("Overflow") || run.stdout.contains("Overflow"),
        "expected an Overflow abort, got {:?} / {:?}",
        run.stdout,
        run.stderr
    );
}

/// Nested loops keep their counters apart.
#[test]
fn test_nested_promoted_loops_do_not_collide() {
    let src = "\
T# = 0
FOR I = 1 TO 3
  FOR J = 1 TO 4
    T# = T# + I * J
  NEXT J
NEXT I
PRINT T#
";
    let out = crate::common::compile_and_run(src).expect("the program must run");
    assert_eq!(out.trim(), "60", "(1+2+3) * (1+2+3+4)");
}

/// A loop whose body assigns to its counter sees the assignment, as BASIC
/// allows.
#[test]
fn test_loop_assigning_its_counter_keeps_it_in_memory() {
    // The body steps the counter itself, so it advances by three per
    // iteration: 1, 4, 7, 10, then 13, which is past the limit. Four
    // iterations and a final value of 13, both of which depend on the loop
    // reading back the counter the body wrote.
    let src = "\
N# = 0
FOR I = 1 TO 10
  N# = N# + 1
  I = I + 2
NEXT I
PRINT N#
PRINT I
";
    let out = crate::common::compile_and_run(src).expect("the program must run");
    let lines = crate::common::lines(&out);
    assert_eq!(lines[0], "4", "the body ran four times");
    assert_eq!(lines[1], "13", "the body's own step took effect");
}

/// After EXIT FOR, the counter holds the value it left with.
#[test]
fn test_exit_for_writes_a_promoted_counter_back() {
    let out = crate::common::compile_and_run(
        "N# = 0\nFOR I = 1 TO 100\nN# = N# + 1\nIF I = 4 THEN EXIT FOR\nNEXT I\nPRINT N#\nPRINT I\n",
    )
    .expect("the program must run");
    let lines = crate::common::lines(&out);
    assert_eq!(lines[0], "4", "the body ran four times");
    assert_eq!(lines[1], "4", "the counter survived EXIT FOR");
}

/// INTEGER arithmetic is checked at 16 bits: 300 * 300 is 90000, which does
/// not fit an INTEGER, and GW-BASIC stops with Overflow rather than wrap. This
/// used to give 24464, and the same product printed directly gave 90000.
#[test]
fn test_integer_multiply_overflows_at_16_bits() {
    let src = "\
DIM X AS INTEGER
DIM Y AS LONG
X = 300
Y = X * X
PRINT Y
";
    let run = crate::common::compile_and_run_raw(src, "").expect("should compile");
    assert_eq!(run.exit_code, Some(1), "stderr {}", run.stderr);
    assert!(run.stderr.contains("Overflow"), "stderr {}", run.stderr);
}

/// No runtime helper may fall through into the next one.
///
/// Sections are not laid out in source order, so a helper that ran off its
/// end into its neighbour would land somewhere else entirely once each is
/// sectioned separately. `_rt_lcase` used to do this, into `_rt_case_convert`.
#[test]
fn test_no_runtime_helper_falls_through() {
    for tree in ["sysv", "win64-native"] {
        for file in [
            "print.s", "input.s", "string.s", "math.s", "data.s", "file.s", "error.s", "using.s",
        ] {
            // Anchored at the manifest rather than the working directory,
            // which a test runner is free to set wherever it likes.
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/runtime")
                .join(tree)
                .join(file);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
            let mut previous: Option<String> = None;
            for line in text.lines() {
                let t = line.trim();
                if t.starts_with("_rt_")
                    && t.ends_with(':')
                    && let Some(prev) = &previous
                {
                    let op = prev.split_whitespace().next().unwrap_or("");
                    // `_rt_error` never returns, so reaching a label after
                    // a call to it is not a fall-through.
                    let terminal = matches!(op, "ret" | "jmp" | "ud2" | "hlt")
                        || prev.starts_with("call _rt_error");
                    assert!(
                        terminal,
                        "{} falls into {} from `{}`",
                        path.display(),
                        t,
                        prev
                    );
                }
                // Track the last thing that is an instruction: not a comment,
                // not a directive, and not a label -- including the
                // `name: .skip 1024` form, whose first token ends in a colon
                // but whose line does not.
                let first = t.split_whitespace().next().unwrap_or("");
                if !t.is_empty()
                    && !t.starts_with('#')
                    && !t.starts_with('.')
                    && !first.ends_with(':')
                {
                    previous = Some(t.to_string());
                }
            }
        }
    }
}

/// Several accumulators in one loop all accumulate.
#[test]
fn test_accumulator_promotion_has_a_limit() {
    let src = "\
A# = 0
B# = 0
C# = 0
D# = 0
FOR I = 1 TO 4
  A# = A# + 1
  B# = B# + 2
  C# = C# + 3
  D# = D# + 4
NEXT I
PRINT A#
PRINT B#
PRINT C#
PRINT D#
";
    let out = crate::common::compile_and_run(src).expect("must run");
    let got = crate::common::lines(&out);
    assert_eq!(got, vec!["4", "8", "12", "16"], "all four still accumulate");
}

/// An INTEGER accumulator in an INTEGER loop sums correctly.
#[test]
fn test_integer_accumulator_register_is_saved() {
    let src = "S% = 0\nFOR I% = 1 TO 5\nS% = S% + I%\nNEXT I%\nPRINT S%\n";
    let out = crate::common::compile_and_run(src).expect("must run");
    assert_eq!(out.trim(), "15");
}

/// Every way out of the loop leaves the accumulator visible and correct.
#[test]
fn test_accumulator_write_back_is_correct() {
    for (src, expected) in [
        // Normal exit.
        (
            "T# = 0\nFOR I = 1 TO 10\nT# = T# + I\nNEXT I\nPRINT T#\n",
            "55",
        ),
        // EXIT FOR.
        (
            "E# = 0\nFOR I = 1 TO 100\nE# = E# + 1\nIF I = 3 THEN EXIT FOR\nNEXT I\nPRINT E#\n",
            "3",
        ),
        // A body that never runs must leave the value alone, not zero it.
        ("H# = 5\nFOR I = 1 TO 0\nH# = 99\nNEXT I\nPRINT H#\n", "5"),
        // Assigned only on some iterations.
        (
            "F# = 0\nFOR I = 1 TO 3\nIF I = 2 THEN F# = F# + 10\nNEXT I\nPRINT F#\n",
            "10",
        ),
        // An INTEGER accumulator still wraps at sixteen bits.
        (
            "L% = 32000\nFOR I = 1 TO 3\nL% = L% + 100\nNEXT I\nPRINT L%\n",
            "32300",
        ),
    ] {
        let out = crate::common::compile_and_run(src).expect("must run");
        assert_eq!(out.trim(), expected, "for:\n{}", src);
    }
}

/// The limit is captured before the loop, so assigning it inside does not
/// extend the loop -- and the assignment still takes effect.
#[test]
fn test_promoted_accumulator_does_not_change_the_limit() {
    let out = crate::common::compile_and_run(
        "N = 3\nK# = 0\nFOR I = 1 TO N\nN = 100\nK# = K# + 1\nNEXT I\nPRINT K#\nPRINT N\n",
    )
    .expect("must run");
    let got = crate::common::lines(&out);
    assert_eq!(got, vec!["3", "100"]);
}

/// A loop that never runs creates no array.
///
/// A use of an array with no storage creates it, as GW-BASIC does; doing that
/// before the loop rather than at the access would create an array that a
/// loop running zero times never touched, and a later DIM of it would then be
/// a Duplicate definition.
#[test]
fn test_autodim_check_is_not_hoisted_out_of_the_loop() {
    let out = crate::common::compile_and_run(
        "FOR I = 1 TO 0\nB(I) = 1\nNEXT I\nDIM B(3)\nPRINT UBOUND(B)\n",
    )
    .expect("a loop that never runs creates nothing");
    assert_eq!(out.trim(), "3");
}

/// Bounds checking inside a loop catches the first subscript past the end.
#[test]
fn test_bounds_check_uses_the_hoisted_bound() {
    let run =
        crate::common::compile_and_run_raw("DIM A(3)\nFOR I = 0 TO 9\nA(I) = I\nNEXT I\n", "")
            .expect("should compile");
    assert_eq!(run.exit_code, Some(1));
    assert!(
        run.stderr.contains("Subscript out of range"),
        "got {:?}",
        run.stderr
    );
}

/// Several arrays, a two-dimensional one, and nesting all still compute.
#[test]
fn test_hoisted_arrays_still_compute_correctly() {
    let out = crate::common::compile_and_run(
        "\
DIM A(5)
DIM B(5)
DIM C(5)
DIM D(3, 3)
FOR I = 0 TO 5
  A(I) = I
  B(I) = I * 2
  C(I) = A(I) + B(I)
NEXT I
PRINT C(3)
FOR I = 0 TO 3
  FOR J = 0 TO 3
    D(I, J) = I * 10 + J
  NEXT J
NEXT I
PRINT D(2, 3)
T# = 0
FOR I = 0 TO 5
  T# = T# + A(I) + B(I) + C(I)
NEXT I
PRINT T#
",
    )
    .expect("must run");
    let got = crate::common::lines(&out);
    assert_eq!(got, vec!["9", "23", "90"]);
}
