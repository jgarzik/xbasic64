//! String functions: LEFT$, MID$, INSTR, concatenation, conversions
//!
//! Ported from string.s. A string is a (pointer, length) pair; slices of an
//! argument are returned as they are, and every fresh result comes from
//! `crate::alloc_str`.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::Str;

/// A fresh, owned copy of `s` (what `_rt_strdup` does), for helpers that
/// build a result in a scratch buffer and must not return the buffer itself.
pub fn strdup(s: &[u8]) -> Str {
    let _ = s;
    todo!("port _rt_strdup")
}
