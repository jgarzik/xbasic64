//! ABI abstraction layer for x86-64 calling conventions
//!
//! Provides platform-specific constants for System V AMD64 (Linux, macOS, BSD)
//! and Win64 (Windows) ABIs.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

/// Calling convention abstraction for x86-64
pub trait Abi {
    /// Integer/pointer argument registers (in order)
    const INT_ARG_REGS: &'static [&'static str];

    /// Symbol prefix for external symbols ("_" on macOS, "" elsewhere)
    const SYMBOL_PREFIX: &'static str;
}

/// System V AMD64 ABI (Linux, macOS, BSD)
pub struct SysV64;

impl Abi for SysV64 {
    const INT_ARG_REGS: &'static [&'static str] = &["rdi", "rsi", "rdx", "rcx", "r8", "r9"];

    #[cfg(target_os = "macos")]
    const SYMBOL_PREFIX: &'static str = "_";
    #[cfg(not(target_os = "macos"))]
    const SYMBOL_PREFIX: &'static str = "";
}

/// Windows x64 ABI
#[cfg(any(windows, test))]
pub struct Win64;

#[cfg(any(windows, test))]
impl Abi for Win64 {
    const INT_ARG_REGS: &'static [&'static str] = &["rcx", "rdx", "r8", "r9"];
    const SYMBOL_PREFIX: &'static str = "";
}

/// Type alias for the current platform's ABI
#[cfg(windows)]
pub type PlatformAbi = Win64;

#[cfg(not(windows))]
pub type PlatformAbi = SysV64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysv64_int_regs() {
        assert_eq!(SysV64::INT_ARG_REGS.len(), 6);
        assert_eq!(SysV64::INT_ARG_REGS[0], "rdi");
    }

    #[test]
    fn test_win64_int_regs() {
        assert_eq!(Win64::INT_ARG_REGS.len(), 4);
        assert_eq!(Win64::INT_ARG_REGS[0], "rcx");
    }
}
