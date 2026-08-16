//! The megatest: one large program exercising most of the supported language.
//!
//! The per-feature suites each compile a program containing almost nothing
//! else, which proves a feature works in isolation and nothing more. This one
//! compiles records, arrays, procedures, DATA, GOSUB targets, sequential files
//! and random files into a single executable, so it also covers what only
//! breaks when they coexist: global versus local storage, the static layout of
//! `.bss`, a frame large enough to matter, and a DATA pointer threaded through
//! control flow that jumps around it.
//!
//! `mega.bas` is its own assertion harness -- see the comment at its head --
//! so a regression names itself here rather than showing up as a diff between
//! two walls of output.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run_with_files;

/// The program is kept as a `.bas` file rather than a Rust string literal so
/// it stays readable, and editable, as BASIC.
const MEGA: &str = include_str!("mega.bas");

/// Every check in the program must pass, and the count must be the expected
/// one: a program that stopped early would otherwise report no failures.
#[test]
fn test_megatest_passes_every_check() {
    let (output, _tmp) = compile_and_run_with_files(MEGA, |_| Ok(()))
        .expect("the megatest must compile and run to completion");

    let failures: Vec<&str> = output.lines().filter(|l| l.starts_with("FAIL ")).collect();
    assert!(
        failures.is_empty(),
        "{} check(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );

    assert!(
        output.contains("FAILED0"),
        "the program's own tally disagrees; output was:\n{}",
        output
    );

    // Guards against a program that exits early: the count only ever goes up,
    // so this is a floor, not an exact match to be churned on every addition.
    let passed: usize = output
        .lines()
        .find_map(|l| l.strip_prefix("PASSED"))
        .and_then(|n| n.trim().parse().ok())
        .expect("the megatest must report its tally");
    assert!(
        passed >= 200,
        "only {} checks ran; the program stopped early",
        passed
    );
}

/// PRINT USING has no file form -- `PRINT #n, USING` is rejected -- so the
/// formatted output cannot be read back by the program itself and is checked
/// here instead.
#[test]
fn test_megatest_print_using_block() {
    let (output, _tmp) = compile_and_run_with_files(MEGA, |_| Ok(())).unwrap();

    let block: Vec<&str> = output
        .lines()
        .skip_while(|l| *l != "USING-BEGIN")
        .skip(1)
        .take_while(|l| *l != "USING-END")
        .collect();

    assert_eq!(
        block,
        vec![
            "  3.14",     // ###.##
            "Total:  42", // literal text around a field
            " 1 and  2",  // two fields, two values
            "  +5",       // leading sign
            "1,234",      // thousands
            "   $9.50",   // floating currency
            "****7",      // asterisk fill
            "a",          // ! -- first character only
            "abcde",      // \   \ -- fixed width
            "whole",      // & -- the whole string
        ]
    );
}
