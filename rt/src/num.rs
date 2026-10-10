//! Numbers as text: GW-BASIC's number formatting, and its number reader
//!
//! Ported from `_rt_fmt_double` / `_rt_fmt_basic` (print.s) and
//! `_rt_scan_number` (string.s). Shared by PRINT, STR$, WRITE, PRINT USING,
//! VAL, INPUT, INPUT # and READ, so they all agree.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

/// What `scan_number` read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scanned {
    /// Bytes consumed, up to the last that belongs to the number; 0 when
    /// the text does not begin with a number at all.
    pub consumed: usize,
    /// The value; 0 when there is no number.
    pub value: f64,
    /// The value is too large for a DOUBLE.
    pub overflow: bool,
}

/// Read a number the way GW-BASIC's VAL and INPUT do (`_rt_scan_number`).
pub fn scan_number(text: &[u8]) -> Scanned {
    let _ = text;
    todo!("port _rt_scan_number")
}

/// A formatted number: at most `NumText::CAP` bytes, no terminator.
#[derive(Clone, Copy)]
pub struct NumText {
    buf: [u8; NumText::CAP],
    len: usize,
}

impl NumText {
    pub const CAP: usize = 48;

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

/// A number's digits as GW-BASIC writes them, without the leading blank
/// (`_rt_fmt_double`): 7 significant digits for a SINGLE, 16 for a DOUBLE.
pub fn fmt_double(x: f64, single: bool) -> NumText {
    let _ = (x, single);
    todo!("port _rt_fmt_double")
}

/// `fmt_double` with the blank that stands where a minus sign would go, as
/// PRINT and STR$ write a number (`_rt_fmt_basic`).
pub fn fmt_basic(x: f64, single: bool) -> NumText {
    let _ = (x, single);
    todo!("port _rt_fmt_basic")
}
