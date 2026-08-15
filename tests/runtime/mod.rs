//! Structural checks on the two assembly runtimes.
//!
//! `runtime/sysv/` and `runtime/win64-native/` are parallel hand-written
//! implementations that codegen calls by the same names, chosen with a `cfg`
//! at compile time. Only one of them can be exercised by running programs on
//! any given host, so the other is checked structurally here: a helper added
//! to one and forgotten in the other would otherwise surface as a link error
//! on the platform CI builds but nobody develops on.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use std::collections::BTreeSet;

/// Every `.globl` symbol exported by one runtime tree.
fn exported_symbols(dir: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {dir}: {e}"));
    for entry in entries {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_none_or(|e| e != "s") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("readable .s file");
        for line in text.lines() {
            if let Some(sym) = line.trim().strip_prefix(".globl ") {
                names.insert(sym.trim().to_string());
            }
        }
    }
    names
}

/// Helpers that legitimately exist on only one platform.
///
/// The Win64 runtime writes through Win32 handles, which have to be fetched
/// once at startup; System V uses libc streams and needs no equivalent.
const WIN64_ONLY: &[&str] = &["_rt_init_console", "_rt_init_input"];

#[test]
fn test_runtimes_export_the_same_helpers() {
    let sysv = exported_symbols("src/runtime/sysv");
    let win64 = exported_symbols("src/runtime/win64-native");

    assert!(
        sysv.len() > 20,
        "only found {} symbols in the sysv runtime; the scan is probably broken",
        sysv.len()
    );

    let missing_in_win64: Vec<&String> = sysv.difference(&win64).collect();
    assert!(
        missing_in_win64.is_empty(),
        "these helpers exist in the System V runtime but not the Win64 one: {missing_in_win64:?}"
    );

    let extra_in_win64: Vec<&String> = win64
        .difference(&sysv)
        .filter(|s| !WIN64_ONLY.contains(&s.as_str()))
        .collect();
    assert!(
        extra_in_win64.is_empty(),
        "these helpers exist in the Win64 runtime but not the System V one: {extra_in_win64:?}"
    );
}

/// Message lengths must be computed by the assembler, never hand-counted.
///
/// A hand-counted length was wrong by one and made WriteFile emit a stray
/// byte; a commit "fixing" it changed the correct value to the incorrect one.
#[test]
fn test_message_lengths_are_computed() {
    for dir in ["src/runtime/sysv", "src/runtime/win64-native"] {
        for entry in std::fs::read_dir(dir).expect("readable runtime directory") {
            let path = entry.expect("readable directory entry").path();
            if path.extension().is_none_or(|e| e != "s") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("readable .s file");
            for (i, line) in text.lines().enumerate() {
                let trimmed = line.trim();
                if !trimmed.starts_with(".equ ") || !trimmed.contains("_len") {
                    continue;
                }
                assert!(
                    trimmed.contains(". -"),
                    "{}:{}: length should be computed with `. - label`, not hand-counted: {}",
                    path.display(),
                    i + 1,
                    trimmed
                );
            }
        }
    }
}
