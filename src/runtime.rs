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
//!
//! Platform-specific runtimes:
//! - sysv/: System V AMD64 ABI (Linux, macOS, BSD)
//! - win64/: Windows x64 ABI

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

// System V ABI runtime (Linux, macOS, BSD)
#[cfg(not(windows))]
mod runtime_files {
    pub const DATA_DEFS: &str = include_str!("runtime/sysv/data_defs.s");
    pub const PRINT_FUNCS: &str = include_str!("runtime/sysv/print.s");
    pub const INPUT_FUNCS: &str = include_str!("runtime/sysv/input.s");
    pub const STRING_FUNCS: &str = include_str!("runtime/sysv/string.s");
    pub const MATH_FUNCS: &str = include_str!("runtime/sysv/math.s");
    pub const DATA_FUNCS: &str = include_str!("runtime/sysv/data.s");
    pub const FILE_FUNCS: &str = include_str!("runtime/sysv/file.s");
}

// Windows x64 Native runtime (pure Win32 API, no MinGW)
#[cfg(windows)]
mod runtime_files {
    pub const DATA_DEFS: &str = include_str!("runtime/win64-native/data_defs.s");
    pub const PRINT_FUNCS: &str = include_str!("runtime/win64-native/print.s");
    pub const INPUT_FUNCS: &str = include_str!("runtime/win64-native/input.s");
    pub const STRING_FUNCS: &str = include_str!("runtime/win64-native/string.s");
    pub const MATH_FUNCS: &str = include_str!("runtime/win64-native/math.s");
    pub const DATA_FUNCS: &str = include_str!("runtime/win64-native/data.s");
    pub const FILE_FUNCS: &str = include_str!("runtime/win64-native/file.s");
}

use runtime_files::*;

pub fn generate_runtime() -> String {
    // On macOS, C library functions need underscore prefix
    // On Linux and Windows, no prefix
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
