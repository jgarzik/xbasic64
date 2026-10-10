//! Numbers as text: GW-BASIC's number formatting, and its number reader
//!
//! Ported from `_rt_fmt_double` / `_rt_fmt_basic` (print.s) and
//! `_rt_scan_number` (string.s). Shared by PRINT, STR$, WRITE, PRINT USING,
//! VAL, INPUT, INPUT # and READ, so they all agree.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::error;
use crate::sys;
use core::ffi::{c_char, c_int};

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

const NONE: Scanned = Scanned {
    consumed: 0,
    value: 0.0,
    overflow: false,
};

/// Read a number the way GW-BASIC's VAL and INPUT do (`_rt_scan_number`).
///
/// Shared by VAL, INPUT, INPUT # and READ, so they all accept the same text:
/// leading blanks; blanks inside the number, which are ignored ("1 2" is
/// 12); a sign; digits with at most one point; an exponent spelled E or D,
/// with an optional sign; or &H, &O, &B or a bare & followed by digits, which
/// are an INTEGER when they fit 16 bits and a LONG when they fit 32 -- two's
/// complement, as a literal. Scanning stops at the first character that
/// cannot continue the number. strtod saw the text directly once, so VAL
/// accepted "inf", "nan" and "0x1F", read past the end of a LEFT$ view, and
/// knew no D.
pub fn scan_number(text: &[u8]) -> Scanned {
    let mut i = 0;
    while i < text.len() && matches!(text[i], b' ' | b'\t') {
        i += 1;
    }
    if i == text.len() {
        return NONE;
    }
    if text[i] == b'&' {
        return scan_radix(text, i + 1);
    }

    // The mantissa's significant digits, for strtod.
    let mut digits = Decimal::EMPTY;
    let mut negative = false;
    // Just past the last digit or point used.
    let mut consumed = 0;
    // 0 sign, 1 mantissa, 2 exponent sign, 3 exponent.
    let mut stage = 0;
    let mut point = false;
    // A digit of the mantissa was read, zero or not.
    let mut mantissa = false;
    // Places to move the kept digits' point by: plus the integer digits past
    // the room, less the fraction digits kept or skipped as leading zeros.
    let mut places: i64 = 0;
    // The exponent as written, saturated far past any DOUBLE's.
    let mut exponent: i64 = 0;
    let mut exponent_negative = false;
    while i < text.len() {
        let c = text[i];
        match c {
            b' ' | b'\t' => {}
            b'.' => {
                // No point in an exponent, nor a second one.
                if stage > 1 || point {
                    break;
                }
                point = true;
                stage = 1;
                consumed = i + 1;
            }
            b'0'..=b'9' if stage < 2 => {
                stage = 1;
                mantissa = true;
                match (digits.push(c), point) {
                    (true, true) => places -= 1,
                    (false, false) => places += 1, // only a place now
                    _ => {}
                }
                consumed = i + 1;
            }
            b'0'..=b'9' => {
                stage = 3;
                exponent = (exponent * 10 + i64::from(c - b'0')).min(EXPONENT_MAX);
                consumed = i + 1;
            }
            // A sign begins the number or its exponent, and nothing else.
            b'+' | b'-' if stage == 0 || stage == 2 => {
                if stage == 0 {
                    negative = c == b'-';
                } else {
                    exponent_negative = c == b'-';
                }
                stage += 1;
            }
            // An exponent only after some of the mantissa.
            b'e' | b'E' | b'd' | b'D' if stage == 1 => stage = 2,
            _ => break,
        }
        i += 1;
    }
    if !mantissa {
        return NONE; // "+", "." and the like are no number
    }
    let exponent = if exponent_negative {
        -exponent
    } else {
        exponent
    };
    let value = digits.value(negative, exponent + places);
    // Infinity is the only way past DOUBLE's range.
    let overflow = !value.is_finite();
    Scanned {
        consumed,
        value: if overflow { 0.0 } else { value },
        overflow,
    }
}

/// An exponent far past any DOUBLE's, with room to add a number's length.
const EXPONENT_MAX: i64 = 1 << 40;

/// A number's significant digits, given to strtod as an integer and an
/// exponent, so that a number of any length keeps its magnitude.
///
/// 767 digits are enough for strtod to round exactly as it would with all of
/// them: no DOUBLE, nor any point halfway between two, has more than 767
/// significant digits, and the digits past the 767th, when any is not 0, are
/// stood for by one sticky 1. So the value is what strtod gives the whole
/// number. No more than that: UCRT's strtod reads only 768 significant
/// digits, so a sticky digit any later is ignored there, and a number just
/// past a halfway point rounded to even.
struct Decimal {
    /// Sign, digits, sticky digit, 'e', exponent, NUL.
    buf: [u8; Decimal::CAP],
    /// Digits kept, from `buf[1]` on.
    len: usize,
    /// A digit past the room was not 0.
    sticky: bool,
}

impl Decimal {
    const DIGITS: usize = 767;
    const CAP: usize = Decimal::DIGITS + 32;

    const EMPTY: Decimal = Decimal {
        buf: [0; Decimal::CAP],
        len: 0,
        sticky: false,
    };

    /// Keep digit `c`: false once there is no room for it. Leading zeros
    /// are not kept, and take none.
    fn push(&mut self, c: u8) -> bool {
        if self.len == 0 && c == b'0' {
            return true;
        }
        if self.len < Decimal::DIGITS
            && let Some(b) = self.buf.get_mut(1 + self.len)
        {
            *b = c;
            self.len += 1;
            return true;
        }
        self.sticky |= c != b'0';
        false
    }

    fn put(&mut self, at: &mut usize, c: u8) {
        if let Some(b) = self.buf.get_mut(*at) {
            *b = c;
            *at += 1;
        }
    }

    /// The digits kept, read as an integer, times ten to `exponent`.
    fn value(mut self, negative: bool, exponent: i64) -> f64 {
        if let Some(b) = self.buf.get_mut(0) {
            *b = if negative { b'-' } else { b'+' };
        }
        let mut at = 1 + self.len;
        if self.len == 0 {
            self.put(&mut at, b'0'); // "-0" is -0, as strtod read it before
        } else {
            // Past 100000 either way, 767 digits are infinity or 0 alike.
            let exponent = exponent.clamp(-100_000, 100_000) - i64::from(self.sticky);
            if self.sticky {
                self.put(&mut at, b'1');
            }
            self.put(&mut at, b'e');
            if exponent < 0 {
                self.put(&mut at, b'-');
            }
            // The exponent's digits, least significant first.
            let mut e = exponent.unsigned_abs();
            let mut text = [0u8; 20];
            let mut n = 0;
            loop {
                if let Some(t) = text.get_mut(n) {
                    *t = b'0' + (e % 10) as u8;
                    n += 1;
                }
                e /= 10;
                if e == 0 {
                    break;
                }
            }
            while n > 0 {
                n -= 1;
                let c = text.get(n).copied().unwrap_or(b'0');
                self.put(&mut at, c);
            }
        }
        self.put(&mut at, 0);
        let start = self.buf.as_ptr().cast::<c_char>();
        unsafe { sys::strtod(start, core::ptr::null_mut()) }
    }
}

/// The digits after `&`, `&H`, `&O` or `&B`, from `text[i]` on.
fn scan_radix(text: &[u8], mut i: usize) -> Scanned {
    let mut radix = 8; // a bare & is octal
    if i < text.len() {
        match text[i] | 32 {
            b'h' => (radix, i) = (16, i + 1),
            b'o' => i += 1,
            b'b' => (radix, i) = (2, i + 1),
            _ => {}
        }
    }
    let mut value: u64 = 0;
    let mut overflow = false; // it no longer fits 32 bits
    let mut consumed = 0;
    while i < text.len() {
        let c = text[i];
        let d = if c.is_ascii_digit() {
            c - b'0'
        } else {
            // 'a' -> 10; anything below 'a' wraps to a value past any radix.
            let d = (c | 32).wrapping_sub(87);
            if d < 10 {
                break;
            }
            d
        };
        if u64::from(d) >= radix {
            break; // not a digit of this radix
        }
        value = value * radix + u64::from(d);
        if value >> 32 != 0 {
            // Keep it bounded; the overflow is recorded.
            overflow = true;
            value &= 0xFFFF_FFFF;
        }
        i += 1;
        consumed = i;
    }
    if consumed == 0 {
        return NONE;
    }
    let value = if overflow {
        0.0
    } else if value <= 0xFFFF {
        f64::from(value as u16 as i16) // an INTEGER, two's complement
    } else {
        f64::from(value as u32 as i32) // a LONG, two's complement
    };
    Scanned {
        consumed,
        value,
        overflow,
    }
}

/// A formatted number: the bytes `buf[start..end]`, no terminator.
///
/// `fmt_double` always leaves a byte free before `start`, so `fmt_basic`
/// adds its blank there, and room after `end` for PRINT's blank, rather
/// than either copying the text. Built without a path that could panic:
/// the runtime object carries no part of `core`'s panic machinery, so a
/// bounds check that could fail would be an undefined symbol at link time.
/// Every index is masked to the buffer instead; no number comes near it.
#[derive(Clone, Copy)]
pub struct NumText {
    buf: [u8; NumText::CAP],
    start: usize,
    end: usize,
}

impl NumText {
    /// A power of two, for the masks. The longest text is about 25 bytes.
    pub const CAP: usize = 64;
    const MASK: usize = NumText::CAP - 1;

    pub fn as_bytes(&self) -> &[u8] {
        self.buf.get(self.start..self.end).unwrap_or(&[])
    }

    /// The text with a blank after it, as PRINT writes a number.
    pub fn with_trailing_blank(mut self) -> NumText {
        self.buf[self.end & NumText::MASK] = b' ';
        self.end += 1;
        self
    }
}

/// Writes a `NumText` front to back through a local cursor, so a digit is a
/// store, not a call.
struct Cursor {
    buf: [u8; NumText::CAP],
    k: usize,
}

impl Cursor {
    /// Text starts at byte 1: byte 0 is `fmt_basic`'s.
    fn new() -> Cursor {
        Cursor {
            buf: [0; NumText::CAP],
            k: 1,
        }
    }

    #[inline(always)]
    fn put(&mut self, c: u8) {
        self.buf[self.k & NumText::MASK] = c;
        self.k += 1;
    }

    /// Append `src` in one copy: per-byte loops unroll into kilobytes.
    fn copy(&mut self, src: &[u8]) {
        if let Some(d) = self.buf.get_mut(self.k..self.k + src.len()) {
            d.copy_from_slice(src);
            self.k += src.len();
        }
    }

    fn done(self) -> NumText {
        NumText {
            buf: self.buf,
            start: 1,
            end: self.k,
        }
    }
}

const FMT_SCI: &[u8] = b"%.*e\0";

/// A whole number of at most `p` digits, as `fmt_double` writes it, without
/// the C library: such a value prints in full, every digit exact, so its
/// text is just its integer's. `None` for any other value.
fn fmt_whole(x: f64, single: bool) -> Option<NumText> {
    // 10^P: past it the value has more digits than the type shows.
    let limit = if single { 1E7 } else { 1E16 };
    // NaN fails both comparisons; -0.0 never gets here.
    if !(x > -limit && x < limit) {
        return None;
    }
    let i = x as i64; // exact within the limit, so the test is exact
    if i as f64 != x {
        return None; // a fraction
    }
    // Written back to front, ending at 24: 16 digits and a sign fit, and
    // byte `start - 1` stays free for `fmt_basic`.
    let mut buf = [0u8; NumText::CAP];
    let end = 24;
    let mut k = end;
    let mut u = i.unsigned_abs();
    loop {
        k -= 1;
        buf[k & NumText::MASK] = b'0' + (u % 10) as u8;
        u /= 10;
        if u == 0 {
            break;
        }
    }
    if i < 0 {
        k -= 1;
        buf[k & NumText::MASK] = b'-';
    }
    Some(NumText { buf, start: k, end })
}

/// A number's digits as GW-BASIC writes them, without the leading blank
/// (`_rt_fmt_double`): 7 significant digits for a SINGLE, 16 for a DOUBLE.
///
/// GW-BASIC's rule, which QuickBASIC documents: a value is rounded to the
/// type's significant digits, P, and written in fixed point when that shows
/// it in at most P digits, otherwise with an exponent. So a SINGLE's 1.1E-6
/// prints .0000011 and 1.1E-7 prints 1.1E-07; 1234567 prints in full and
/// 12345678 as 1.234568E+07. Trailing zeros are dropped, a value below one
/// has no leading zero, and the exponent is spelled E for a SINGLE and D for
/// a DOUBLE, with at least two digits. A SINGLE arrives widened to double.
pub fn fmt_double(x: f64, single: bool) -> NumText {
    // Zero, of either sign, is 0.
    if x == 0.0 {
        let mut out = Cursor::new();
        out.put(b'0');
        return out.done();
    }
    // Whole numbers, every integer variable's among them, need no snprintf.
    match fmt_whole(x, single) {
        Some(t) => t,
        None => fmt_sci(x, single),
    }
}

/// `fmt_double` of a nonzero value, through the C library's rounding.
fn fmt_sci(x: f64, single: bool) -> NumText {
    let p: usize = if single { 7 } else { 16 };
    let mut out = Cursor::new();
    // "[-]d.ddd...e[+-]xx": P significant digits, rounded by the C library.
    let mut sci = [0u8; 64];
    unsafe {
        sys::snprintf(
            sci.as_mut_ptr().cast(),
            sci.len(),
            FMT_SCI.as_ptr().cast(),
            (p - 1) as c_int,
            x,
        )
    };
    // The text after any sign, from `at(0)` on. Indexes are masked to the
    // buffer, which the C library's text never fills, so none can panic.
    let mut start = 0;
    if sci[0] == b'-' {
        out.put(b'-');
        start = 1;
    }
    let at = |i: usize| sci[(start + i) & 63];
    if !at(0).is_ascii_digit() {
        // inf or nan: as the C library wrote it.
        let mut i = 0;
        while at(i) != 0 && i < 63 {
            out.put(at(i));
            i += 1;
        }
        return out.done();
    }

    // The P digits, without the point.
    let mut digits = [0u8; 16];
    digits[0] = at(0);
    // Both are P-1 long: the copy cannot fail.
    if let (Some(d), Some(s)) = (digits.get_mut(1..p), sci.get(start + 2..start + 1 + p)) {
        d.copy_from_slice(s);
    }
    // The decimal exponent X, so the value is d.ddd * 10^X.
    let mut x: i32 = 0;
    let mut i = p + 3;
    while at(i) != 0 && i < 63 {
        x = x * 10 + i32::from(at(i).wrapping_sub(b'0'));
        i += 1;
    }
    if at(p + 2) == b'-' {
        x = -x;
    }
    // n = how many of them are significant: trailing zeros are not.
    let mut n = p;
    while n > 1 && digits[(n - 1) & 15] == b'0' {
        n -= 1;
    }
    let (pi, ni) = (p as i32, n as i32);
    let put = |out: &mut Cursor, from: usize, to: usize| {
        out.copy(digits.get(from..to).unwrap_or(&[]));
    };

    // Fixed point when it shows the value in at most P digits, as GW-BASIC
    // and QuickBASIC do: X+1 integer digits for X >= 0, or for X < 0 the
    // -X-1 zeros after the point and then the n digits.
    if (0..pi).contains(&x) {
        // Past n the digits are the trailing zeros, which these need.
        let int = x as usize + 1;
        put(&mut out, 0, int);
        if int < n {
            out.put(b'.');
            put(&mut out, int, n);
        }
    } else if x < 0 && -x - 1 + ni <= pi {
        out.put(b'.'); // no leading zero, as .5
        // At most P-1 zeros, by the test above.
        const ZEROS: [u8; 16] = [b'0'; 16];
        out.copy(ZEROS.get(..(-x - 1) as usize).unwrap_or(&[]));
        put(&mut out, 0, n);
    } else {
        // d[.ddd]E+xx, D for a DOUBLE.
        out.put(digits[0]);
        if n > 1 {
            out.put(b'.');
            put(&mut out, 1, n);
        }
        out.put(if single { b'E' } else { b'D' });
        out.put(if x < 0 { b'-' } else { b'+' });
        let mut e = x.unsigned_abs();
        if e >= 100 {
            // Two digits at least, three if needed.
            out.put(b'0' + (e / 100) as u8);
            e %= 100;
        }
        out.put(b'0' + (e / 10) as u8);
        out.put(b'0' + (e % 10) as u8);
    }
    out.done()
}

/// `fmt_double` with the blank that stands where a minus sign would go, as
/// PRINT and STR$ write a number (`_rt_fmt_basic`).
///
/// GW-BASIC puts one there for every non-negative number, which is why
/// `PRINT 1; 2` reads " 1  2 " and why STR$(5) is " 5" -- the
/// `MID$(STR$(N), 2)` idiom exists to strip exactly this blank. Not folded
/// into `fmt_double`, because WRITE and PRINT USING render numbers through
/// that and must not gain the blank. `fmt_double` leaves the byte before
/// its text free for it.
pub fn fmt_basic(x: f64, single: bool) -> NumText {
    let mut t = fmt_double(x, single);
    if t.buf[t.start & NumText::MASK] != b'-' {
        // '-' already occupies the sign position; anything else gets a blank.
        t.start = t.start.wrapping_sub(1);
        t.buf[t.start & NumText::MASK] = b' ';
    }
    t
}

export! {
    /// VAL: the number a string begins with, read by `scan_number`. Text
    /// that does not begin with a number is 0, and one too large for a
    /// DOUBLE is Overflow rather than infinity.
    fn _rt_val(ptr: *const u8, len: i64) -> f64 {
        let s = scan_number(unsafe { crate::string::arg(ptr, len) });
        if s.overflow {
            error::raise(&error::_err_overflow[..], 0); // returns only when trapped
        }
        s.value
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! Errors that are not trapped end the process, so a test of one runs
    //! itself again in a child process, which only that test runs in. That
    //! also keeps the trapping state the child arms away from tests running
    //! in parallel in this one.

    /// Whether this is the child process `expect_fatal` started.
    /// Where a child process's test keeps its files: the parent's
    /// directory, which the parent removes.
    pub const TMP_VAR: &str = "XBASIC64_RT_TMP";

    pub fn in_child() -> bool {
        std::env::var_os("XBASIC64_RT_CHILD").is_some()
    }

    /// Run the test `name` (its full path) in a child process, and require
    /// that it end with the fatal report `msg`.
    pub fn expect_fatal(name: &str, msg: &str) {
        // The child's scratch directory, removed here: the child exits
        // without unwinding.
        let tmp = tempfile::tempdir().unwrap();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([name, "--exact", "--nocapture", "--test-threads=1"])
            .env("XBASIC64_RT_CHILD", "1")
            .env(TMP_VAR, tmp.path())
            .output()
            .unwrap();
        // A program puts stderr in binary mode at startup and writes CRLF
        // itself on Windows; this child's stderr is still in text mode, so
        // that comes out as CR CR LF. Either way it is one line end.
        let err = String::from_utf8_lossy(&out.stderr)
            .replace("\r\r\n", "\n")
            .replace("\r\n", "\n");
        assert_eq!(out.status.code(), Some(1), "{name}: {err}");
        assert!(err.contains(msg), "{name}: wanted {msg:?}, got {err:?}");
    }

    /// Run `f` with ON ERROR armed, and return its result and whether it
    /// raised an error; trapping is off again afterwards.
    pub fn trapped<T>(f: impl FnOnce() -> T) -> (T, bool) {
        use crate::error::{_err_active, _err_handler, _err_pending};
        assert!(in_child(), "trapping state is shared by every test thread");
        _err_handler.set(1);
        let r = f();
        let raised = _err_pending.get() != 0;
        _err_pending.set(0);
        _err_active.set(0);
        _err_handler.set(0);
        (r, raised)
    }
}

#[cfg(test)]
#[allow(clippy::approx_constant)] // 3.14159 is a value to print
mod tests {
    use super::testing::*;
    use super::*;

    fn single(x: f32) -> String {
        String::from_utf8(fmt_basic(f64::from(x), true).as_bytes().to_vec()).unwrap()
    }
    fn double(x: f64) -> String {
        String::from_utf8(fmt_basic(x, false).as_bytes().to_vec()).unwrap()
    }

    // The expected text is what STR$ printed through the assembly runtime.
    #[test]
    fn singles_as_the_asm_wrote_them() {
        let cases: &[(f32, &str)] = &[
            (1.1E-6, " .0000011"),
            (1.1E-7, " 1.1E-07"),
            (1234567.0, " 1234567"),
            (12345678.0, " 1.234568E+07"),
            (-1.5, "-1.5"),
            (0.0, " 0"),
            (-0.0, " 0"),
            (0.1 + 0.2, " .3"),
            (3.14159, " 3.14159"),
            (1E+38, " 1E+38"),
            (1.5E-38, " 1.5E-38"),
            (100.0, " 100"),
            (-0.001, "-.001"),
            (1.0 / 3.0, " .3333333"),
            (2.0 / 3.0, " .6666667"),
            (1E-10, " 1E-10"),
            (123456.7, " 123456.7"),
            (9999999.0, " 9999999"),
            (99999999.0, " 1E+08"),
            (-12345678.0, "-1.234568E+07"),
            (0.5, " .5"),
            (1000000.0, " 1000000"),
            (10000000.0, " 1E+07"),
            (0.000001, " .000001"),
            (0.0000001, " .0000001"),
            (-1.1E-7, "-1.1E-07"),
            (1.234567E-5, " 1.234567E-05"),
        ];
        for &(x, want) in cases {
            assert_eq!(single(x), want, "{x:e}");
        }
    }

    #[test]
    fn doubles_as_the_asm_wrote_them() {
        let cases: &[(f64, &str)] = &[
            (1.1E-6, " .0000011"),
            (1.1E-7, " .00000011"),
            (1234567.0, " 1234567"),
            (12345678.0, " 12345678"),
            (-1.5, "-1.5"),
            (0.0, " 0"),
            (0.1 + 0.2, " .3"),
            (3.14159, " 3.14159"),
            (1E+300, " 1D+300"),
            (1E-300, " 1D-300"),
            (1.5E-100, " 1.5D-100"),
            (1234567890123456.0, " 1234567890123456"),
            (12345678901234567.0, " 1.234567890123457D+16"),
            (1E+16, " 1D+16"),
            (1E+15, " 1000000000000000"),
            (1.0 / 3.0, " .3333333333333333"),
            (-2.0 / 3.0, "-.6666666666666666"),
            (4.9E-324, " 4.940656458412465D-324"),
            (f64::MAX, " 1.797693134862316D+308"),
            (123.456, " 123.456"),
            (1E+100, " 1D+100"),
            (1E-16, " .0000000000000001"),
            (1E-17, " 1D-17"),
            (1.5E-16, " 1.5D-16"),
            (-1E+16, "-1D+16"),
            (9999999999999999.0, " 1D+16"),
            (0.1, " .1"),
            (1E+22, " 1D+22"),
            (2.5E-5, " .000025"),
            (-0.00000000123, "-.00000000123"),
        ];
        for &(x, want) in cases {
            assert_eq!(double(x), want, "{x:e}");
        }
    }

    #[test]
    fn whole_numbers_print_as_the_c_library_rounds_them() {
        // The fast path's boundaries, and either side of each.
        let cases: &[(f64, bool, &str)] = &[
            (-0.0, false, " 0"),
            (-0.0, true, " 0"),
            (1.0, false, " 1"),
            (-1.0, true, "-1"),
            (0.5, false, " .5"),
            (-0.5, true, "-.5"),
            (1.5, true, " 1.5"),
            (9999999.0, true, " 9999999"),
            (-9999999.0, true, "-9999999"),
            (10000000.0, true, " 1E+07"),
            (-10000000.0, true, "-1E+07"),
            (9999999.5, true, " 1E+07"),
            (9999999.4, true, " 9999999"),
            (16777216.0, true, " 1.677722E+07"),
            (10000000.0, false, " 10000000"),
            (999999999999999.0, false, " 999999999999999"),
            (1E15, false, " 1000000000000000"),
            (-1E15, false, "-1000000000000000"),
            (1E15 + 1.0, false, " 1000000000000001"),
            (9999999999999998.0, false, " 9999999999999998"),
            (-9999999999999998.0, false, "-9999999999999998"),
            (1E16, false, " 1D+16"),
            (1E16 + 2.0, false, " 1D+16"),
            (-1E16, false, "-1D+16"),
            (4503599627370497.0, false, " 4503599627370497"),
            (2147483647.0, false, " 2147483647"),
            (-2147483648.0, false, "-2147483648"),
            (32767.0, true, " 32767"),
            (-32768.0, true, "-32768"),
            (1E300, false, " 1D+300"),
        ];
        for &(x, single, want) in cases {
            let got = fmt_basic(x, single);
            assert_eq!(got.as_bytes(), want.as_bytes(), "{x:e} single={single}");
            assert_eq!(
                got.with_trailing_blank().as_bytes(),
                format!("{want} ").as_bytes()
            );
        }
        // The fast path agrees with the C library's for every whole number
        // it takes, near zero and near each limit.
        for (limit, single) in [(1E7, true), (1E16, false)] {
            for k in 0..2000 {
                // Past 2^53 a DOUBLE holds only the even integers.
                for x in [k as f64, limit - 2.0 - k as f64 * 2.0] {
                    for x in [x, -x] {
                        if x == 0.0 {
                            continue;
                        }
                        let fast = fmt_whole(x, single).expect("whole and in range");
                        assert_eq!(
                            fast.as_bytes(),
                            fmt_sci(x, single).as_bytes(),
                            "{x:e} single={single}"
                        );
                    }
                }
            }
            assert!(fmt_whole(limit, single).is_none());
            assert!(fmt_whole(-limit, single).is_none());
        }
        assert!(fmt_whole(0.5, false).is_none());
        assert!(fmt_whole(f64::NAN, false).is_none());
        assert!(fmt_whole(f64::INFINITY, true).is_none());
    }

    #[test]
    fn fmt_double_has_no_blank() {
        assert_eq!(fmt_double(5.0, false).as_bytes(), b"5");
        assert_eq!(fmt_double(-5.0, true).as_bytes(), b"-5");
        assert_eq!(fmt_double(0.0, true).as_bytes(), b"0");
        assert_eq!(fmt_double(1E-7, false).as_bytes(), b".0000001");
    }

    #[test]
    fn infinity_and_nan_as_the_c_library_spells_them() {
        assert_eq!(fmt_double(f64::INFINITY, false).as_bytes(), b"inf");
        assert_eq!(fmt_basic(f64::NEG_INFINITY, true).as_bytes(), b"-inf");
        assert_eq!(fmt_basic(f64::NAN, false).as_bytes(), b" nan");
    }

    fn val(s: &str) -> f64 {
        unsafe { _rt_val(s.as_ptr(), s.len() as i64) }
    }

    // The expected values are what VAL returned through the assembly runtime.
    #[test]
    fn val_as_the_asm_read_it() {
        let cases: &[(&str, f64)] = &[
            ("1 2", 12.0),
            (" -3.5e2x", -350.0),
            ("1d3", 1000.0),
            ("1D-2", 0.01),
            ("&H1F", 31.0),
            ("&HFFFF", -1.0),
            ("&H10000", 65536.0),
            ("&HFFFFFFFF", -1.0),
            ("&h7fffffff", 2147483647.0),
            ("&H80000000", -2147483648.0),
            ("&O17", 15.0),
            ("&17", 15.0),
            ("&B101", 5.0),
            ("&b2", 0.0),
            ("&H", 0.0),
            ("&", 0.0),
            ("abc", 0.0),
            ("", 0.0),
            ("+", 0.0),
            (".", 0.0),
            ("-.5", -0.5),
            ("1e", 1.0),
            ("1e+", 1.0),
            ("12abc", 12.0),
            ("1.2.3", 1.2),
            ("  +  4 5 . 5", 45.5),
            ("1-2", 1.0),
            ("0x1F", 0.0),
            ("inf", 0.0),
            ("&HG", 0.0),
            ("--1", 0.0),
            ("1E5E3", 100000.0),
            ("1,000", 1.0),
            (".5.5", 0.5),
            ("e5", 0.0),
            ("1 e 2", 100.0),
            ("&H 1F", 0.0),
        ];
        for &(s, want) in cases {
            assert_eq!(val(s), want, "VAL({s:?})");
        }
    }

    #[test]
    fn scan_reports_what_it_consumed() {
        let s = |t: &str| scan_number(t.as_bytes());
        assert_eq!(s("  12  ").consumed, 4);
        assert_eq!(s("1 2,3").consumed, 3);
        // The exponent's letter and sign count only once a digit follows.
        assert_eq!(s("1e+").consumed, 1);
        assert_eq!(s("1e+5x").consumed, 4);
        assert_eq!(s("-.5").consumed, 3);
        assert_eq!(s("&H1Fz").consumed, 4);
        assert_eq!(s("&Hz"), NONE);
        assert_eq!(s("   "), NONE);
        assert_eq!(s("x1"), NONE);
        // Only the text given is read: a view stops where its length says.
        assert_eq!(scan_number(&b"123"[..2]).value, 12.0);
    }

    #[test]
    fn scan_reports_overflow() {
        let s = scan_number(b"1e999");
        assert!(s.overflow);
        assert_eq!((s.consumed, s.value), (5, 0.0));
        let s = scan_number(b"&HFFFFFFFFF,");
        assert!(s.overflow);
        assert_eq!((s.consumed, s.value), (11, 0.0));
        assert!(!scan_number(b"1.7976931348623157D308").overflow);
        // Too small is 0, not an overflow.
        let s = scan_number(b"1e-999");
        assert!(!s.overflow && s.value == 0.0 && s.consumed == 6);
    }

    #[test]
    fn scan_reads_a_number_of_any_length() {
        let s = |t: &str| scan_number(t.as_bytes());
        let long = "1".repeat(200);
        assert_eq!(s(&long).consumed, 200);
        assert_eq!(s(&long).value, long.parse::<f64>().unwrap());
        // Digits past the room still count as places.
        let big = format!("1{}", "0".repeat(129));
        assert_eq!(s(&big).value, 1e129);
        assert_eq!(s(&format!("{big}.5x")).consumed, 132);
        let huge = format!("0.{}9", "1".repeat(2000));
        assert_eq!(s(&huge).value, huge.parse::<f64>().unwrap());
        assert!(s(&"9".repeat(400)).overflow);
        // Leading zeros take no room, before the point or after it.
        let small = format!("0.{}5", "0".repeat(200));
        assert_eq!(s(&small).value, 5e-201);
        assert_eq!(s(&small).consumed, small.len());
        assert_eq!(s(&format!("{}1.5", "0".repeat(1000))).value, 1.5);
        assert_eq!(s(&format!("-.{}1e990", "0".repeat(1000))).value, -1e-11);
        // The fraction past the room still rounds: exactly halfway
        // between two DOUBLEs rounds to even, a hair above it rounds up.
        let half = "9007199254740993"; // 2^53 + 1
        assert_eq!(s(half).value, 9007199254740992.0);
        let above = format!("{half}.{}1", "0".repeat(1000));
        assert_eq!(s(&above).value, 9007199254740994.0);
        // Exponents and signs as before.
        assert_eq!(s(&format!("{}e-129", big)).value, 1.0);
        assert_eq!(s(&format!("0.{}1e201", "0".repeat(199))).value, 10.0);
        assert_eq!(s("-0").value.to_bits(), (-0.0f64).to_bits());
        assert!(s("1e99999999999999999999").overflow);
        assert_eq!(s("1e-99999999999999999999").value, 0.0);
    }

    #[test]
    fn val_overflow_is_an_error() {
        if in_child() {
            assert_eq!(trapped(|| val("1e999")), (0.0, true));
            assert_eq!(trapped(|| val("&H100000000")), (0.0, true));
            val("1D400");
            return;
        }
        expect_fatal("num::tests::val_overflow_is_an_error", "?Overflow\n");
    }
}
