//! Runtime support - generates assembly for runtime functions
//!
//! Uses libc functions for cross-platform compatibility.
//!
//! Runtime is split into separate assembly files for maintainability:
//! - data_defs.s: Data section definitions (format strings, buffers)
//! - print.s: Print functions
//! - input.s: Input functions
//! - string.s: String manipulation functions
//! - math.s: Math and utility functions
//! - data.s: DATA/READ support functions
//! - file.s: File I/O functions (OPEN, CLOSE, PRINT#, INPUT#)
//! - error.s: Runtime error reporting
//! - using.s: PRINT USING field output
//!
//! Platform-specific runtimes:
//! - sysv/: System V AMD64 ABI (Linux)
//! - win64-native/: Windows x64 ABI

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

/// One runtime tree's sources, in the order they are concatenated.
struct Runtime {
    data_defs: &'static str,
    parts: [&'static str; 8],
    /// Appended after the last part. Object-format specific, so it is a
    /// property of the tree rather than something [`emit`] decides.
    trailer: &'static str,
    /// Directive putting one helper in a section of its own, with `{}` where
    /// its name goes. Empty leaves the whole runtime in one `.text`.
    ///
    /// A linker discards an unreferenced *section*, never an unreferenced
    /// label, so this is what lets a program drop the helpers it never calls.
    /// Object-format specific, and the flags are not the same spelling on
    /// COFF, which is why it lives here rather than in [`emit`].
    function_section: &'static str,
}

// Under test both trees are compiled in, so the one this host does not use is
// still assembled; a release build carries only the tree it can emit.
#[cfg(any(test, not(windows)))]
const SYSV: Runtime = Runtime {
    data_defs: include_str!("runtime/sysv/data_defs.s"),
    parts: [
        include_str!("runtime/sysv/print.s"),
        include_str!("runtime/sysv/input.s"),
        include_str!("runtime/sysv/string.s"),
        include_str!("runtime/sysv/math.s"),
        include_str!("runtime/sysv/data.s"),
        include_str!("runtime/sysv/file.s"),
        include_str!("runtime/sysv/error.s"),
        include_str!("runtime/sysv/using.s"),
    ],
    // Without this note GNU ld cannot tell whether the object needs an
    // executable stack, assumes it does, and warns on every single compile --
    // and the program it links really does get a writable, executable stack.
    // This runtime never runs code from the stack, so say so.
    trailer: ".section .note.GNU-stack,\"\",@progbits\n",
    function_section: ".section .text.{},\"ax\",@progbits\n",
};

#[cfg(any(test, windows))]
const WIN64: Runtime = Runtime {
    data_defs: include_str!("runtime/win64-native/data_defs.s"),
    parts: [
        include_str!("runtime/win64-native/print.s"),
        include_str!("runtime/win64-native/input.s"),
        include_str!("runtime/win64-native/string.s"),
        include_str!("runtime/win64-native/math.s"),
        include_str!("runtime/win64-native/data.s"),
        include_str!("runtime/win64-native/file.s"),
        include_str!("runtime/win64-native/error.s"),
        include_str!("runtime/win64-native/using.s"),
    ],
    // COFF has no equivalent note, and clang rejects the ELF spelling.
    trailer: "",
    // Left alone deliberately: the ELF flag spelling is not COFF's, and there
    // is no COFF assembler on the machine this was developed on -- not clang,
    // not llvm-mc, not mingw -- so the directive could not be checked even for
    // syntax. Windows keeps the whole runtime; correctness is not affected.
    function_section: "",
};

/// Put each `_rt_*` helper in `part` into a section of its own.
///
/// Done here rather than by hand in the sources so that a helper added later
/// is covered without anyone remembering to, and so the two trees cannot
/// drift apart on it.
///
/// Only the exported entry points are split. A helper's internal `.L` labels
/// stay with it, and a private label that is *not* an entry point -- like
/// `_rt_case_convert`, which `_rt_ucase` and `_rt_lcase` both jump to -- gets
/// its own section too and is kept alive by those references.
///
/// This is only sound because no helper falls through into the next one:
/// sections are not laid out in source order, so a fall-through would land
/// somewhere else entirely. `_rt_lcase` used to, and now jumps explicitly.
fn split_helpers(part: &str, template: &str) -> String {
    let mut output = String::with_capacity(part.len() + part.len() / 8);
    for line in part.lines() {
        if let Some(name) = helper_label(line) {
            output.push_str(&template.replace("{}", name));
        }
        output.push_str(line);
        output.push('\n');
    }
    output
}

/// The helper name on a line that is exactly a `_rt_*` label definition.
fn helper_label(line: &str) -> Option<&str> {
    let name = line.strip_prefix("_rt_")?.strip_suffix(':')?;
    // A label and nothing else: `_rt_foo:` yes, `_rt_foo: .quad 0` no.
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }
    Some(&line[..line.len() - 1])
}

/// Concatenate one runtime tree into a single assembly unit.
fn emit(rt: &Runtime) -> String {
    let mut output = String::new();

    output.push_str("# BASIC Runtime Library\n");
    output.push_str("# Uses libc for cross-platform compatibility\n");
    output.push_str(".intel_syntax noprefix\n\n");

    output.push_str(rt.data_defs);
    output.push_str("\n.text\n\n");

    for part in rt.parts {
        if rt.function_section.is_empty() {
            output.push_str(part);
        } else {
            output.push_str(&split_helpers(part, rt.function_section));
        }
        output.push('\n');
    }

    output.push_str(rt.trailer);

    output
}

pub fn generate_runtime() -> String {
    #[cfg(windows)]
    let rt = &WIN64;
    #[cfg(not(windows))]
    let rt = &SYSV;

    emit(rt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

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
    /// mistake fails the Linux job outright -- and Windows builds its own tree
    /// on every compile a test performs.
    #[test]
    #[cfg_attr(windows, ignore = "GNU as assembles every tree; Windows uses clang")]
    fn test_both_runtimes_assemble() {
        // Every tree the compiler can emit.
        let variants = [("sysv", &SYSV), ("win64-native", &WIN64)];

        for (name, rt) in variants {
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
