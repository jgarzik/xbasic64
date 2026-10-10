//! The runtime library, as an object file built with the compiler
//!
//! `build.rs` assembles the target's runtime tree (see `rtbuild.rs`) once;
//! the compiler writes this object out beside each program's own and links
//! the two. Helpers a program never calls are dropped at link time: each is
//! in a section of its own.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

/// The assembled runtime for the target this compiler was built for.
pub const OBJECT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/runtime.o"));

#[cfg(test)]
use crate::rtbuild::{Runtime, SYSV, WIN64, emit, for_target};

/// The helper name on a line that is exactly a `_rt_*` label definition.
///
/// Used to pair a `.globl` with the label that must follow it, which is an
/// invariant `split_helpers` depends on and only a test checks.
#[cfg(test)]
fn helper_label(line: &str) -> Option<&str> {
    let name = line.strip_prefix("_rt_")?.strip_suffix(':')?;
    // A label and nothing else: `_rt_foo:` yes, `_rt_foo: .quad 0` no.
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }
    Some(&line[..line.len() - 1])
}

/// Both runtime trees, each as one unsplit assembly text, by tree name.
#[cfg(test)]
pub(crate) fn tree_sources() -> [(&'static str, String); 2] {
    let whole = |rt: &Runtime| {
        emit(&Runtime {
            function_section: "",
            ..*rt
        })
    };
    [
        ("sysv", whole(for_target("linux"))),
        ("win64-native", whole(for_target("windows"))),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Every exported helper's `.globl` is immediately followed by its label.
    ///
    /// `split_helpers` puts the section directive ahead of the `.globl`, so a
    /// `.globl` that drifted away from its label would open a section around
    /// the wrong code -- and on ELF, where the flags are accepted, it would do
    /// so silently.
    #[test]
    fn test_globl_precedes_its_label() {
        for rt in [&SYSV, &WIN64] {
            for part in rt.parts {
                let lines: Vec<&str> = part.lines().collect();
                for (i, line) in lines.iter().enumerate() {
                    let Some(name) = line.strip_prefix(".globl ").map(str::trim) else {
                        continue;
                    };
                    if !name.starts_with("_rt_") {
                        continue;
                    }
                    let next = lines.get(i + 1).copied().unwrap_or("");
                    assert_eq!(
                        helper_label(next),
                        Some(name),
                        "`.globl {}` must be followed by its label, not {:?}",
                        name,
                        next
                    );
                }
            }
        }
    }

    /// Each runtime helper is emitted into a section of its own.
    ///
    /// A linker discards an unreferenced *section*, never an unreferenced
    /// label, so this is what lets the linker drop the helpers a program does
    /// not call. Without it all 69 went into every binary.
    ///
    /// Counted against the exported helpers, not against every label: a COMDAT
    /// section is identified by a symbol and that symbol has to be external,
    /// so the handful of helpers that are not `.globl` deliberately ride in
    /// the section of whichever helper precedes them.
    ///
    /// The two object formats spell the directive differently -- ELF names the
    /// section after the helper, COFF repeats `.text` and names the helper as
    /// the COMDAT symbol -- so the match is on what they have in common.
    #[test]
    fn test_runtime_helpers_get_their_own_sections() {
        for rt in [&SYSV, &WIN64] {
            let asm = emit(rt);
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
    }

    /// Both runtimes must assemble, including the one this host cannot run.
    ///
    /// Only one tree is ever reachable from a compiled program, so a mistake in
    /// the other -- a duplicated `.L` label, a typo'd branch target, an operand
    /// form the assembler rejects -- stays invisible until the other platform's
    /// CI runs. That is exactly how a block of code duplicated into a second
    /// function, defining its labels twice, reached Windows and failed every
    /// single compile there.
    ///
    /// The files of a tree are concatenated into one assembly unit, so this
    /// assembles the whole tree rather than each file alone: `.L` labels are
    /// not file-local the way they look.
    ///
    /// This is a Unix-host check, which is where it is worth having: GNU `as`
    /// assembles every variant, so the Linux job catches a Win64 mistake before
    /// Windows CI ever sees it. The reverse is not needed -- a System V
    /// mistake fails the Linux job outright -- and Windows CI builds its own
    /// tree in build.rs.
    ///
    /// What it does *not* cover, since GNU `as` targets ELF: the Win64 tree's
    /// per-function section directives, whose flags and COMDAT selector are
    /// COFF spellings that this assembler rejects outright. They are stripped
    /// here so the hand-written code underneath still gets checked, which is
    /// what this test was ever for. Only Windows CI can say whether the
    /// directive itself is right.
    #[test]
    #[cfg_attr(windows, ignore = "GNU as assembles every tree; Windows uses clang")]
    fn test_both_runtimes_assemble() {
        // Every tree the compiler can emit, with the section directives taken
        // off: this host's assembler has no opinion worth hearing about them.
        let variants = [
            (
                "sysv",
                Runtime {
                    function_section: "",
                    ..SYSV
                },
            ),
            (
                "win64-native",
                Runtime {
                    function_section: "",
                    ..WIN64
                },
            ),
        ];

        for (name, rt) in &variants {
            let dir = std::env::temp_dir().join(format!("xbasic64-asm-{name}"));
            std::fs::create_dir_all(&dir).expect("writable temp directory");
            let asm = dir.join("runtime.s");
            let obj = dir.join("runtime.o");

            std::fs::write(&asm, emit(rt)).expect("writable assembly file");

            let output = Command::new("as")
                .arg("-o")
                .arg(&obj)
                .arg(&asm)
                .output()
                .expect("GNU as, which the compiler itself requires");

            assert!(
                output.status.success(),
                "the {} runtime does not assemble:\n{}",
                name,
                String::from_utf8_lossy(&output.stderr)
            );

            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
