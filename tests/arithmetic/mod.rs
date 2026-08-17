//! Arithmetic and operator tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_basic_arithmetic() {
    // Tests: add, sub, mul, division, integer_division, mod, power
    let output = compile_and_run(
        r#"
PRINT 10 + 5
PRINT 10 - 3
PRINT 6 * 7
PRINT 10 / 4
PRINT 10 \ 4
PRINT 10 MOD 3
PRINT 2 ^ 10
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "15", "add");
    assert_eq!(lines[1], "7", "sub");
    assert_eq!(lines[2], "42", "mul");
    assert_eq!(lines[3], "2.5", "division");
    assert_eq!(lines[4], "2", "integer division");
    assert_eq!(lines[5], "1", "mod");
    assert_eq!(lines[6], "1024", "power");
}

#[test]
fn test_expressions() {
    // Tests: precedence, parentheses, negative numbers
    let output = compile_and_run(
        r#"
PRINT 2 + 3 * 4
PRINT (2 + 3) * 4
PRINT -5 + 10
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "14", "precedence");
    assert_eq!(lines[1], "20", "parentheses");
    assert_eq!(lines[2], "5", "negative");
}

/// The logical operators used as conditions.
///
/// `IF NOT 1` is the interesting line. `NOT` is a *bitwise* complement, so
/// `NOT 1` is -2 -- non-zero, therefore true. This test used to assert that it
/// was false, which is what a logical not would give; the two agree only when
/// the operand is already 0 or -1. Comparisons yield exactly those values,
/// which is why `IF NOT (A > 0)` reads the way anyone would expect while
/// `IF NOT 1` does not.
#[test]
fn test_logical_operators() {
    // Tests: AND, OR, NOT, XOR
    let output = compile_and_run(
        r#"
IF 1 AND 1 THEN PRINT "and-yes"
IF 1 AND 0 THEN PRINT "and-no"
IF 0 OR 1 THEN PRINT "or-yes"
IF 0 OR 0 THEN PRINT "or-no"
IF NOT 0 THEN PRINT "not-yes"
IF NOT 1 THEN PRINT "not-minus-two-is-true"
IF NOT (1 > 0) THEN PRINT "not-comparison-no"
IF NOT (1 < 0) THEN PRINT "not-comparison-yes"
IF 1 XOR 0 THEN PRINT "xor-a"
IF 0 XOR 1 THEN PRINT "xor-b"
IF 1 XOR 1 THEN PRINT "xor-c"
IF 0 XOR 0 THEN PRINT "xor-d"
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(
        lines,
        vec![
            "and-yes",
            "or-yes",
            "not-yes",
            "not-minus-two-is-true",
            "not-comparison-yes",
            "xor-a",
            "xor-b"
        ]
    );
}

/// `NOT` complements every bit; it is not a boolean negation.
///
/// It used to emit `sete al / movzx / neg` -- "0 gives -1, anything else gives
/// 0" -- so `NOT 12` was 0 rather than -13, while `12 AND 10` beside it was
/// correctly bitwise. LANGREF says these "operate bitwise on integers".
#[test]
fn test_not_is_a_bitwise_complement() {
    let output = compile_and_run(
        r#"
PRINT NOT 12
A = 12
PRINT NOT A
B% = 12
PRINT NOT B%
PRINT NOT 0
PRINT NOT -1
C = 1.9
PRINT NOT C
D% = &H00FF
PRINT NOT D%
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(
        lines,
        vec!["-13", "-13", "-13", "-1", "0", "-2", "-256"],
        "literal, Double, Integer, the boolean values, a truncated Double, and a bit mask"
    );
}

#[test]
fn test_comparison_operators() {
    let output = compile_and_run(
        r#"
IF 5 < 10 THEN PRINT "ok1"
IF 10 > 5 THEN PRINT "ok2"
IF 5 <= 5 THEN PRINT "ok3"
IF 5 >= 5 THEN PRINT "ok4"
IF 5 = 5 THEN PRINT "ok5"
IF 5 <> 6 THEN PRINT "ok6"
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines.len(), 6);
}

#[test]
fn test_integer_arithmetic() {
    // Tests: Integer (%) add, sub, mul, div, intdiv, mod, power, neg
    let output = compile_and_run(
        r#"
A% = 100: B% = 50: PRINT A% + B%
A% = 100: B% = 30: PRINT A% - B%
A% = 12: B% = 5: PRINT A% * B%
A% = 7: B% = 2: PRINT A% / B%
A% = 17: B% = 5: PRINT A% \ B%
A% = 17: B% = 5: PRINT A% MOD B%
A% = 2: B% = 8: PRINT A% ^ B%
A% = 42: PRINT -A%
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "150", "int add");
    assert_eq!(lines[1], "70", "int sub");
    assert_eq!(lines[2], "60", "int mul");
    assert_eq!(lines[3], "3.5", "int div");
    assert_eq!(lines[4], "3", "int intdiv");
    assert_eq!(lines[5], "2", "int mod");
    assert_eq!(lines[6], "256", "int power");
    assert_eq!(lines[7], "-42", "int neg");
}

#[test]
fn test_long_arithmetic() {
    // Tests: Long (&) add, sub, mul, div, intdiv, mod, power, neg
    let output = compile_and_run(
        r#"
A& = 100000: B& = 50000: PRINT A& + B&
A& = 100000: B& = 30000: PRINT A& - B&
A& = 1000: B& = 500: PRINT A& * B&
A& = 7: B& = 2: PRINT A& / B&
A& = 100: B& = 30: PRINT A& \ B&
A& = 100: B& = 30: PRINT A& MOD B&
A& = 3: B& = 5: PRINT A& ^ B&
A& = 12345: PRINT -A&
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "150000", "long add");
    assert_eq!(lines[1], "70000", "long sub");
    assert_eq!(lines[2], "500000", "long mul");
    assert_eq!(lines[3], "3.5", "long div");
    assert_eq!(lines[4], "3", "long intdiv");
    assert_eq!(lines[5], "10", "long mod");
    assert_eq!(lines[6], "243", "long power");
    assert_eq!(lines[7], "-12345", "long neg");
}

#[test]
fn test_single_arithmetic() {
    // Tests: Single (!) add, sub, mul, div, power, neg
    let output = compile_and_run(
        r#"
A! = 1.5: B! = 2.5: PRINT A! + B!
A! = 5.5: B! = 2.25: PRINT A! - B!
A! = 2.5: B! = 4.0: PRINT A! * B!
A! = 10.0: B! = 4.0: PRINT A! / B!
A! = 2.0: B! = 3.0: PRINT A! ^ B!
A! = 3.14: PRINT -A!
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "4", "single add");
    assert_eq!(lines[1], "3.25", "single sub");
    assert_eq!(lines[2], "10", "single mul");
    assert_eq!(lines[3], "2.5", "single div");
    assert_eq!(lines[4], "8", "single power");
    assert_eq!(lines[5], "-3.14", "single neg");
}

#[test]
fn test_double_arithmetic() {
    // Tests: Double (#) add, sub, mul, div, power, neg
    let output = compile_and_run(
        r#"
A# = 1.5: B# = 2.5: PRINT A# + B#
A# = 100.75: B# = 50.25: PRINT A# - B#
A# = 3.5: B# = 2.0: PRINT A# * B#
A# = 15.0: B# = 4.0: PRINT A# / B#
A# = 2.0: B# = 10.0: PRINT A# ^ B#
A# = 2.71828: PRINT -A#
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "4", "double add");
    assert_eq!(lines[1], "50.5", "double sub");
    assert_eq!(lines[2], "7", "double mul");
    assert_eq!(lines[3], "3.75", "double div");
    assert_eq!(lines[4], "1024", "double power");
    assert_eq!(lines[5], "-2.71828", "double neg");
}

/// LANGREF's precedence table puts `^` above unary minus, as does GW-BASIC, so
/// -2^2 is -(2^2). Unary minus used to bind tighter, giving 4.
#[test]
fn test_unary_minus_vs_power_precedence() {
    let output = compile_and_run(
        r#"
PRINT -2 ^ 2
PRINT -2 ^ 3
PRINT (-2) ^ 2
PRINT 0 - 2 ^ 2
A = 3
PRINT -A ^ 2
PRINT -2 * 3
PRINT 1 - -2
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(
        lines,
        vec!["-4", "-8", "4", "-4", "-9", "-6", "3"],
        "^ binds tighter than unary minus"
    );
}

/// Radix literals. Only &H was recognized, so &O17 lexed as two identifiers and
/// silently produced garbage.
#[test]
fn test_radix_literals() {
    let output = compile_and_run(
        r#"
PRINT &HFF
PRINT &O17
PRINT &B1010
PRINT &17
PRINT &HFFFFFFFF
PRINT &H10 + &H10
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(
        lines,
        vec!["255", "15", "10", "15", "-1", "32"],
        "hex, octal, binary, bare-& octal; &H values are 32-bit signed"
    );
}

/// An integer literal wider than LONG becomes a Double rather than wrapping.
/// PRINT 1000000000000001 used to print -1530494975.
#[test]
fn test_wide_integer_literals() {
    let output = compile_and_run(
        r#"
PRINT 1000000000000001
PRINT 3000000000
PRINT 2147483647
PRINT -2147483648
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(
        lines,
        vec![
            "1000000000000001",
            "3000000000",
            "2147483647",
            "-2147483648"
        ]
    );
}

/// Doubles print the shortest decimal that reads back unchanged. %g's 6
/// significant digits turned 1/3 into 0.333333 and pi into 3.14159.
#[test]
fn test_double_precision_output() {
    let output = compile_and_run(
        r#"
PRINT 1 / 3
PRINT 2 / 3
PRINT 4 * ATN(1)
PRINT SQR(2)
PRINT 3.14159
PRINT 0.1
PRINT 123456789.123456
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(
        lines,
        vec![
            ".3333333333333333",
            ".6666666666666666",
            "3.141592653589793",
            "1.4142135623730951",
            // Values that are exact at fewer digits keep their short form.
            "3.14159",
            ".1",
            "123456789.123456",
        ]
    );
}

/// A SINGLE carries ~7 significant digits, so it must not be printed with a
/// Double's digits: 3.14159! would otherwise show as 3.1415901184082.
#[test]
fn test_single_precision_output() {
    let output = compile_and_run(
        r#"
A! = 3.14159
PRINT A!
B! = 0.1
PRINT B!
C! = -3.14
PRINT ABS(C!)
PRINT CSNG(1 / 3)
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["3.14159", ".1", "3.14", ".33333334"]);
}

/// The bitwise operators must work on Double operands, which is what an
/// unsuffixed variable is.
///
/// `promote_types` had no case for AND/OR/XOR, so their result type came out
/// Double while codegen emitted the answer into EAX. PRINT then called
/// `_rt_file_print_float`, read xmm0 -- still holding the left operand -- and
/// the result was silently discarded:
///
///     A = 12 : B = 10 : PRINT A AND B     printed 12, not 8
///
/// Every existing test used literal operands, which are constant-folded before
/// this path is reached, so 470 tests coexisted with it.
#[test]
fn test_bitwise_operators_on_double_variables() {
    let output = compile_and_run(
        r#"
A = 12 : B = 10
PRINT A AND B
PRINT A OR B
PRINT A XOR B
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["8", "14", "6"], "AND/OR/XOR over Double");
}

/// The same through every type, and through assignment as well as PRINT --
/// the result was discarded identically in both.
#[test]
fn test_bitwise_operators_across_types() {
    let output = compile_and_run(
        r#"
A% = 12 : B% = 10
PRINT A% AND B%
C& = 12 : D& = 10
PRINT C& AND D&
E = 12 : F = 10
G = E AND F
PRINT G
H% = E AND F
PRINT H%
IF (E AND F) = 8 THEN PRINT "cond-ok" ELSE PRINT "cond-bad"
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(
        lines,
        vec!["8", "8", "8", "8", "cond-ok"],
        "INTEGER, LONG, Double assignment, narrowing, and IF condition"
    );
}

/// The neighbouring operators in `promote_types` must not shift: `\`, MOD and
/// the comparisons already returned Long and were correct.
#[test]
fn test_non_bitwise_operators_on_doubles_are_unchanged() {
    let output = compile_and_run(
        r#"
A = 12 : B = 10
PRINT A \ B
PRINT A MOD B
PRINT A + B
PRINT A / B
PRINT A = B
PRINT A > B
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["1", "2", "22", "1.2", "0", "-1"]);
}

/// Operator precedence must match LANGREF's own table.
///
/// It listed `NOT` (6) as binding tighter than `AND` (7), and `OR` and `XOR`
/// together at 8 -- but the parser gave `XOR` its own level tighter than `AND`,
/// and parsed `NOT`'s operand at the caller's precedence so that `NOT` swallowed
/// whatever followed it. Both are silent wrong answers, not rejections.
#[test]
fn test_logical_operator_precedence() {
    let output = compile_and_run(
        r#"
A% = 0 : B% = 0
PRINT NOT A% AND B%
PRINT (NOT A%) AND B%
PRINT -1 OR 0 XOR -1
PRINT (-1 OR 0) XOR -1
PRINT 1 AND 0 OR 1
PRINT 12 XOR 10 AND 6
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    // NOT binds tighter than AND, so the first two agree.
    assert_eq!(&lines[0..2], &["0", "0"], "NOT binds tighter than AND");
    // OR and XOR share a level and associate left to right, so these agree too.
    assert_eq!(&lines[2..4], &["0", "0"], "OR and XOR share a level");
    // AND binds tighter than OR: (1 AND 0) OR 1 = 1.
    assert_eq!(lines[4], "1", "AND binds tighter than OR");
    // AND binds tighter than XOR: 12 XOR (10 AND 6) = 12 XOR 2 = 14.
    assert_eq!(lines[5], "14", "AND binds tighter than XOR");
}

/// `NOT` still binds looser than a comparison, which is the form that matters.
#[test]
fn test_not_binds_looser_than_comparison() {
    let output = compile_and_run(
        r#"
A = 1 : B = 2
IF NOT A = B THEN PRINT "not-equal" ELSE PRINT "equal"
IF NOT A < B THEN PRINT "not-less" ELSE PRINT "less"
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    // NOT (A = B): A <> B, so NOT 0 = -1, true.
    assert_eq!(
        lines[0], "not-equal",
        "NOT groups over the whole comparison"
    );
    // NOT (A < B): A < B, so NOT -1 = 0, false.
    assert_eq!(lines[1], "less");
}

/// Equal-precedence operators associate left to right, `^` included.
///
/// `^` was right-associative, so `2 ^ 3 ^ 2` gave 512 where GW-BASIC and
/// QuickBASIC give 64. Unary minus still binds looser than `^`.
#[test]
fn test_operator_associativity() {
    let output = compile_and_run(
        r#"
PRINT 2 ^ 3 ^ 2
PRINT -2 ^ 2
PRINT 2 ^ -2
PRINT 100 - 10 - 5
PRINT 100 / 10 / 5
PRINT 2 ^ 3 ^ 2 ^ 1
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "64", "(2^3)^2, not 2^(3^2)");
    assert_eq!(lines[1], "-4", "^ binds tighter than unary minus");
    assert_eq!(lines[2], ".25", "a negative exponent still parses");
    assert_eq!(lines[3], "85", "subtraction is left to right");
    assert_eq!(lines[4], "2", "division is left to right");
    assert_eq!(lines[5], "64", "((2^3)^2)^1");
}

/// `EQV` and `IMP` are the two remaining logical operators.
///
/// Neither was a keyword, so `PRINT 1 EQV 1` printed `101` -- the items `1`,
/// an undefined variable named EQV, and `1`. A wrong answer with no
/// diagnostic, in the same family as the bitwise bugs.
#[test]
fn test_eqv_and_imp() {
    let output = compile_and_run(
        r#"
PRINT -1 EQV -1
PRINT -1 EQV 0
PRINT 0 EQV 0
PRINT 12 EQV 10
PRINT -1 IMP -1
PRINT -1 IMP 0
PRINT 0 IMP -1
PRINT 0 IMP 0
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    // EQV is bitwise equivalence: NOT (a XOR b).
    assert_eq!(&lines[0..4], &["-1", "0", "-1", "-7"], "EQV");
    // IMP is implication: (NOT a) OR b.
    assert_eq!(&lines[4..8], &["-1", "0", "-1", "-1"], "IMP");
}

/// They sit below OR and XOR, and IMP below EQV, as GW-BASIC orders them.
#[test]
fn test_eqv_and_imp_precedence() {
    let output = compile_and_run(
        r#"
A% = 0 : B% = 0
PRINT A% EQV B% OR B%
PRINT (A% EQV B%) OR B%
PRINT A% EQV (B% OR B%)
PRINT -1 IMP 0 EQV 0
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    // OR binds tighter, so the first two disagree and the first matches the third.
    assert_eq!(lines[0], lines[2], "OR binds tighter than EQV");
    // -1 IMP (0 EQV 0) = -1 IMP -1 = -1
    assert_eq!(lines[3], "-1", "EQV binds tighter than IMP");
}
