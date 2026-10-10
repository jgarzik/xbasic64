//! String functions: LEFT$, MID$, INSTR, concatenation, conversions
//!
//! Ported from string.s. A string is a (pointer, length) pair, not
//! NUL-terminated, which lets LEFT$, RIGHT$, MID$, LTRIM$ and RTRIM$ return
//! a view into their argument without copying: BASIC strings are never
//! mutated in place except by MID$ assignment, which writes only bytes the
//! target owns. Every other result is fresh, from `crate::alloc_str`, and
//! the caller owns and frees it.
//!
//! A conversion that formats into a scratch buffer (STR$, CHR$, HEX$, OCT$)
//! returns a copy of it, never the buffer: nothing copies a result until it
//! is assigned, so STR$(A) + STR$(B) would otherwise see the first half
//! overwritten by the second.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::error;
use crate::num::fmt_basic;
use crate::{Str, alloc_str, sys};
use core::ffi::c_longlong;

/// A string argument as the bytes it names.
///
/// # Safety
/// `ptr` must point at `len` readable bytes that outlive the slice, unless
/// `len` is 0.
pub(crate) unsafe fn arg<'a>(ptr: *const u8, len: i64) -> &'a [u8] {
    unsafe {
        Str {
            ptr,
            len: len as usize,
        }
        .bytes()
    }
}

/// A fresh string of `len` bytes, each `fill`, for the caller to fill in
/// further: `None` when there is no memory, which has then been raised. One
/// byte more is allocated and set to NUL, for safety, as the asm did.
fn fresh(len: usize, fill: u8) -> Option<&'static mut [u8]> {
    let p = alloc_str(len.wrapping_add(1));
    if p.is_null() {
        return None;
    }
    unsafe {
        p.write_bytes(fill, len);
        *p.add(len) = 0;
        Some(core::slice::from_raw_parts_mut(p, len))
    }
}

/// The string a `fresh` buffer holds; the empty string if there was none.
fn owned(s: Option<&'static mut [u8]>) -> Str {
    s.map_or(Str::EMPTY, |s| Str {
        ptr: s.as_ptr(),
        len: s.len(),
    })
}

/// `len` bytes from `ptr + from`: a view of an argument, as a result. Even
/// an empty one points into the argument, as the asm's did.
fn view(ptr: *const u8, from: usize, len: usize) -> Str {
    Str {
        ptr: ptr.wrapping_add(from),
        len,
    }
}

/// A fresh, owned copy of `s` (what `_rt_strdup` does), for helpers that
/// build a result in a scratch buffer and must not return the buffer itself.
pub fn strdup(s: &[u8]) -> Str {
    let mut d = fresh(s.len(), 0);
    if let Some(d) = d.as_deref_mut() {
        d.copy_from_slice(s);
    }
    owned(d)
}

/// `s` with ASCII letters mapped by `f`, as a fresh copy: the source may be
/// a literal shared with other uses, so it is never changed in place.
fn map_case(s: &[u8], f: fn(&u8) -> u8) -> Str {
    let mut d = fresh(s.len(), 0);
    if let Some(d) = d.as_deref_mut() {
        for (d, c) in d.iter_mut().zip(s) {
            *d = f(c);
        }
    }
    owned(d)
}

/// HEX$ / OCT$: `n` through the C library's `fmt`.
fn radix(n: i64, fmt: &[u8]) -> Str {
    let mut buf = [0u8; 32];
    let len = unsafe {
        sys::snprintf(
            buf.as_mut_ptr().cast(),
            buf.len(),
            fmt.as_ptr().cast(),
            n as c_longlong,
        )
    };
    strdup(buf.get(..len as usize).unwrap_or(&[]))
}

/// A count clamped to a length, compared unsigned as the asm did: a
/// negative count is the whole length.
fn clamp(n: i64, len: usize) -> usize {
    (n as u64).min(len as u64) as usize
}

export! {
    /// STR$: exactly the text PRINT writes for a DOUBLE, sign blank
    /// included, so that VAL(STR$(X)) is X to the digits PRINT shows.
    fn _rt_str(x: f64) -> Str {
        strdup(fmt_basic(x, false).as_bytes())
    }

    /// STR$ of a SINGLE, at a SINGLE's 7 digits as PRINT writes it:
    /// rendered as a DOUBLE, 3.14159! would be "3.141590118408203".
    fn _rt_str_single(x: f64) -> Str {
        strdup(fmt_basic(x, true).as_bytes())
    }

    /// CHR$: the one-character string whose code is `code`'s low byte.
    fn _rt_chr(code: i64) -> Str {
        strdup(&[code as u8])
    }

    /// LEFT$: the first `n` characters, or all of them; a view.
    fn _rt_left(ptr: *const u8, len: i64, n: i64) -> Str {
        view(ptr, 0, clamp(n, len as usize))
    }

    /// RIGHT$: the last `n` characters, or all of them; a view.
    fn _rt_right(ptr: *const u8, len: i64, n: i64) -> Str {
        let n = clamp(n, len as usize);
        view(ptr, len as usize - n, n)
    }

    /// MID$: `count` characters from the 1-based `start`, or the rest when
    /// `count` is negative or runs past the end; a view. A start past the
    /// end, or below 1, is the empty string.
    fn _rt_mid(ptr: *const u8, len: i64, start: i64, count: i64) -> Str {
        let from = start.wrapping_sub(1) as u64;
        if from >= len as u64 {
            return view(ptr, 0, 0);
        }
        let rest = len as usize - from as usize;
        let n = if count < 0 { rest } else { clamp(count, rest) };
        view(ptr, from as usize, n)
    }

    /// INSTR: the 1-based position of `needle` in `hay` at or after
    /// `start`, 0 if it is not there. An empty needle is found at `start`
    /// itself, unless that is past the end. A start below 1 is Illegal
    /// function call, and then "not found" when a handler traps it.
    fn _rt_instr(hay: *const u8, hay_len: i64, needle: *const u8, needle_len: i64, start: i64) -> i64 {
        if start < 1 {
            error::raise(&error::_err_domain[..], 0);
            return 0;
        }
        let (hay, needle) = unsafe { (arg(hay, hay_len), arg(needle, needle_len)) };
        let from = (start - 1) as usize;
        if from >= hay.len() {
            return 0;
        }
        if needle.is_empty() {
            return start;
        }
        hay[from..]
            .windows(needle.len())
            .position(|w| w == needle)
            .map_or(0, |i| (from + i + 1) as i64)
    }

    /// `+` on strings: a fresh string holding both.
    fn _rt_strcat(a: *const u8, a_len: i64, b: *const u8, b_len: i64) -> Str {
        let (a, b) = unsafe { (arg(a, a_len), arg(b, b_len)) };
        let mut d = fresh(a.len() + b.len(), 0);
        if let Some(d) = d.as_deref_mut() {
            let p = d.as_mut_ptr();
            unsafe {
                p.copy_from_nonoverlapping(a.as_ptr(), a.len());
                p.add(a.len()).copy_from_nonoverlapping(b.as_ptr(), b.len());
            }
        }
        owned(d)
    }

    /// Compare two strings by unsigned byte value, then by length when one
    /// is a prefix of the other, so "ab" < "abc". Negative, zero or
    /// positive as `a` is below, equal to or above `b`: the difference of
    /// the first differing bytes, else -1, 0 or 1. An empty string's pointer
    /// is never read, so an unassigned (NULL, 0) string compares correctly.
    fn _rt_strcmp(a: *const u8, a_len: i64, b: *const u8, b_len: i64) -> i32 {
        let (a, b) = unsafe { (arg(a, a_len), arg(b, b_len)) };
        match a.iter().zip(b).find(|(x, y)| x != y) {
            Some((&x, &y)) => i32::from(x) - i32::from(y),
            None => a.len().cmp(&b.len()) as i32,
        }
    }

    /// SPACE$: `n` blanks; none for a negative `n`.
    fn _rt_space(n: i64) -> Str {
        owned(fresh(n.max(0) as usize, b' '))
    }

    /// Fit a string to a declared width, for STRING * n: padded with blanks
    /// on the right, or cut. Fresh rather than a view, because the result is
    /// stored and the source may be a temporary.
    fn _rt_fixed(ptr: *const u8, len: i64, width: i64) -> Str {
        let s = unsafe { arg(ptr, len) };
        let width = width.max(0) as usize;
        let mut d = fresh(width, b' ');
        if let Some(d) = d.as_deref_mut() {
            let n = clamp(width as i64, s.len());
            d[..n].copy_from_slice(&s[..n]);
        }
        owned(d)
    }

    /// STRING$(n, c): `n` copies of the character `code`; none for a
    /// negative `n`. Codegen checks the code, and that a string argument is
    /// not empty, and passes its first character's code.
    fn _rt_string_n(n: i64, code: i64) -> Str {
        owned(fresh(n.max(0) as usize, code as u8))
    }

    /// LTRIM$: without its leading blanks; a view.
    fn _rt_ltrim(ptr: *const u8, len: i64) -> Str {
        let s = unsafe { arg(ptr, len) };
        let n = s.iter().take_while(|&&c| c == b' ').count();
        view(ptr, n, s.len() - n)
    }

    /// RTRIM$: without its trailing blanks; a view.
    fn _rt_rtrim(ptr: *const u8, len: i64) -> Str {
        let s = unsafe { arg(ptr, len) };
        let n = s.iter().rev().take_while(|&&c| c == b' ').count();
        Str { ptr, len: s.len() - n }
    }

    /// UCASE$: ASCII letters only, as GW-BASIC.
    fn _rt_ucase(ptr: *const u8, len: i64) -> Str {
        map_case(unsafe { arg(ptr, len) }, u8::to_ascii_uppercase)
    }

    /// LCASE$: ASCII letters only.
    fn _rt_lcase(ptr: *const u8, len: i64) -> Str {
        map_case(unsafe { arg(ptr, len) }, u8::to_ascii_lowercase)
    }

    /// HEX$: `n` in upper-case hexadecimal, all 64 bits. Codegen has
    /// already cut a negative INTEGER or LONG to its own width, so HEX$(-1%)
    /// is FFFF.
    fn _rt_hex(n: i64) -> Str {
        radix(n, b"%llX\0")
    }

    /// OCT$: `n` in octal, all 64 bits, as HEX$.
    fn _rt_oct(n: i64) -> Str {
        radix(n, b"%llo\0")
    }

    /// A fresh copy of a string. String assignment copies, so that changing
    /// one variable cannot be seen through another -- or through the shared
    /// literal a string constant points at, which MID$(A$, 1, 1) = "J"
    /// after A$ = "HELLO" would otherwise rewrite for every use of "HELLO".
    fn _rt_strdup(ptr: *const u8, len: i64) -> Str {
        strdup(unsafe { arg(ptr, len) })
    }

    /// MID$(s, start [, count]) = value: overwrite the target's bytes in
    /// place, from the 1-based `start`, with at most `count` characters of
    /// the source and never past the target's end, whose length never
    /// changes. A start below 1 writes nothing. Copied a byte at a time,
    /// forward, so a source overlapping the target reads what was written.
    fn _rt_mid_assign(dst: *mut u8, dst_len: i64, start: i64, count: i64, src: *const u8, src_len: i64) {
        let count = clamp(count, src_len as usize);
        if start < 1 {
            return;
        }
        let from = (start - 1) as usize;
        for i in 0..count {
            let at = from.wrapping_add(i);
            if at >= dst_len as usize {
                break;
            }
            unsafe { *dst.add(at) = *src.add(i) };
        }
    }
}

#[cfg(test)]
#[allow(clippy::approx_constant)] // 3.14159 is a value to print
mod tests {
    use super::*;
    use crate::num::testing::*;

    fn s(r: Str) -> Vec<u8> {
        unsafe { r.bytes() }.to_vec()
    }

    /// `r` was allocated for the caller, who frees it.
    fn freed(r: Str) -> Vec<u8> {
        let v = s(r);
        assert!(!r.ptr.is_null());
        unsafe { sys::free(r.ptr.cast_mut()) };
        v
    }

    /// `r` is a view of `of`.
    fn inside(r: Str, of: &[u8]) -> Vec<u8> {
        let range = of.as_ptr_range();
        assert!(range.start <= r.ptr && r.ptr.wrapping_add(r.len) <= range.end);
        s(r)
    }

    macro_rules! a {
        ($s:expr) => {
            ($s.as_ptr(), $s.len() as i64)
        };
    }

    #[test]
    fn conversions_are_fresh_copies() {
        unsafe {
            assert_eq!(freed(_rt_str(5.0)), b" 5");
            assert_eq!(freed(_rt_str(-0.1 - 0.2)), b"-.3");
            assert_eq!(freed(_rt_str(12345678.0)), b" 12345678");
            assert_eq!(freed(_rt_str_single(12345678.0)), b" 1.234568E+07");
            assert_eq!(freed(_rt_str_single(f64::from(3.14159f32))), b" 3.14159");
            assert_eq!(freed(_rt_chr(65)), b"A");
            assert_eq!(freed(_rt_chr(0)), b"\0");
            assert_eq!(freed(_rt_chr(0x1FF)), b"\xFF");
            // Two results are two buffers.
            let (x, y) = (_rt_chr(1), _rt_chr(2));
            assert_ne!(x.ptr, y.ptr);
            assert_eq!((freed(x), freed(y)), (vec![1], vec![2]));
        }
    }

    #[test]
    fn hex_and_oct() {
        unsafe {
            assert_eq!(freed(_rt_hex(255)), b"FF");
            assert_eq!(freed(_rt_hex(0)), b"0");
            // What codegen passes for HEX$(-1%), and a raw negative.
            assert_eq!(freed(_rt_hex(0xFFFF)), b"FFFF");
            assert_eq!(freed(_rt_hex(-1)), b"FFFFFFFFFFFFFFFF");
            assert_eq!(freed(_rt_oct(8)), b"10");
            assert_eq!(freed(_rt_oct(0xFFFF)), b"177777");
            assert_eq!(freed(_rt_oct(-1)), b"1777777777777777777777");
        }
    }

    #[test]
    fn left_and_right_are_views() {
        let h = b"HELLO";
        unsafe {
            let (p, l) = a!(h);
            assert_eq!(inside(_rt_left(p, l, 2), h), b"HE");
            assert_eq!(inside(_rt_left(p, l, 9), h), b"HELLO");
            assert_eq!(inside(_rt_left(p, l, 0), h), b"");
            assert_eq!(inside(_rt_left(p, l, -1), h), b"HELLO");
            assert_eq!(inside(_rt_right(p, l, 2), h), b"LO");
            assert_eq!(inside(_rt_right(p, l, 9), h), b"HELLO");
            assert_eq!(inside(_rt_right(p, l, 0), h), b"");
            assert_eq!(_rt_left(core::ptr::null(), 0, 3), Str::EMPTY);
            // An empty view still points into its argument.
            assert_eq!(_rt_left(p, l, 0).ptr, p);
            assert_eq!(_rt_right(p, l, 0).ptr, p.add(5));
            assert_eq!(_rt_mid(p, l, 9, 1).ptr, p);
        }
    }

    // As MID$ printed through the assembly runtime.
    #[test]
    fn mid_is_a_view() {
        let h = b"HELLO WORLD";
        unsafe {
            let (p, l) = a!(h);
            assert_eq!(inside(_rt_mid(p, l, 20, -1), h), b"");
            assert_eq!(inside(_rt_mid(p, l, 12, -1), h), b"");
            assert_eq!(inside(_rt_mid(p, l, 11, -1), h), b"D");
            assert_eq!(inside(_rt_mid(p, l, 7, -1), h), b"WORLD");
            assert_eq!(inside(_rt_mid(p, l, 3, 2), h), b"LL");
            assert_eq!(inside(_rt_mid(p, l, 10, 50), h), b"LD");
            assert_eq!(inside(_rt_mid(p, l, 1, 0), h), b"");
            assert_eq!(inside(_rt_mid(p, l, 0, 3), h), b"");
            assert_eq!(inside(_rt_mid(p, l, -5, 3), h), b"");
        }
    }

    // As INSTR printed through the assembly runtime: 5 5 0 1 3 0.
    #[test]
    fn instr_finds_from_start() {
        let h = b"HELLO WORLD";
        let f = |n: &[u8], start| unsafe {
            let (p, l) = a!(h);
            _rt_instr(p, l, n.as_ptr(), n.len() as i64, start)
        };
        assert_eq!(f(b"O", 1), 5);
        assert_eq!(f(b"O", 5), 5);
        assert_eq!(f(b"O", 6), 8);
        assert_eq!(f(b"O", 20), 0);
        assert_eq!(f(b"", 1), 1);
        assert_eq!(f(b"", 3), 3);
        assert_eq!(f(b"", 11), 11);
        assert_eq!(f(b"", 12), 0);
        assert_eq!(f(b"WORLD", 1), 7);
        assert_eq!(f(b"WORLDS", 1), 0);
        assert_eq!(f(b"LD", 10), 10);
        assert_eq!(f(b"HELLO WORLD!", 1), 0);
        assert_eq!(
            unsafe { _rt_instr(core::ptr::null(), 0, b"".as_ptr(), 0, 1) },
            0
        );
    }

    #[test]
    fn instr_start_below_one_is_illegal() {
        if in_child() {
            let f = |start| unsafe { _rt_instr(b"ab".as_ptr(), 2, b"a".as_ptr(), 1, start) };
            assert_eq!(trapped(|| f(0)), (0, true));
            assert_eq!(trapped(|| f(-1)), (0, true));
            assert_eq!(trapped(|| f(1)), (1, false));
            f(0);
            return;
        }
        expect_fatal(
            "string::tests::instr_start_below_one_is_illegal",
            "?Illegal function call\n",
        );
    }

    #[test]
    fn strcat_and_strcmp() {
        unsafe {
            let (p, l) = a!(b"ab");
            let (q, m) = a!(b"cde");
            assert_eq!(freed(_rt_strcat(p, l, q, m)), b"abcde");
            assert_eq!(freed(_rt_strcat(core::ptr::null(), 0, q, m)), b"cde");
            let e = _rt_strcat(core::ptr::null(), 0, core::ptr::null(), 0);
            assert_eq!(freed(e), b"");

            let c = |x: &[u8], y: &[u8]| {
                _rt_strcmp(x.as_ptr(), x.len() as i64, y.as_ptr(), y.len() as i64)
            };
            assert!(c(b"abc", b"abd") < 0);
            assert_eq!(c(b"abc", b"abd"), -1);
            assert_eq!(c(b"b", b"abc"), 1);
            assert_eq!(c(b"ab", b"abc"), -1);
            assert_eq!(c(b"abc", b"ab"), 1);
            assert_eq!(c(b"", b""), 0);
            assert_eq!(c(b"abc", b"abc"), 0);
            // Unsigned bytes, and their difference.
            assert_eq!(c(b"\xFF", b"a"), 0xFF - 0x61);
            assert_eq!(c(b"A", b"a"), -32);
            assert_eq!(_rt_strcmp(core::ptr::null(), 0, core::ptr::null(), 0), 0);
        }
    }

    #[test]
    fn space_fixed_and_string_n() {
        unsafe {
            assert_eq!(freed(_rt_space(3)), b"   ");
            assert_eq!(freed(_rt_space(0)), b"");
            assert_eq!(freed(_rt_space(-4)), b"");
            let (p, l) = a!(b"abc");
            assert_eq!(freed(_rt_fixed(p, l, 5)), b"abc  ");
            assert_eq!(freed(_rt_fixed(p, l, 2)), b"ab");
            assert_eq!(freed(_rt_fixed(p, l, 3)), b"abc");
            assert_eq!(freed(_rt_fixed(p, l, -1)), b"");
            assert_eq!(freed(_rt_fixed(core::ptr::null(), 0, 2)), b"  ");
            assert_eq!(freed(_rt_string_n(3, 120)), b"xxx");
            assert_eq!(freed(_rt_string_n(2, 65)), b"AA");
            assert_eq!(freed(_rt_string_n(-2, 65)), b"");
            assert_eq!(freed(_rt_string_n(1, 0)), b"\0");
        }
    }

    #[test]
    fn trims_are_views() {
        let l = |x: &[u8]| unsafe { inside(_rt_ltrim(x.as_ptr(), x.len() as i64), x) };
        let r = |x: &[u8]| unsafe { inside(_rt_rtrim(x.as_ptr(), x.len() as i64), x) };
        assert_eq!(l(b"   "), b"");
        assert_eq!(r(b"   "), b"");
        assert_eq!(l(b"  a "), b"a ");
        assert_eq!(r(b" a  "), b" a");
        // Blanks only: a tab is kept.
        assert_eq!(l(b"\ta"), b"\ta");
        assert_eq!(r(b"a\t"), b"a\t");
        assert_eq!(unsafe { _rt_ltrim(core::ptr::null(), 0) }.len, 0);
        assert_eq!(unsafe { _rt_rtrim(core::ptr::null(), 0) }.len, 0);
    }

    #[test]
    fn case_is_ascii_and_fresh() {
        unsafe {
            let src = b"aBz{AbZ@\xE9";
            let (p, l) = a!(src);
            assert_eq!(freed(_rt_ucase(p, l)), b"ABZ{ABZ@\xE9");
            assert_eq!(freed(_rt_lcase(p, l)), b"abz{abz@\xE9");
            assert_eq!(src, b"aBz{AbZ@\xE9");
            assert_eq!(freed(_rt_ucase(core::ptr::null(), 0)), b"");
            let (p, l) = a!(b"x\0y");
            assert_eq!(freed(_rt_strdup(p, l)), b"x\0y");
            assert_ne!(_rt_strdup(p, l).ptr, p);
        }
    }

    #[test]
    fn mid_assign_overwrites_in_place() {
        let run = |start, count, src: &[u8]| {
            let mut t = *b"HELLO WORLD";
            unsafe {
                _rt_mid_assign(
                    t.as_mut_ptr(),
                    11,
                    start,
                    count,
                    src.as_ptr(),
                    src.len() as i64,
                )
            };
            t
        };
        // As the assembly runtime printed it.
        assert_eq!(&run(10, 5, b"XYZ"), b"HELLO WORXY");
        assert_eq!(&run(1, i64::MAX, b"ab"), b"abLLO WORLD");
        assert_eq!(&run(1, 1, b"ab"), b"aELLO WORLD");
        assert_eq!(&run(1, -1, b"ab"), b"abLLO WORLD");
        assert_eq!(&run(11, 9, b"xyz"), b"HELLO WORLx");
        assert_eq!(&run(12, 9, b"xyz"), b"HELLO WORLD");
        assert_eq!(&run(0, 9, b"xyz"), b"HELLO WORLD");
        assert_eq!(&run(-3, 9, b"xyz"), b"HELLO WORLD");
        assert_eq!(&run(3, 9, b""), b"HELLO WORLD");
        // A source overlapping the target is read as it is written.
        let mut t = *b"abcdef";
        let p = t.as_mut_ptr();
        unsafe { _rt_mid_assign(p, 6, 2, 9, p, 6) };
        assert_eq!(&t, b"aaaaaa");
    }
}
