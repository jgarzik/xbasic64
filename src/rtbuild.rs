//! The runtime library's sources, and how they become one assembly unit
//!
//! Shared by `build.rs`, which assembles the unit for the target into
//! `runtime.o` once, when the compiler is built, and by the compiler's own
//! tests, which check both trees. The compiler itself never sees this text: it
//! embeds the object (`runtime::OBJECT`) and links it beside each program's.
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
#[derive(Clone, Copy)]
pub struct Runtime {
    pub data_defs: &'static str,
    pub parts: [&'static str; 8],
    /// Appended after the last part. Object-format specific, so it is a
    /// property of the tree rather than something [`emit`] decides.
    pub trailer: &'static str,
    /// Directive putting one helper in a section of its own, with `{}` where
    /// its name goes. Empty leaves the whole runtime in one `.text`.
    ///
    /// A linker discards an unreferenced *section*, never an unreferenced
    /// label, so this is what lets a program drop the helpers it never calls.
    /// Object-format specific, and the flags are not the same spelling on
    /// COFF, which is why it lives here rather than in [`emit`].
    pub function_section: &'static str,
}

pub const SYSV: Runtime = Runtime {
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

pub const WIN64: Runtime = Runtime {
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
    // COFF's spelling of the same idea. The section name repeats -- COMDAT
    // sections are told apart by their symbol, not their name -- and the
    // fourth field names that symbol. `discard` is IMAGE_COMDAT_SELECT_ANY:
    // there is only ever one definition here, so the selection rule does not
    // matter, and the tolerant one is the better bet for something that
    // cannot be assembled on a Linux host. link.exe discards unreferenced
    // COMDATs under /OPT:REF, which main.rs passes explicitly.
    function_section: ".section .text,\"xr\",discard,{}\n",
};

/// The tree for a target, by Cargo's `CARGO_CFG_TARGET_OS` spelling. Chosen
/// by the target rather than by `cfg`, which in a build script describes
/// the machine building the compiler.
pub fn for_target(os: &str) -> &'static Runtime {
    if os == "windows" { &WIN64 } else { &SYSV }
}

/// Put each `_rt_*` helper in `part` into a section of its own.
///
/// Done here rather than by hand in the sources so that a helper added later
/// is covered without anyone remembering to, and so the two trees cannot
/// drift apart on it.
///
/// Only exported entry points are split, for COFF's sake: a COMDAT section is
/// identified by a symbol, and that symbol has to be external. The three
/// helpers that are not `.globl` -- `_rt_case_convert` and, on Win64,
/// `_rt_init_input` -- therefore ride in the section of whichever helper
/// precedes them, which costs nothing: a jump into one keeps that section,
/// and `_rt_ucase` and `_rt_lcase` both jump to `_rt_case_convert`.
///
/// This is only sound because no helper falls through into the next one:
/// sections are not laid out in source order, so a fall-through would land
/// somewhere else entirely. `_rt_lcase` used to, and now jumps explicitly.
fn split_helpers(part: &str, template: &str) -> String {
    let exported: Vec<&str> = part
        .lines()
        .filter_map(|l| l.strip_prefix(".globl "))
        .map(str::trim)
        .collect();

    // Emitted ahead of the `.globl`, not between it and the label, because
    // that is the order LLVM itself writes a COFF COMDAT in and there is no
    // way to check the alternative from here. Every `.globl _rt_x` in both
    // trees is immediately followed by `_rt_x:`, which a test enforces.
    let mut output = String::with_capacity(part.len() + part.len() / 8);
    for line in part.lines() {
        if let Some(name) = line.strip_prefix(".globl ").map(str::trim)
            && exported.contains(&name)
            && name.starts_with("_rt_")
        {
            output.push_str(&template.replace("{}", name));
        }
        output.push_str(line);
        output.push('\n');
    }
    output
}

/// Concatenate one runtime tree into a single assembly unit.
pub fn emit(rt: &Runtime) -> String {
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
