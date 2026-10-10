//! PRINT USING, interpreted at run time (using.s)
//!
//! The format is read here as the statement runs, so it may be any string
//! expression and the output may go to a file. `_rt_using_start` names the
//! file and the format; then each value goes through `_rt_using_num` or
//! `_rt_using_str`, which print the literal text up to the next field and
//! the value in it; `_rt_using_end` prints the text after the last field
//! used, up to the next one. A format that runs out before the values does
//! what GW-BASIC does: it starts again from the beginning. A format with no
//! field at all is Illegal function call, and a value of the wrong kind for
//! its field is Type mismatch.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::error::{self, raise};
use crate::file::{_rt_file_print_char, _rt_file_print_string};
use crate::{State, Str, sys};

// The flags a numeric field carries.
/// Group the integer part in thousands.
const COMMA: u32 = 1;
/// Prefix a currency sign, floated against the digits.
const DOLLAR: u32 = 2;
/// Fill with '*' rather than spaces.
const STAR: u32 = 4;
/// Place the sign after the number.
const TRAIL_SIGN: u32 = 8;
/// Always show a sign, '+' for positives.
const FORCE_SIGN: u32 = 16;
/// Render in exponential form.
const EXP: u32 = 32;

/// The widest field, and the most digits after the point. Buffer sizes
/// follow from them: a double's widest %f rendering is a sign, 309 integer
/// digits, a point and MAX_DECIMALS fractional digits -- 351 characters.
const MAX_WIDTH: usize = 255;
const MAX_DECIMALS: usize = 40;

/// A field found in the format.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Field {
    /// A number: width, digits after the point, flags.
    Num(usize, usize, u32),
    /// A string: width, 0 for the whole string.
    Str(usize),
}

/// The statement running.
struct Using {
    /// Where the output goes.
    file: i64,
    fmt: Str,
    /// How far through the format.
    pos: usize,
    /// A field was taken since the format began.
    found: bool,
}

static USING: State<Using> = State::new(Using {
    file: 0,
    fmt: Str::EMPTY,
    pos: 0,
    found: false,
});

/// What the format holds at `i`: a field and the position after it, or
/// None when the character there is text.
///
/// The fields are GW-BASIC's, as LANGREF lists them: `!`, `&` and `\  \`
/// for a string; for a number, digit positions `#` with an optional `.`,
/// `,` among the digits, a leading `+`, `**`, `$$` or `**$`, a trailing
/// `^^^^`, and a trailing `+` or `-`.
fn field_at(f: &[u8], i: usize) -> Option<(Field, usize)> {
    let len = f.len();
    match f[i] {
        b'!' => return Some((Field::Str(1), i + 1)),
        b'&' => return Some((Field::Str(0), i + 1)),
        b'\\' => {
            // Spaces, then '\'.
            let mut j = i + 1;
            while j < len && f[j] == b' ' {
                j += 1;
            }
            if j >= len || f[j] != b'\\' {
                return None;
            }
            return Some((Field::Str((j - i + 1).min(MAX_WIDTH)), j + 1));
        }
        b'#' | b'+' | b'$' | b'*' => {}
        _ => return None,
    }
    let (mut j, mut flags, mut width) = (i, 0u32, 0usize);
    if f[j] == b'+' {
        flags |= FORCE_SIGN; // a leading '+'
        width += 1;
        j += 1;
    }
    if j + 1 < len {
        if f[j] == b'*' && f[j + 1] == b'*' {
            flags |= STAR; // '**', and '**$'
            width += 2;
            j += 2;
            if j < len && f[j] == b'$' {
                flags |= DOLLAR;
                width += 1;
                j += 1;
            }
        } else if f[j] == b'$' && f[j + 1] == b'$' {
            flags |= DOLLAR; // '$$': a sign and a digit
            width += 2;
            j += 2;
        }
    }
    let (mut digits, mut comma) = (0usize, false);
    while j < len {
        match f[j] {
            b'#' => {
                digits += 1;
                width += 1;
            }
            b',' => {
                flags |= COMMA;
                comma = true;
            }
            _ => break,
        }
        j += 1;
    }
    // A comma takes no position itself; the field grows by the separators
    // its digits will need.
    if comma && digits > 0 {
        width += (digits - 1) / 3;
    }
    if digits == 0 && width == 0 {
        return None; // nothing here formed a field
    }
    let mut decimals = 0;
    if j < len && f[j] == b'.' {
        let k = f.iter().skip(j + 1).take_while(|&&c| c == b'#').count();
        // A point with no digits is not part of the field.
        if k > 0 {
            decimals = k.min(MAX_DECIMALS);
            width += decimals + 1;
            j += k + 1;
        }
    }
    if digits == 0 && decimals == 0 {
        return None; // a field needs a digit somewhere
    }
    if f.get(j..j + 4) == Some(b"^^^^") {
        flags |= EXP;
        width += 4;
        j += 4;
    }
    if j < len && (f[j] == b'-' || f[j] == b'+') {
        if f[j] == b'+' {
            flags |= FORCE_SIGN;
        }
        flags |= TRAIL_SIGN;
        width += 1;
        j += 1;
    }
    Some((Field::Num(width.min(MAX_WIDTH), decimals, flags), j))
}

/// Print literal text up to the next field and take the field; with
/// `stop`, at the end of the statement, stop before a field instead, and at
/// the end of the format instead of starting it again. `_` makes the next
/// character literal, and anything that is not a field is printed as it
/// is. None when there is no field to take.
fn next(stop: bool) -> Option<Field> {
    let u = unsafe { USING.get() };
    let (file, fmt) = (u.file, unsafe { u.fmt.bytes() });
    let mut i = u.pos;
    let result = loop {
        if i >= fmt.len() {
            if stop {
                break None; // the statement's end: done
            }
            if !u.found {
                // A whole pass found nothing to fill.
                raise(&error::_err_domain[..], 0);
                break None;
            }
            u.found = false;
            i = 0; // use the format again from its start
            continue;
        }
        let c = fmt[i];
        if c == b'_' && i + 1 < fmt.len() {
            unsafe { _rt_file_print_char(file, fmt[i + 1] as i64) };
            i += 2;
            continue;
        }
        match field_at(fmt, i) {
            Some((field, after)) => {
                if stop {
                    break None;
                }
                i = after;
                u.found = true;
                break Some(field);
            }
            None => {
                unsafe { _rt_file_print_char(file, c as i64) };
                i += 1;
            }
        }
    };
    u.pos = i;
    result
}

/// A layout buffer written front to back. The buffers are sized so that
/// nothing is ever dropped, but a write past the end would be dropped
/// rather than panic: the runtime has no panic machinery to call.
struct Out<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl Out<'_> {
    fn push(&mut self, c: u8) {
        if let Some(b) = self.buf.get_mut(self.len) {
            *b = c;
            self.len += 1;
        }
    }

    fn extend(&mut self, s: &[u8]) {
        for &c in s {
            self.push(c);
        }
    }
}

/// `raw` with a ',' before every third digit of its integer part, which
/// ends at the '.' or 'E', after any leading sign.
fn group(raw: &[u8], out: &mut Out) {
    let int_end = raw
        .iter()
        .position(|&c| c == b'.' || c == b'E')
        .unwrap_or(raw.len());
    let first = usize::from(matches!(raw.first(), Some(b'-' | b'+')));
    for (i, &c) in raw.iter().enumerate() {
        if i < int_end && i > first && (int_end - i) % 3 == 0 {
            out.push(b',');
        }
        out.push(c);
    }
}

/// One numeric field's text, `width` wide unless it does not fit: then it
/// is printed in full, preceded by '%', which is what GW-BASIC does rather
/// than truncating. Its length in `out`.
fn render_num(width: usize, decimals: usize, flags: u32, value: f64, out: &mut [u8]) -> usize {
    let decimals = decimals.min(MAX_DECIMALS);
    let width = width.min(MAX_WIDTH);
    let mut value = value;
    // A trailing sign, or a forced sign placed here, means the number is
    // formatted without one.
    let mut sign = 0u8;
    if flags & (TRAIL_SIGN | FORCE_SIGN) != 0 {
        // NaN is unordered, so it takes the minus sign, as ucomisd did.
        if value >= 0.0 {
            if flags & FORCE_SIGN != 0 {
                sign = b'+';
            }
        } else {
            sign = b'-';
            value = -value;
        }
    }
    let mut raw = [0u8; 512];
    let fmt: &[u8] = if flags & EXP != 0 {
        b"%.*E\0"
    } else {
        b"%.*f\0"
    };
    let n = unsafe {
        sys::snprintf(
            raw.as_mut_ptr().cast(),
            raw.len(),
            fmt.as_ptr().cast(),
            decimals as core::ffi::c_int,
            value,
        )
    };
    let raw = &raw[..(n.max(0) as usize).min(511)];
    // The number, with its commas.
    let mut work = [0u8; 1024];
    let mut body = Out {
        buf: &mut work,
        len: 0,
    };
    if flags & COMMA != 0 {
        group(raw, &mut body);
    } else {
        body.extend(raw);
    }
    let body_len = body.len;
    let lead = sign != 0 && flags & TRAIL_SIGN == 0;
    let dollar = flags & DOLLAR != 0;
    let trail = flags & TRAIL_SIGN != 0;
    let len = usize::from(lead) + usize::from(dollar) + body_len + usize::from(trail);
    let mut o = Out { buf: out, len: 0 };
    // Padded on the left to the field width, or marked as too wide.
    if len < width {
        let fill = if flags & STAR != 0 { b'*' } else { b' ' };
        for _ in len..width {
            o.push(fill);
        }
    } else if len > width {
        o.push(b'%');
    }
    if lead {
        o.push(sign);
    }
    if dollar {
        o.push(b'$');
    }
    o.extend(work.get(..body_len).unwrap_or(&[]));
    // A trailing sign is a column of its own, and a positive number leaves
    // it blank, as GW-BASIC and QuickBASIC do, so the digits line up.
    if trail {
        o.push(if sign != 0 { sign } else { b' ' });
    }
    o.len
}

/// One string field's text: `width` wide, padded on the right or cut, as
/// GW-BASIC does. Its length in `out`. (A width of 0, the whole string,
/// needs no layout.)
fn render_str(s: &[u8], width: usize, out: &mut [u8]) -> usize {
    let width = width.min(MAX_WIDTH);
    let mut o = Out { buf: out, len: 0 };
    o.extend(s.get(..width).unwrap_or(s));
    for _ in o.len..width {
        o.push(b' ');
    }
    o.len
}

/// The layout buffer.
static OUT: State<[u8; 2048]> = State::new([0; 2048]);

/// Write to the statement's file.
fn emit(bytes: &[u8]) {
    let file = unsafe { USING.get() }.file;
    unsafe { _rt_file_print_string(file, bytes.as_ptr(), bytes.len() as i64) };
}

export! {
    /// Begin a PRINT USING statement on file `file` (0 = the console).
    fn _rt_using_start(file: i64, fmt: *const u8, len: i64) {
        let u = unsafe { USING.get() };
        u.file = file;
        u.fmt = Str { ptr: fmt, len: len as usize };
        u.pos = 0;
        u.found = false;
    }

    /// PRINT USING of a number.
    fn _rt_using_num(x: f64) {
        match next(false) {
            Some(Field::Num(w, d, f)) => {
                let out = unsafe { OUT.get() };
                let n = render_num(w, d, f, x, out);
                emit(out.get(..n).unwrap_or(&[]));
            }
            Some(Field::Str(_)) => raise(&error::_err_typemismatch[..], 0),
            None => {} // an error was raised and trapped
        }
    }

    /// PRINT USING of a string.
    fn _rt_using_str(p: *const u8, len: i64) {
        match next(false) {
            Some(Field::Str(0)) => emit(unsafe { Str { ptr: p, len: len as usize }.bytes() }),
            Some(Field::Str(w)) => {
                let s = unsafe { Str { ptr: p, len: len as usize }.bytes() };
                let out = unsafe { OUT.get() };
                let n = render_str(s, w, out);
                emit(out.get(..n).unwrap_or(&[]));
            }
            Some(Field::Num(..)) => raise(&error::_err_typemismatch[..], 0),
            None => {}
        }
    }

    /// Finish a PRINT USING statement: the text after the last field used,
    /// up to the next one.
    fn _rt_using_end() {
        next(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(f: &str) -> Vec<Option<(Field, usize)>> {
        (0..f.len()).map(|i| field_at(f.as_bytes(), i)).collect()
    }

    #[test]
    fn string_fields() {
        assert_eq!(field_at(b"!", 0), Some((Field::Str(1), 1)));
        assert_eq!(field_at(b"&x", 0), Some((Field::Str(0), 1)));
        assert_eq!(field_at(b"\\  \\x", 0), Some((Field::Str(4), 4)));
        assert_eq!(field_at(b"\\\\", 0), Some((Field::Str(2), 2)));
        assert_eq!(field_at(b"\\ x\\", 0), None);
        assert_eq!(field_at(b"\\  ", 0), None);
    }

    #[test]
    fn numeric_fields() {
        let num = |s: &str| field_at(s.as_bytes(), 0);
        assert_eq!(num("###"), Some((Field::Num(3, 0, 0), 3)));
        assert_eq!(num("##.##"), Some((Field::Num(5, 2, 0), 5)));
        assert_eq!(num(".##"), None); // a '.' does not start a field
        assert_eq!(num("#.x"), Some((Field::Num(1, 0, 0), 1)));
        assert_eq!(num("+#.#"), Some((Field::Num(4, 1, FORCE_SIGN), 4)));
        assert_eq!(num("**#.##"), Some((Field::Num(6, 2, STAR), 6)));
        assert_eq!(num("**$##"), Some((Field::Num(5, 0, STAR | DOLLAR), 5)));
        assert_eq!(num("$$##.##"), Some((Field::Num(7, 2, DOLLAR), 7)));
        assert_eq!(num("#,###.##"), Some((Field::Num(8, 2, COMMA), 8)));
        assert_eq!(num("#######,"), Some((Field::Num(9, 0, COMMA), 8)));
        assert_eq!(num("##.##^^^^"), Some((Field::Num(9, 2, EXP), 9)));
        assert_eq!(num("##.##^^^"), Some((Field::Num(5, 2, 0), 5)));
        assert_eq!(num("###-"), Some((Field::Num(4, 0, TRAIL_SIGN), 4)));
        assert_eq!(
            num("###+"),
            Some((Field::Num(4, 0, TRAIL_SIGN | FORCE_SIGN), 4))
        );
        assert_eq!(num("$x"), None);
        assert_eq!(num("**"), None);
        assert_eq!(num("+"), None);
        assert_eq!(num("*"), None);
        // '$$' with no '#': the dollars form the width, but no digit.
        assert_eq!(num("$$"), None);
        assert_eq!(num("$$.#"), Some((Field::Num(4, 1, DOLLAR), 4)));
        let wide = "#".repeat(300);
        assert_eq!(num(&wide), Some((Field::Num(255, 0, 0), 300)));
        let deep = format!("#.{}", "#".repeat(50));
        assert_eq!(num(&deep), Some((Field::Num(42, 40, 0), 52)));
        assert_eq!(fields("a#")[0], None);
    }

    fn num(w: usize, d: usize, f: u32, x: f64) -> String {
        let mut out = [0; 2048];
        let n = render_num(w, d, f, x, &mut out);
        String::from_utf8(out[..n].to_vec()).unwrap()
    }

    #[test]
    fn numbers_render_as_gw_basic() {
        assert_eq!(num(5, 2, 0, 3.14163), " 3.14");
        assert_eq!(num(5, 2, 0, -3.14163), "-3.14");
        assert_eq!(num(3, 0, 0, 12345.0), "%12345");
        assert_eq!(num(5, 0, 0, 12345.0), "12345");
        assert_eq!(num(8, 2, COMMA, 1234.5), "1,234.50");
        assert_eq!(num(12, 0, COMMA, -1234567.0), "  -1,234,567");
        assert_eq!(num(6, 2, STAR, 1.5), "**1.50");
        assert_eq!(num(7, 2, DOLLAR, 1.5), "  $1.50");
        assert_eq!(num(4, 0, TRAIL_SIGN, 5.0), "  5 ");
        assert_eq!(num(4, 0, TRAIL_SIGN, -5.0), "  5-");
        assert_eq!(num(4, 0, TRAIL_SIGN | FORCE_SIGN, 5.0), "  5+");
        assert_eq!(num(4, 1, FORCE_SIGN, 1.25), "+1.2");
        assert_eq!(num(4, 1, FORCE_SIGN, -1.0), "-1.0");
        assert_eq!(num(9, 2, EXP, 1234.5), " 1.23E+03");
        // The widest DOUBLE fits the buffers.
        assert_eq!(num(5, 40, 0, f64::MAX).len(), 351);
    }

    #[test]
    fn strings_render_as_gw_basic() {
        let s = |s: &[u8], w| {
            let mut out = [0; 2048];
            let n = render_str(s, w, &mut out);
            out[..n].to_vec()
        };
        assert_eq!(s(b"hello", 2), b"he");
        assert_eq!(s(b"hi", 4), b"hi  ");
        assert_eq!(s(b"x", 300).len(), 255);
    }

    /// Whole statements, to a file, as the asm runtime printed them.
    #[test]
    fn statements_reuse_the_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("using.txt");
        let name = path.to_str().unwrap();
        unsafe { crate::file::_rt_file_open(name.as_ptr(), name.len() as i64, 1, 6) };
        let start = |f: &str| unsafe { _rt_using_start(6, f.as_ptr(), f.len() as i64) };
        let st = |s: &str| unsafe { _rt_using_str(s.as_ptr(), s.len() as i64) };
        start("a_#b ## c");
        unsafe { _rt_using_num(1.0) };
        unsafe { _rt_using_num(2.0) };
        unsafe { _rt_using_end() };
        unsafe { crate::file::_rt_file_print_newline(6) };
        start("\\  \\|!|&|");
        st("hello");
        st("xyz");
        st("abc");
        unsafe { _rt_using_end() };
        unsafe { crate::file::_rt_file_close(6) };
        let nl = std::str::from_utf8(sys::NEWLINE).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("a#b  1 ca#b  2 c{nl}hell|x|abc|")
        );
    }
}
