//! ABI abstraction layer for x86-64 calling conventions
//!
//! Provides platform-specific constants for System V AMD64 (Linux)
//! and Win64 (Windows) ABIs.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

/// Calling convention abstraction for x86-64
pub trait Abi {
    /// Integer/pointer argument registers (in order)
    const INT_ARG_REGS: &'static [&'static str];

    /// Bytes the caller reserves for the callee to spill its register
    /// arguments into. Win64 requires this "home space" before every call;
    /// System V requires none.
    const SHADOW_SPACE: i32;

    /// Register for a floating-point argument: the `nth_float`-th float
    /// argument, at position `pos` in the whole argument list.
    ///
    /// System V numbers float registers separately from integer ones, so only
    /// `nth_float` matters. Win64 gives each position one slot, either `rcx`
    /// or `xmm0` for the first argument, `rdx` or `xmm1` for the second, and
    /// so on, so only `pos` matters.
    fn float_arg_reg(pos: usize, nth_float: usize) -> &'static str;

    /// General registers a function must preserve for its caller.
    const CALLEE_SAVED: &'static [&'static str];
}

const XMM: [&str; 8] = [
    "xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5", "xmm6", "xmm7",
];

/// System V AMD64 ABI (Linux)
///
/// Compiled in on a Windows host only under test, mirroring [`Win64`]: each is
/// dead code in a release build for the other platform, and the unit tests
/// below check both.
#[cfg(any(not(windows), test))]
pub struct SysV64;

#[cfg(any(not(windows), test))]
impl Abi for SysV64 {
    const INT_ARG_REGS: &'static [&'static str] = &["rdi", "rsi", "rdx", "rcx", "r8", "r9"];

    const SHADOW_SPACE: i32 = 0;

    const CALLEE_SAVED: &'static [&'static str] = &["rbx", "r12", "r13", "r14", "r15"];

    fn float_arg_reg(_pos: usize, nth_float: usize) -> &'static str {
        XMM[nth_float]
    }
}

/// Windows x64 ABI
#[cfg(any(windows, test))]
pub struct Win64;

#[cfg(any(windows, test))]
impl Abi for Win64 {
    const INT_ARG_REGS: &'static [&'static str] = &["rcx", "rdx", "r8", "r9"];
    const SHADOW_SPACE: i32 = 32;

    // xmm6-xmm15 are callee-saved here too. Nothing in a trapping program
    // keeps a value in one: FOR-loop promotion, codegen's only user of them,
    // is off when a program traps.
    const CALLEE_SAVED: &'static [&'static str] =
        &["rbx", "rsi", "rdi", "r12", "r13", "r14", "r15"];

    fn float_arg_reg(pos: usize, _nth_float: usize) -> &'static str {
        XMM[pos]
    }
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

    /// A double after one integer argument: System V counts it as the first
    /// float, Win64 as the second position.
    #[test]
    fn test_float_arg_reg_after_an_integer() {
        assert_eq!(SysV64::float_arg_reg(1, 0), "xmm0");
        assert_eq!(Win64::float_arg_reg(1, 0), "xmm1");
        assert_eq!(SysV64::float_arg_reg(3, 0), "xmm0");
        assert_eq!(Win64::float_arg_reg(3, 0), "xmm3");
    }

    #[test]
    fn test_win64_int_regs() {
        assert_eq!(Win64::INT_ARG_REGS.len(), 4);
        assert_eq!(Win64::INT_ARG_REGS[0], "rcx");
    }
}
