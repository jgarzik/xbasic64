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

/// A message length must not be an assembler constant.
///
/// `.equ len, . - label` reads as the safe way to avoid hand-counting, and it
/// is -- under GNU as, which folds it to an immediate. This tree is assembled
/// by GNU as on Linux and by clang on Windows, and where an assembler cannot
/// prove the symbol absolute it assembles `mov reg, len` as a *memory load
/// from that address* instead. `error.s` says so at the top: it hit this with
/// forward references and now measures its messages at run time.
///
/// `_cls_seq_len` was the same shape, so every `CLS` on Windows loaded from
/// address 7 and died with an access violation, while Linux ran it as the
/// immediate the source appears to say. Nothing else was wrong with the
/// helper, which is why it took a table of exit codes to find.
///
/// Bracket the data with labels and subtract at run time instead. Two
/// instructions, and the same answer whichever assembler ran.
#[test]
fn test_message_lengths_are_not_assembler_constants() {
    let mut problems = Vec::new();

    for dir in ["src/runtime/sysv", "src/runtime/win64-native"] {
        for (path, text) in runtime_sources(dir) {
            for (i, line) in text.lines().enumerate() {
                let trimmed = line.split('#').next().unwrap_or("").trim();
                // Both spellings of an assembler constant: `.equ N, v` and
                // `N = v`. Only checking `.equ` let one through before.
                let (name, value) = match trimmed.strip_prefix(".equ ") {
                    Some(rest) => match rest.split_once(',') {
                        Some((n, v)) => (n.trim(), v.trim()),
                        None => continue,
                    },
                    None => match trimmed.split_once('=') {
                        Some((lhs, rhs)) if !lhs.trim().contains(char::is_whitespace) => {
                            (lhs.trim(), rhs.trim())
                        }
                        _ => continue,
                    },
                };
                // A plain number is fine: it is absolute to any assembler.
                // Anything naming a label is not.
                let symbolic = value
                    .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
                    .any(|t| {
                        !t.is_empty() && !t.chars().all(|c| c.is_ascii_hexdigit() || c == 'x')
                    });
                if symbolic || value.contains(". -") {
                    problems.push(format!(
                        "{path}:{}: `{trimmed}` measures {name} with the assembler. Bracket the \
                         data with a `_end` label and subtract at run time: an assembler that \
                         cannot prove this absolute turns `mov reg, {name}` into a memory load.",
                        i + 1
                    ));
                }
            }
        }
    }

    assert!(
        problems.is_empty(),
        "{} assembler-computed length(s):\n{}",
        problems.len(),
        problems.join("\n")
    );
}

/// An `_end` label must sit directly after the data it bounds.
///
/// It is the run-time replacement for `. - label`, and it inherits the same
/// hazard: anything inserted between the data and the label is measured as
/// part of the message. Four format strings once were, and `CLS` wrote 75
/// bytes where it meant to write 7.
#[test]
fn test_end_labels_bound_their_own_data() {
    for dir in ["src/runtime/sysv", "src/runtime/win64-native"] {
        for (path, text) in runtime_sources(dir) {
            let lines: Vec<&str> = text
                .lines()
                .map(|l| l.split('#').next().unwrap_or("").trim())
                .collect();
            for (i, line) in lines.iter().enumerate() {
                let Some(label) = line.strip_suffix(':') else {
                    continue;
                };
                let Some(measured) = label.strip_suffix("_end") else {
                    continue;
                };
                // Only labels that bound something. `.Lfile_past_end` is a
                // branch target, and `_rt_end` is a helper whose name happens
                // to split this way -- neither has a `_x:` data line to sit
                // after.
                let bounds_data = lines
                    .iter()
                    .any(|l| l.starts_with(&format!("{measured}: .")));
                if !bounds_data {
                    continue;
                }
                let previous = lines[..i].iter().rev().find(|l| !l.is_empty());
                assert!(
                    previous.is_some_and(|p| p.starts_with(&format!("{measured}:"))),
                    "{path}:{}: {label} must sit directly after {measured}'s data, but {:?} \
                     intervenes -- everything between them is measured as part of the message",
                    i + 1,
                    previous.copied().unwrap_or("")
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

/// A helper must preserve the registers its own ABI calls callee-saved.
///
/// The lists differ, and that is the whole hazard: `rdi` and `rsi` are
/// callee-saved on Win64 and scratch on System V, so a Win64 helper that uses
/// one as a temporary is wrong in a way no Linux run can show. `_rt_print_using_str`
/// did exactly that, and only survived because the code that calls it happens
/// not to keep anything in `rdi`.
///
/// Deliberately crude: any `push` of a register anywhere in the helper counts
/// as saving it, and only writes through a named destination are seen. That is
/// enough for hand-written assembly of this shape, and a false negative is
/// better here than a test nobody trusts.
#[test]
fn test_helpers_preserve_callee_saved_registers() {
    // `rbp` is excluded: every helper frames with it and restores it via
    // `leave`, which this scan would have to model separately to no purpose.
    const SYSV_SAVED: &[&str] = &["rbx", "r12", "r13", "r14", "r15"];
    const WIN64_SAVED: &[&str] = &[
        "rbx", "rdi", "rsi", "r12", "r13", "r14", "r15", "xmm6", "xmm7", "xmm8", "xmm9", "xmm10",
        "xmm11", "xmm12", "xmm13", "xmm14", "xmm15",
    ];

    // `_rt_random_prepare` returns four values in callee-saved registers and
    // says so: it is reached only from GET and PUT, which save them for it.
    // A helper listed here has a private convention its own comment states.
    const PRIVATE_CONVENTION: &[&str] = &["_rt_random_prepare"];

    let mut problems = Vec::new();

    for (dir, saved) in [
        ("src/runtime/sysv", SYSV_SAVED),
        ("src/runtime/win64-native", WIN64_SAVED),
    ] {
        for (path, text) in runtime_sources(dir) {
            let mut helper = String::new();
            let mut pushed: BTreeSet<String> = BTreeSet::new();
            let mut used: Vec<(usize, String)> = Vec::new();

            let mut flush = |helper: &str, pushed: &BTreeSet<String>, used: &[(usize, String)]| {
                if PRIVATE_CONVENTION.contains(&helper) {
                    return;
                }
                for (line, reg) in used {
                    if !pushed.contains(reg) {
                        problems.push(format!(
                            "{path}:{line}: {helper} writes {reg}, which is callee-saved here, \
                             without pushing it"
                        ));
                    }
                }
            };

            for (i, raw) in text.lines().enumerate() {
                let line = raw.split('#').next().unwrap_or("").trim();
                if let Some(name) = line.strip_prefix(".globl ") {
                    flush(&helper, &pushed, &used);
                    helper = name.trim().to_string();
                    pushed.clear();
                    used.clear();
                    continue;
                }
                if let Some(reg) = line.strip_prefix("push ") {
                    pushed.insert(reg.trim().to_string());
                    continue;
                }
                // `<op> <dest>, ...` -- the destination is what gets written.
                let Some((_, rest)) = line.split_once(' ') else {
                    continue;
                };
                let dest = rest.split(',').next().unwrap_or("").trim();
                // 32-bit writes clear the upper half, so they count too.
                let full = match dest {
                    "edi" => "rdi",
                    "esi" => "rsi",
                    "ebx" => "rbx",
                    other => other,
                };
                if saved.contains(&full) {
                    used.push((i + 1, full.to_string()));
                }
            }
            flush(&helper, &pushed, &used);
        }
    }

    assert!(
        problems.is_empty(),
        "{} clobbered callee-saved register(s):\n{}",
        problems.len(),
        problems.join("\n")
    );
}
