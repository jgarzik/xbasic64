//! Assertions on the assembly the compiler emits, rather than on what a
//! compiled program prints.
//!
//! The optimizations these cover are invisible from the outside: a program
//! that stops folding a constant, stops branching on flags, or starts
//! rematerializing an address computes exactly the same answers, just with
//! more instructions. Every other suite in this tree would stay green while
//! the generated code silently got worse, which is what these are for.
//!
//! They are pattern assertions, not golden files. A golden `.s` would have to
//! be regenerated after any change at all, so it would be rubber-stamped
//! rather than read; naming the one instruction that must or must not appear
//! says what the test is actually about.
//!
//! [`compile_to_asm`] returns only the generated portion, cut before the
//! runtime's banner -- the runtime is thousands of hand-written instructions
//! and contains an example of nearly everything asserted here.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_to_asm;

/// Every one of `needles` must appear in the generated assembly.
#[track_caller]
fn asserts_emits(source: &str, needles: &[&str]) {
    let asm = compile_to_asm(source).expect("the program must compile");
    for needle in needles {
        assert!(
            asm.contains(needle),
            "expected `{}` in the generated assembly for:\n{}\n--- got ---\n{}",
            needle,
            source,
            asm
        );
    }
}

/// None of `needles` may appear in the generated assembly.
#[track_caller]
fn asserts_absent(source: &str, needles: &[&str]) {
    let asm = compile_to_asm(source).expect("the program must compile");
    for needle in needles {
        assert!(
            !asm.contains(needle),
            "did not expect `{}` in the generated assembly for:\n{}\n--- got ---\n{}",
            source,
            needle,
            asm
        );
    }
}

/// An argument register loaded with a small constant is written narrow.
///
/// `mov edi, 1` and `mov rdi, 1` put the same value in rdi, because writing a
/// 32-bit register zeroes the upper half; the narrow encoding is two bytes
/// shorter. PRINT's separator handling is the easiest site to reach from
/// BASIC -- the comma is passed as an immediate character code.
#[test]
fn test_small_arguments_use_32_bit_registers() {
    let src = "PRINT 1, 2\n";
    let asm = compile_to_asm(src).expect("the program must compile");

    assert!(
        asm.lines().any(|l| l.trim_start().starts_with("mov e")
            || l.trim_start().starts_with("mov r8d")
            || l.trim_start().starts_with("mov r9d")),
        "expected a narrow immediate load:\n{}",
        asm
    );

    // No argument register should be loaded with a wide `mov` for a value
    // that fits in 32 bits. Only the six System V / four Win64 argument
    // registers are checked, since those are what emit_arg_imm writes.
    for line in asm.lines().map(str::trim_start) {
        for reg in ["rdi", "rsi", "rdx", "rcx", "r8", "r9"] {
            let wide = format!("mov {}, ", reg);
            if let Some(rest) = line.strip_prefix(&wide) {
                if let Ok(v) = rest.parse::<i64>() {
                    assert!(
                        !(0..=u32::MAX as i64).contains(&v),
                        "`{}` should have used the 32-bit register:\n{}",
                        line,
                        asm
                    );
                }
            }
        }
    }
}

/// A string literal's length is a small constant, so it too is written narrow.
#[test]
fn test_string_literal_length_uses_32_bit_register() {
    asserts_emits("PRINT \"hello\"\n", &["mov edx, 5"]);
    asserts_absent("PRINT \"hello\"\n", &["mov rdx, 5"]);
}

/// Reading an INTEGER already sign-extends it, so widening it is a no-op.
///
/// `movsx eax, WORD PTR [...]` leaves eax holding the sign-extended value; the
/// `movsx eax, ax` that used to follow every INTEGER-to-LONG coercion could
/// not change it.
#[test]
fn test_integer_load_is_not_sign_extended_twice() {
    let src = "\
DIM X AS INTEGER
DIM Y AS LONG
X = 7
Y = X
PRINT Y
";
    asserts_emits(src, &["movsx eax, WORD PTR"]);
    asserts_absent(src, &["movsx eax, ax"]);
}

/// ...but a 16-bit truncation that carries meaning is still emitted.
///
/// INTEGER * INTEGER is INTEGER, computed with a 32-bit `imul`, so the result
/// has to be brought back down to 16 bits before it is used as a LONG. This is
/// the case the peephole must decline, and the reason it keys on the emitted
/// instruction rather than on the static type.
#[test]
fn test_integer_arithmetic_is_still_truncated() {
    let src = "\
DIM X AS INTEGER
DIM Y AS LONG
X = 300
Y = X * X
PRINT Y
";
    asserts_emits(src, &["movsx eax, ax"]);
}

/// A Double constant is named in memory, not built as a 64-bit immediate.
///
/// `mov rax, <imm64>` plus `movq xmm0, rax` is fifteen bytes and costs a
/// scratch register and a register round trip; a pool reference is eight.
#[test]
fn test_double_constants_come_from_the_pool() {
    let src = "X# = 3.5\nPRINT X#\n";
    asserts_emits(src, &["_f64_0: .quad 0x400C000000000000"]);
    asserts_absent(src, &["mov rax, 0x400C000000000000", "movq xmm0, rax"]);
}

/// Equal constants share one pool entry.
#[test]
fn test_double_constant_pool_is_deduplicated() {
    let asm = compile_to_asm("X# = 2.5 + 2.5\nPRINT X#\n").expect("the program must compile");
    let entries = asm.lines().filter(|l| l.starts_with("_f64_")).count();
    assert_eq!(entries, 1, "expected one pooled constant:\n{}", asm);
}

/// Zero is shorter still as an idiom, and breaks rather than makes a
/// dependency on whatever xmm0 last held.
#[test]
fn test_double_zero_is_an_xor() {
    let src = "X# = 0.0\nPRINT X#\n";
    asserts_emits(src, &["xorpd xmm0, xmm0"]);
}

/// A constant right operand needs no spill: it cannot clobber the left one.
///
/// The general path parks the left operand on the stack while the right is
/// evaluated, because evaluating the right can call a function. A literal
/// cannot, so the whole `sub rsp` / store / reload / `add rsp` sequence -- and
/// the `cvtsi2sd` that used to widen an integer literal -- goes away.
#[test]
fn test_double_operand_folds_into_the_instruction() {
    let src = "Y# = 2\nX# = Y# * 3\nPRINT X#\n";
    asserts_emits(src, &["mulsd xmm0, QWORD PTR [rip + _f64_"]);
    asserts_absent(src, &["cvtsi2sd", "sub rsp, 16"]);
}

/// The same for integers, where the constant becomes an immediate.
#[test]
fn test_integer_operand_becomes_an_immediate() {
    let src = "Y% = 2\nX% = Y% + 5\nPRINT X%\n";
    asserts_emits(src, &["add eax, 5"]);
    asserts_absent(src, &["sub rsp, 16"]);
}

/// Dividing by a constant zero is decided at compile time, not re-tested.
///
/// The runtime test (`movq r11, xmm1` / `add r11, r11` / `jz`) exists to catch
/// a divisor only known at run time. When the divisor is written into the
/// program the branch is not a branch, and the trap is unconditional -- but it
/// is still a trap.
#[test]
fn test_constant_zero_divisor_traps_without_testing() {
    let src = "Y# = 5\nX# = Y# / 0\nPRINT X#\n";
    asserts_absent(src, &["movq r11, xmm1"]);

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
    asserts_absent("Y# = 5\nX# = Y# / 2\nPRINT X#\n", &["movq r11, xmm1"]);
}

/// Every operator that takes the constant path must still compute correctly.
///
/// The shape assertions above say the fast path fired; this says it was right.
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

/// A CONST is a constant here too, not just a literal.
#[test]
fn test_named_constants_take_the_constant_path() {
    let src = "CONST THREE = 3\nY# = 2\nX# = Y# * THREE\nPRINT X#\n";
    asserts_emits(src, &["mulsd xmm0, QWORD PTR [rip + _f64_"]);
    let out = crate::common::compile_and_run(src).expect("the program must run");
    assert_eq!(out.trim(), "6");
}

/// A comparison used as a condition branches on its own flags.
///
/// The `setcc` / `movzx` / `neg` / `test` sequence built a -1/0 word only to
/// compare it against zero, when the comparison had already set exactly the
/// flags the jump reads.
#[test]
fn test_if_branches_on_comparison_flags() {
    let src = "A% = 5\nB% = 3\nIF A% > B% THEN PRINT \"y\"\n";
    asserts_emits(src, &["cmp eax, ecx", "jle "]);
    asserts_absent(src, &["setg", "neg eax", "test eax, eax"]);
}

/// The same for a Double comparison, which reads the flags unsigned because
/// ucomisd reports through CF and ZF.
#[test]
fn test_double_comparison_branches_unsigned() {
    let src = "X# = 1.5\nIF X# < 2 THEN PRINT \"y\"\n";
    asserts_emits(src, &["ucomisd xmm0, QWORD PTR [rip + _f64_", "jae "]);
    asserts_absent(src, &["setb", "neg eax"]);
}

/// ...and for strings, where _rt_strcmp's memcmp-style result reads signed.
#[test]
fn test_string_comparison_branches_on_strcmp() {
    let src = "S$ = \"abc\"\nIF S$ < \"abd\" THEN PRINT \"y\"\n";
    asserts_emits(src, &["call _rt_strcmp", "test eax, eax", "jge "]);
    asserts_absent(src, &["setl", "neg eax"]);
}

/// Every loop form takes the same path, in both branch senses.
#[test]
fn test_every_loop_form_branches_on_flags() {
    for src in [
        "I% = 0\nWHILE I% < 3\nI% = I% + 1\nWEND\n",
        "I% = 0\nDO WHILE I% < 3\nI% = I% + 1\nLOOP\n",
        "I% = 0\nDO UNTIL I% >= 3\nI% = I% + 1\nLOOP\n",
        "I% = 0\nDO\nI% = I% + 1\nLOOP WHILE I% < 3\n",
        "I% = 0\nDO\nI% = I% + 1\nLOOP UNTIL I% >= 3\n",
    ] {
        asserts_emits(src, &["cmp eax, 3"]);
        asserts_absent(src, &["setl", "setge", "neg eax"]);
    }
}

/// Each of those loops must still run the right number of times.
///
/// The post-test forms branch backwards, so their senses are inverted against
/// the pre-test ones -- the easiest thing in this change to get backwards.
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
/// This is why the fast path is opt-in per site rather than a change to
/// gen_binary_expr: BASIC's AND is bitwise, so `A > B AND C > D` needs both
/// halves materialized, and assigning or printing a comparison needs a value.
#[test]
fn test_comparison_as_a_value_is_still_materialized() {
    let src = "A% = 5\nB% = 3\nN% = (A% > B%)\nPRINT N%\n";
    asserts_emits(src, &["setg", "neg eax"]);
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

/// The truncation above is not decorative: the program must still wrap.
///
/// 300 * 300 is 90000, which does not fit in an INTEGER; GW-BASIC's INTEGER
/// arithmetic wraps to 16 bits, giving 24464.
#[test]
fn test_integer_multiply_wraps_to_16_bits() {
    let src = "\
DIM X AS INTEGER
DIM Y AS LONG
X = 300
Y = X * X
PRINT Y
";
    let out = crate::common::compile_and_run(src).expect("the program must run");
    assert_eq!(out.trim(), "24464");
}
