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

/// The instructions of the one loop in a single-loop program.
///
/// Found by the label *definitions*, at the start of a line: searching for
/// `.Lendfor_` anywhere finds the forward branch at the top of the loop
/// instead, which sits before the body rather than after it. And by the
/// numbered form only, since a loop whose step is not a constant also emits
/// `.Lfor_neg_N` and `.Lfor_body_N`.
///
/// It asserts there is exactly one loop rather than guessing which to take.
/// Every caller passes a program with one; a test that later grew a nested
/// loop would otherwise quietly start asserting against the wrong body.
#[track_caller]
fn sole_loop_body(asm: &str) -> String {
    let starts = label_positions(asm, ".Lfor_");
    assert_eq!(
        starts.len(),
        1,
        "this helper is for single-loop programs, but found {} loops",
        starts.len()
    );
    let after = &asm[starts[0] + 1..];
    let end = label_positions(after, ".Lendfor_")
        .first()
        .copied()
        .expect("a loop exit label");
    after[..end].to_string()
}

/// Byte offsets of the `\n<prefix><digits>:` label definitions in `asm`.
fn label_positions(asm: &str, prefix: &str) -> Vec<usize> {
    let needle = format!("\n{}", prefix);
    asm.match_indices(&needle)
        .filter(|(i, _)| {
            let rest = &asm[i + needle.len()..];
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            !digits.is_empty() && rest[digits.len()..].starts_with(':')
        })
        .map(|(i, _)| i)
        .collect()
}

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
            needle,
            source,
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

/// A scalar array element is read through a scaled index, not a computed
/// address.
///
/// `imul rax, 8` / `add rax, <base>` / `movsd xmm0, [rax]` is three
/// instructions for what x86-64 addressing does in one operand.
///
/// `add rax, QWORD PTR [rip + _arr_` is the discriminator, not the `imul`:
/// DIM emits its own `imul rax, <size>` to work out how many bytes to
/// allocate, and that one is not going anywhere.
const FLAT_ADDRESS: &str = "add rax, QWORD PTR [rip + _arr_";

#[test]
fn test_array_element_is_read_through_a_scaled_index() {
    let src = "DIM A#(10)\nI% = 3\nPRINT A#(I%)\n";
    asserts_emits(src, &["movsd xmm0, QWORD PTR [r10 + rax*8]"]);
    asserts_absent(src, &[FLAT_ADDRESS]);
}

/// Each scalar width gets its own scale.
#[test]
fn test_every_scalar_element_width_scales() {
    for (decl, elem, scale) in [
        ("DIM A%(10)", "A%", 2),
        ("DIM A&(10)", "A&", 4),
        ("DIM A!(10)", "A!", 4),
        ("DIM A#(10)", "A#", 8),
    ] {
        let src = format!("{}\nI% = 3\nPRINT {}(I%)\n", decl, elem);
        asserts_emits(&src, &[&format!("[r10 + rax*{}]", scale)]);
        asserts_absent(&src, &[FLAT_ADDRESS]);
    }
}

/// A string element is sixteen bytes, which is not a legal scale, so it keeps
/// the multiply and the flat address.
#[test]
fn test_string_array_keeps_the_multiply() {
    let src = "DIM A$(10)\nI% = 3\nA$(I%) = \"x\"\nPRINT A$(I%)\n";
    asserts_emits(src, &["imul rax, 16", FLAT_ADDRESS]);
    let out = crate::common::compile_and_run(src).expect("the program must run");
    assert_eq!(out.trim(), "x");
}

/// Neither is a record element, whose size is eight times its word count.
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
    asserts_emits(src, &["imul rax, 16", FLAT_ADDRESS]);
    let out = crate::common::compile_and_run(src).expect("the program must run");
    assert_eq!(out.trim(), "3");
}

/// The addressing change must not disturb what the elements actually hold.
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
    let got: Vec<&str> = out.lines().map(str::trim).collect();
    assert_eq!(got.len(), 2);
    assert!(got[0].starts_with("6\t3000\t3\t1.5\tv"), "got {:?}", got[0]);
    assert_eq!(got[1], "0\t4000\t0.5");
}

/// Bounds checking still catches a subscript past the end.
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

/// A multi-dimensional array folds its scale the same way, after the
/// row-major index has been accumulated.
#[test]
fn test_multidimensional_array_scales_too() {
    let src = "\
DIM A#(3, 3)
A#(1, 2) = 7
PRINT A#(1, 2)
";
    asserts_emits(src, &["[r10 + rax*8]"]);
    let out = crate::common::compile_and_run(src).expect("the program must run");
    assert_eq!(out.trim(), "7");
}

/// An INTEGER counter counts in integers, not in doubles.
///
/// The loop machinery used to work entirely in Double whatever the control
/// variable's type was, which was both slower and wrong -- see
/// `control::test_for_loop_control_variable_types`.
#[test]
fn test_integer_for_loop_counts_in_integers() {
    let src = "FOR I% = 1 TO 3\nPRINT I%\nNEXT I%\n";
    asserts_emits(
        src,
        &[
            "movsx eax, WORD PTR [rip + _var_I_I + 0]",
            "cmp eax, 3",
            // Sixteen-bit, so that stepping past INTEGER's range sets the
            // overflow flag rather than silently wrapping.
            "add ax, 1",
            "mov WORD PTR [rip + _var_I_I + 0], ax",
        ],
    );
    asserts_absent(src, &["ucomisd", "addsd", "cvttsd2si"]);
}

/// Stepping an INTEGER counter past its range is reported, not wrapped.
///
/// Wrapping means the counter never passes its limit, so the loop runs
/// forever. GW-BASIC reports Overflow here and so does this.
#[test]
fn test_integer_counter_overflow_is_reported() {
    let src = "FOR I% = 32764 TO 32767\nPRINT I%\nNEXT I%\n";
    asserts_emits(src, &["jo .Lerr_ovf"]);

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

/// A loop that calls nothing keeps its counter in a register.
///
/// A counter in memory makes the loop-carried dependency a store followed by
/// the next iteration's load of the same address -- around ten cycles of
/// store-to-load forwarding that nothing else in the loop can hide.
#[test]
fn test_call_free_loop_promotes_its_counter() {
    let src = "T# = 0\nFOR I = 1 TO 10\nT# = T# + I\nNEXT I\nPRINT T#\n";
    asserts_emits(src, &["addsd xmm4,", "movapd xmm4, xmm0"]);
    // Nothing reads or writes the counter's storage inside the loop; it is
    // written back once, after it.
    let asm = compile_to_asm(src).expect("must compile");
    assert_eq!(
        asm.matches("_var_I + 0").count(),
        1,
        "the counter's storage should be touched once, at the writeback:\n{}",
        asm
    );
}

/// An INTEGER counter goes to a callee-saved GPR, which has to be saved.
#[test]
fn test_integer_counter_register_is_saved_and_restored() {
    let src = "T% = 0\nFOR I% = 1 TO 10\nT% = T% + I%\nNEXT I%\nPRINT T%\n";
    asserts_emits(
        src,
        &[
            "mov QWORD PTR [rsp], r12",
            "add r12w, 1",
            "mov WORD PTR [rip + _var_I_I + 0], r12w",
            "mov r12, QWORD PTR [rsp]",
        ],
    );
}

/// Nested promotable loops get different registers.
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
    asserts_emits(src, &["xmm4", "xmm5"]);
    let out = crate::common::compile_and_run(src).expect("the program must run");
    assert_eq!(out.trim(), "60", "(1+2+3) * (1+2+3+4)");
}

/// A loop that calls anything keeps its counter in memory.
///
/// System V has no callee-saved XMM register, so a Double counter would not
/// survive the call; and a called procedure can reach a module-level counter
/// by name. PRINT is a call, so this is most loops.
#[test]
fn test_calling_loop_keeps_its_counter_in_memory() {
    for src in [
        // A runtime call.
        "FOR I = 1 TO 3\nPRINT I\nNEXT I\n",
        // A libc math call.
        "T# = 0\nFOR I = 1 TO 3\nT# = T# + SIN(I)\nNEXT I\nPRINT T#\n",
        // Exponentiation, which calls pow.
        "T# = 0\nFOR I = 1 TO 3\nT# = T# + I ^ 2\nNEXT I\nPRINT T#\n",
        // A string operation, which goes through the runtime.
        "S$ = \"\"\nFOR I = 1 TO 3\nS$ = S$ + \"x\"\nNEXT I\nPRINT S$\n",
    ] {
        asserts_absent(src, &["xmm4", "r12"]);
    }
}

/// Neither does a loop whose body assigns to the counter, since the register
/// would go stale and BASIC allows the assignment.
#[test]
fn test_loop_assigning_its_counter_keeps_it_in_memory() {
    // The body steps the counter itself, so it advances by three per
    // iteration: 1, 4, 7, 10, then 13, which is past the limit. Four
    // iterations and a final value of 13, both of which depend on the loop
    // reading the counter back out of memory rather than out of a register it
    // never saw the assignment through.
    let src = "\
N# = 0
FOR I = 1 TO 10
  N# = N# + 1
  I = I + 2
NEXT I
PRINT N#
PRINT I
";
    asserts_absent(src, &["xmm4"]);
    let out = crate::common::compile_and_run(src).expect("the program must run");
    let lines: Vec<&str> = out.trim().lines().collect();
    assert_eq!(lines[0], "4", "the body ran four times");
    assert_eq!(lines[1], "13", "the body's own step took effect");
}

/// EXIT FOR leaves through the loop's own exit, where the register is written
/// back, so the counter is correct afterwards.
#[test]
fn test_exit_for_writes_a_promoted_counter_back() {
    let out = crate::common::compile_and_run(
        "N# = 0\nFOR I = 1 TO 100\nN# = N# + 1\nIF I = 4 THEN EXIT FOR\nNEXT I\nPRINT N#\nPRINT I\n",
    )
    .expect("the program must run");
    let lines: Vec<&str> = out.trim().lines().collect();
    assert_eq!(lines[0], "4", "the body ran four times");
    assert_eq!(lines[1], "4", "the counter survived EXIT FOR");
}

/// A constant step settles the loop's direction at compile time.
///
/// The step's sign decides whether the loop exits above or below its limit.
/// It used to be re-tested on every iteration -- three loads, an xorpd, a
/// ucomisd and two branches -- even when it was written into the program.
#[test]
fn test_constant_step_needs_no_direction_test() {
    for src in [
        "FOR I% = 1 TO 3\nPRINT I%\nNEXT I%\n",
        "FOR I% = 3 TO 1 STEP -1\nPRINT I%\nNEXT I%\n",
        "FOR X# = 0 TO 1 STEP 0.5\nPRINT X#\nNEXT X#\n",
    ] {
        asserts_absent(src, &[".Lfor_neg_"]);
    }
}

/// ...and a step that is not constant still gets one.
#[test]
fn test_runtime_step_keeps_its_direction_test() {
    let src = "S% = 2\nFOR I% = 0 TO 6 STEP S%\nPRINT I%\nNEXT I%\n";
    asserts_emits(src, &[".Lfor_neg_"]);
}

/// A constant limit and step need no frame slots either: they are the
/// compare's and the add's own operands.
#[test]
fn test_constant_bounds_need_no_frame_slots() {
    let asm = compile_to_asm("FOR I% = 1 TO 3\nPRINT I%\nNEXT I%\n").expect("must compile");
    assert!(
        asm.contains("sub rsp, 0        # STACK_RESERVE"),
        "expected an empty frame, since nothing needs a slot:\n{}",
        asm
    );
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

/// Each runtime helper is emitted into a section of its own.
///
/// A linker discards an unreferenced *section*, never an unreferenced label,
/// so this is what lets the linker drop the helpers a program does not call.
/// Without it all 69 went into every binary.
///
/// Counted against the exported helpers, not against every label: a COMDAT
/// section is identified by a symbol and that symbol has to be external, so
/// the handful of helpers that are not `.globl` deliberately ride in the
/// section of whichever helper precedes them.
///
/// The two object formats spell the directive differently -- ELF names the
/// section after the helper, COFF repeats `.text` and names the helper as the
/// COMDAT symbol -- so the match is on what they have in common.
#[test]
fn test_runtime_helpers_get_their_own_sections() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let bas = tmp.path().join("t.bas");
    std::fs::write(&bas, "PRINT \"hi\"\n").expect("write");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_xbasic64"))
        .arg("-S")
        .arg(&bas)
        .arg("-o")
        .arg(tmp.path().join("t"))
        .output()
        .expect("run compiler");
    assert!(out.status.success());
    let asm = std::fs::read_to_string(tmp.path().join("t.s")).expect("read asm");

    let exported = asm.lines().filter(|l| l.starts_with(".globl _rt_")).count();
    let sections = asm
        .lines()
        .filter(|l| l.starts_with(".section .text") && l.contains("_rt_"))
        .count();
    assert!(
        exported > 50,
        "expected the whole runtime, saw {} exported helpers",
        exported
    );
    assert_eq!(
        sections, exported,
        "every exported helper needs a section of its own"
    );
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
                if t.starts_with("_rt_") && t.ends_with(':') {
                    if let Some(prev) = &previous {
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

/// A loop's accumulator lives in a register, not in its storage.
///
/// `T = T + ...` has the same store-then-reload loop-carried dependency the
/// control variable had, and usually a longer one, since the add sits on the
/// critical path too. A hundred million iterations of `C# = C# + 1` went from
/// 0.18s to 0.04s.
#[test]
fn test_loop_accumulator_is_promoted() {
    let src = "C# = 0\nFOR I = 1 TO 10\nC# = C# + 1\nNEXT I\nPRINT C#\n";
    let asm = compile_to_asm(src).expect("must compile");
    let body = sole_loop_body(&asm);
    assert!(
        !body.contains("_var_C_D"),
        "the accumulator's storage must not be touched inside the loop:\n{}",
        body
    );
    // Loaded before the loop and written back after it.
    asserts_emits(src, &["movsd xmm5, QWORD PTR [rip + _var_C_D + 0]"]);
    asserts_emits(src, &["movsd QWORD PTR [rip + _var_C_D + 0], xmm5"]);
}

/// Only so many, and the rest keep working in memory.
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
    let asm = compile_to_asm(src).expect("must compile");
    let body = sole_loop_body(&asm);
    assert!(
        body.contains("_var_D_D"),
        "the fourth accumulator should have stayed in memory:\n{}",
        body
    );
    for name in ["_var_A_D", "_var_B_D", "_var_C_D"] {
        assert!(!body.contains(name), "{} should be in a register", name);
    }
    let out = crate::common::compile_and_run(src).expect("must run");
    let got: Vec<&str> = out.trim().lines().collect();
    assert_eq!(got, vec!["4", "8", "12", "16"], "all four still accumulate");
}

/// An integer accumulator takes a callee-saved GPR, so it must be saved.
#[test]
fn test_integer_accumulator_register_is_saved() {
    let src = "S% = 0\nFOR I% = 1 TO 5\nS% = S% + I%\nNEXT I%\nPRINT S%\n";
    asserts_emits(src, &["save an accumulator register"]);
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

/// A loop that calls anything keeps its accumulator in memory, for the same
/// reason it keeps its counter there.
#[test]
fn test_calling_loop_does_not_promote_accumulators() {
    let src = "T# = 0\nFOR I = 1 TO 3\nT# = T# + I\nPRINT T#\nNEXT I\n";
    let asm = compile_to_asm(src).expect("must compile");
    let body = sole_loop_body(&asm);
    assert!(
        body.contains("_var_T_D"),
        "PRINT is a call, so nothing may be promoted:\n{}",
        body
    );
}

/// The limit is captured before the loop, so assigning it inside does not
/// extend the loop -- and the assignment still takes effect.
#[test]
fn test_promoted_accumulator_does_not_change_the_limit() {
    let out = crate::common::compile_and_run(
        "N = 3\nK# = 0\nFOR I = 1 TO N\nN = 100\nK# = K# + 1\nNEXT I\nPRINT K#\nPRINT N\n",
    )
    .expect("must run");
    let got: Vec<&str> = out.trim().lines().collect();
    assert_eq!(got, vec!["3", "100"]);
}

/// A loop hoists the array descriptors its body keeps re-reading.
///
/// The element pointer and the bounds cannot change while the loop runs, yet
/// every subscript fetched them again: two loads for the pointer, since the
/// null check and the address each wanted it, and one per bound compare.
#[test]
fn test_loop_hoists_array_descriptors() {
    let src = "\
DIM A(5)
T# = 0
FOR I = 0 TO 5
  T# = T# + A(I)
NEXT I
PRINT T#
";
    let asm = compile_to_asm(src).expect("must compile");
    let body = sole_loop_body(&asm);
    assert!(
        !body.contains("_arr_A"),
        "the descriptor should be in registers inside the loop:\n{}",
        body
    );
    // Loaded once, before the loop.
    asserts_emits(src, &["mov r12, QWORD PTR [rip + _arr_A + 0]"]);
}

/// The null check stays inside the loop even though the pointer is hoisted.
///
/// Hoisting the *check* as well would report an array that a loop running
/// zero times never touched.
#[test]
fn test_undim_check_is_not_hoisted_out_of_the_loop() {
    let src = "\
DIM A(5)
T# = 0
FOR I = 0 TO 5
  T# = T# + A(I)
NEXT I
PRINT T#
";
    let asm = compile_to_asm(src).expect("must compile");
    let body = sole_loop_body(&asm);
    assert!(
        body.contains(".Lerr_undim"),
        "the DIM check belongs with the access:\n{}",
        body
    );

    // ...and it still fires when it should.
    let run =
        crate::common::compile_and_run_raw("PRINT A(0)\nDIM A(3)\n", "").expect("should compile");
    assert!(run.stderr.contains("Array used before DIM"));
}

/// Bounds checking survives the hoist, against the hoisted bound.
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
    let got: Vec<&str> = out.trim().lines().collect();
    assert_eq!(got, vec!["9", "23", "90"]);
}

/// Every call in generated code must be made with the stack 16-byte aligned.
///
/// `tests/runtime` already checks this for the hand-written runtime; nothing
/// checked the code the compiler emits. MID$ and INSTR each pushed three
/// callee-saved registers and never padded, so every call in those sequences
/// was 8 bytes out. The leaf helpers tolerated it, which is why it went
/// unnoticed -- until an argument check jumped to an error trampoline, whose
/// `_rt_error` calls into the C library. On Linux that survived; on Windows it
/// was an access violation, and the Linux CI could not see it.
///
/// The model is a linear scan of `main`'s body, which is sound here because
/// each program below is straight-line: no loops, no branches of its own. A
/// conditional jump to an error trampoline is checked too, since the trampoline
/// touches no stack before calling and so inherits the jump site's alignment.
#[test]
fn test_generated_calls_are_stack_aligned() {
    let programs = [
        ("MID$ three args", "PRINT MID$(\"abcdef\", 2, 3)\n"),
        ("MID$ two args", "PRINT MID$(\"abcdef\", 2)\n"),
        ("INSTR two args", "PRINT INSTR(\"abc\", \"b\")\n"),
        ("INSTR three args", "PRINT INSTR(2, \"abc\", \"b\")\n"),
        ("LEFT$", "PRINT LEFT$(\"abc\", 2)\n"),
        ("RIGHT$", "PRINT RIGHT$(\"abc\", 2)\n"),
        ("STRING$", "PRINT STRING$(3, \"x\")\n"),
        ("SPACE$", "PRINT SPACE$(3)\n"),
        ("CHR$", "PRINT CHR$(65)\n"),
        ("ASC", "PRINT ASC(\"A\")\n"),
        (
            "nested",
            "PRINT MID$(LEFT$(\"abcdef\", 5), INSTR(\"abc\", \"b\"), 2)\n",
        ),
        ("concat", "PRINT MID$(\"abc\", 1, 2) + RIGHT$(\"xyz\", 1)\n"),
        (
            "array subscript",
            "DIM A(3)\nA(1) = 2\nPRINT MID$(\"abcdef\", A(1), 2)\n",
        ),
    ];

    let mut problems = Vec::new();
    for (what, source) in programs {
        let asm = compile_to_asm(source).expect("must compile");
        // `offset` is rsp modulo 16 measured from the ABI's state on entry.
        // After `push rbp` it is 0, and every call must see it at 0.
        let mut offset: i64 = 8;
        let mut in_main = false;

        for raw in asm.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line == "main:" {
                in_main = true;
                offset = 8;
                continue;
            }
            if !in_main {
                continue;
            }
            // The body ends at the first `ret`; the trampolines past it are
            // reached by jumps, so a linear scan cannot model them.
            if line == "ret" {
                break;
            }

            if line.starts_with("push ") || line.starts_with("pop ") {
                offset = (offset + 8) % 16;
            } else if let Some(n) = line.strip_prefix("sub rsp, ") {
                offset = (offset - n.trim().parse::<i64>().expect("literal")).rem_euclid(16);
            } else if let Some(n) = line.strip_prefix("add rsp, ") {
                offset = (offset + n.trim().parse::<i64>().expect("literal")) % 16;
            } else if line == "leave" {
                offset = 8;
            } else if let Some(target) = line.strip_prefix("call ") {
                if offset != 0 {
                    problems.push(format!("{what}: call {target} with rsp % 16 == {offset}"));
                }
            } else if line.starts_with('j') && line.contains(".Lerr_") && offset != 0 {
                problems.push(format!("{what}: {line} with rsp % 16 == {offset}"));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "{} misaligned site(s):\n{}",
        problems.len(),
        problems.join("\n")
    );
}
