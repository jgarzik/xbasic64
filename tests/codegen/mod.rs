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
