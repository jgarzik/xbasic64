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

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

const DATA_DEFS: &str = include_str!("runtime/data_defs.s");
const PRINT_FUNCS: &str = include_str!("runtime/print.s");
const INPUT_FUNCS: &str = include_str!("runtime/input.s");
const STRING_FUNCS: &str = include_str!("runtime/string.s");
const MATH_FUNCS: &str = include_str!("runtime/math.s");
const DATA_FUNCS: &str = include_str!("runtime/data.s");
const FILE_FUNCS: &str = include_str!("runtime/file.s");

pub fn generate_runtime() -> String {
    // On macOS, C library functions need underscore prefix
    #[cfg(target_os = "macos")]
    let libc_prefix = "_";
    #[cfg(not(target_os = "macos"))]
    let libc_prefix = "";

    // Assemble all runtime components
    let mut output = String::new();

    output.push_str("# BASIC Runtime Library\n");
    output.push_str("# Uses libc for cross-platform compatibility\n");
    output.push_str(".intel_syntax noprefix\n\n");

    // Data section
    output.push_str(DATA_DEFS);
    output.push_str("\n.text\n\n");

    // Functions - replace {libc} with appropriate prefix
    output.push_str(&PRINT_FUNCS.replace("{libc}", libc_prefix));
    output.push('\n');
    output.push_str(&INPUT_FUNCS.replace("{libc}", libc_prefix));
    output.push('\n');
    output.push_str(&STRING_FUNCS.replace("{libc}", libc_prefix));
    output.push('\n');
    output.push_str(&MATH_FUNCS.replace("{libc}", libc_prefix));
    output.push('\n');
    output.push_str(&DATA_FUNCS.replace("{libc}", libc_prefix));
    output.push('\n');
    output.push_str(&FILE_FUNCS.replace("{libc}", libc_prefix));
    output.push('\n');

    output
}
