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

/// Every `.s` file in one runtime tree, as (path, text).
fn runtime_sources(dir: &str) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {dir}: {e}"));
    for entry in entries {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_none_or(|e| e != "s") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("readable .s file");
        files.push((path.display().to_string(), text));
    }
    files.sort();
    assert!(!files.is_empty(), "no .s files found in {dir}");
    files
}

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
/// Empty, and worth keeping that way: both trees now answer to the same
/// `_rt_platform_init`, each doing whatever its platform needs behind it.
/// Every name listed here is a divergence this test agrees not to notice.
const WIN64_ONLY: &[&str] = &[];

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

/// Message lengths must be computed by the assembler, never hand-counted --
/// and computed where the data ends, not somewhere further down the file.
///
/// A hand-counted length was wrong by one and made WriteFile emit a stray
/// byte; a commit "fixing" it changed the correct value to the incorrect one.
///
/// `. - label` is only right while `.` is still just past the data: `.` means
/// "here", so anything inserted in between is silently counted as part of the
/// message. Four format strings and a scratch buffer were, and `CLS` on
/// Windows wrote 75 bytes where it meant to write 7.
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
                // Both spellings of an assembler constant: `.equ N, v` and
                // `N = v`. Only checking `.equ` let a hand-counted `=` through
                // in the Win64 tree.
                let trimmed = line.split('#').next().unwrap_or("").trim();
                let name = match trimmed.strip_prefix(".equ ") {
                    Some(rest) => rest.split(',').next().unwrap_or("").trim(),
                    None => match trimmed.split_once('=') {
                        Some((lhs, _)) if !lhs.trim().contains(char::is_whitespace) => lhs.trim(),
                        _ => continue,
                    },
                };
                if !name.ends_with("_len") {
                    continue;
                }
                assert!(
                    trimmed.contains(". -"),
                    "{}:{}: length should be computed with `. - label`, not hand-counted: {}",
                    path.display(),
                    i + 1,
                    trimmed
                );

                // `. - label` measures from the label to *here*, so the only
                // safe place for it is immediately after the label's data,
                // with nothing but comments in between.
                let label = trimmed
                    .rsplit_once(". -")
                    .map(|(_, rest)| rest.trim())
                    .expect("just asserted the line computes `. - label`");
                let previous = text
                    .lines()
                    .take(i)
                    .map(|l| l.split('#').next().unwrap_or("").trim())
                    .filter(|l| !l.is_empty())
                    .last()
                    .unwrap_or("");
                assert!(
                    previous.starts_with(&format!("{label}:")),
                    "{}:{}: `{}` must sit directly after {}'s data, but {:?} intervenes -- \
                     everything in between is counted as part of the message",
                    path.display(),
                    i + 1,
                    trimmed,
                    label,
                    previous
                );
            }
        }
    }
}

/// Both ABIs require `rsp` to be 16-byte aligned immediately before a `call`.
///
/// This is not a performance nicety: the CRT spills SSE registers with
/// `movaps`, which faults outright on a misaligned address. A wrong prologue
/// reserve in `_rt_fmt_double` -- on the path of every numeric PRINT -- crashed
/// most of the Windows test suite with STATUS_ACCESS_VIOLATION, and the same
/// class of defect sat latent in the System V tree.
///
/// On entry `rsp` is 8 (mod 16), because the caller's `call` pushed a return
/// address. Walking each function linearly is sound because none of them
/// changes `rsp` inside a branch that rejoins at a different depth; a
/// `lea rsp, ...` restores a frame, after which the model stops trusting
/// itself rather than reporting a guess.
#[test]
fn test_runtime_calls_are_stack_aligned() {
    let mut problems = Vec::new();

    for dir in ["src/runtime/sysv", "src/runtime/win64-native"] {
        for (path, text) in runtime_sources(dir) {
            let mut func = String::new();
            let mut offset: i64 = 8;
            let mut unknown = false;

            for (i, raw) in text.lines().enumerate() {
                let line = raw.split('#').next().unwrap_or("").trim();
                if line.is_empty() {
                    continue;
                }

                if let Some(sym) = line.strip_prefix(".globl ") {
                    func = sym.trim().to_string();
                    continue;
                }
                // A top-level label reopening the current function resets the
                // model; a local `.L` label is inside one and does not.
                if let Some(name) = line.strip_suffix(':') {
                    if name == func {
                        offset = 8;
                        unknown = false;
                    }
                    continue;
                }

                if line.starts_with("lea rsp,") {
                    unknown = true;
                } else if line.starts_with("push ") || line.starts_with("pop ") {
                    offset = (offset + 8) % 16;
                } else if let Some(n) = line.strip_prefix("sub rsp, ") {
                    offset =
                        (offset - n.trim().parse::<i64>().expect("literal reserve")).rem_euclid(16);
                } else if let Some(n) = line.strip_prefix("add rsp, ") {
                    offset = (offset + n.trim().parse::<i64>().expect("literal release")) % 16;
                } else if let Some(target) = line.strip_prefix("call ") {
                    if !unknown && offset != 0 {
                        problems.push(format!(
                            "{}:{}: in {}: call {} with rsp % 16 == {} (want 0)",
                            path,
                            i + 1,
                            func,
                            target.trim(),
                            offset
                        ));
                    }
                }
            }
        }
    }

    assert!(
        problems.is_empty(),
        "{} misaligned call site(s):\n{}",
        problems.len(),
        problems.join("\n")
    );
}
