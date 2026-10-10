//! Console input: INPUT and LINE INPUT (input.s)
//!
//! INPUT reads a whole line and splits it on commas, as GW-BASIC and
//! QuickBASIC do: a field may be quoted, which lets it hold a comma; an
//! unquoted one has its surrounding blanks dropped. Every field is checked
//! against the variable it is for before any is stored: a number must be one
//! and nothing else (read by `num::scan_number`, as VAL reads one) and must
//! fit the variable's type; a quoted field is a string only. A line that
//! fails any of that, or has too few fields, prints "?Redo from start" and
//! asks again -- prompt and all. More fields than variables print "?Extra
//! ignored". At the end of the input every variable reads as empty, rather
//! than the statement waiting forever.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::file::{con_string, set_console_column};
use crate::{State, Str, num, sys};

/// Sema refuses an INPUT with more variables.
const MAX_INPUT_FIELDS: usize = 64;

/// Longest line kept; the rest of a longer one is dropped.
const MAX_LINE: usize = 1023;

/// The line read, and the fields INPUT found in it.
struct Input {
    line: [u8; MAX_LINE + 1],
    /// Where each field starts in `line`, and its length.
    start: [usize; MAX_INPUT_FIELDS],
    len: [usize; MAX_INPUT_FIELDS],
    /// A numeric field's value.
    value: [f64; MAX_INPUT_FIELDS],
}

static INPUT: State<Input> = State::new(Input {
    line: [0; MAX_LINE + 1],
    start: [0; MAX_INPUT_FIELDS],
    len: [0; MAX_INPUT_FIELDS],
    value: [0.0; MAX_INPUT_FIELDS],
});

fn input() -> &'static mut Input {
    unsafe { INPUT.get() }
}

/// Read one line from stdin into `Input::line`: its length, and whether the
/// input had already ended so that nothing was read.
///
/// A byte at a time, so a line is all that is taken. The newline is
/// consumed and not kept, as is any CR. A line longer than the buffer keeps
/// its first MAX_LINE characters. Output is flushed first, so a prompt is
/// on the screen before the program waits; and the console's column is 0
/// afterwards, since the newline the user typed put the cursor there.
fn read_line() -> (usize, bool) {
    unsafe { sys::fflush(sys::stdout()) };
    let line = &mut input().line;
    let (mut len, mut any) = (0, false);
    loop {
        let c = unsafe { sys::fgetc(sys::stdin()) };
        if c < 0 {
            break;
        }
        any = true;
        match c {
            10 => break,
            13 => {} // a CR before the newline is not text
            _ if len < MAX_LINE => {
                line[len] = c as u8;
                len += 1;
            }
            _ => {} // past the buffer: dropped
        }
    }
    let len = len.min(MAX_LINE);
    line[len] = 0;
    set_console_column(0);
    (len, !any)
}

/// `x` converted to an integer as the CPU's cvtsd2si does: rounded to
/// nearest, ties to even, and i64::MIN when out of range or NaN.
fn round_to_i64(x: f64) -> i64 {
    const TWO52: f64 = 4503599627370496.0;
    const TWO63: f64 = 9223372036854775808.0;
    if x.is_nan() || x.abs() >= TWO63 {
        i64::MIN
    } else if x.abs() < TWO52 {
        // Adding 2^52 leaves no bits for a fraction, so the addition rounds
        // as the FPU does: to nearest, ties to even.
        let r = (x.abs() + TWO52) - TWO52;
        (if x < 0.0 { -r } else { r }) as i64
    } else {
        x as i64 // already a whole number
    }
}

/// Does `x` fit a variable of type `ty` ('I', 'L', 'S', else DOUBLE),
/// rounded as an assignment rounds?
fn fits(x: f64, ty: u8) -> bool {
    match ty {
        b'I' => {
            let r = round_to_i64(x);
            r as i16 as i64 == r
        }
        b'L' => {
            let r = round_to_i64(x);
            r as i32 as i64 == r
        }
        b'S' => !(x as f32).is_infinite(),
        _ => true,
    }
}

/// What reading one line for an INPUT statement came to.
#[derive(Debug, PartialEq)]
enum Parsed {
    /// Every variable has its field.
    Done,
    /// Every variable has its field and the line goes on.
    Extra,
    /// The line does not do: ask again.
    Redo,
}

/// Split `line` into fields for variables of `types`, checking each, into
/// `start`, `len` and `value`.
///
/// Nothing here may panic -- the runtime has no panic machinery to call --
/// so bytes are read through `get` wherever the bound is not plain.
fn parse(
    line: &[u8],
    types: &[u8],
    start: &mut [usize; MAX_INPUT_FIELDS],
    flen: &mut [usize; MAX_INPUT_FIELDS],
    value: &mut [f64; MAX_INPUT_FIELDS],
) -> Parsed {
    let n = line.len();
    let mut i = 0; // where in the line
    let mut k = 0; // which field
    loop {
        let (Some(&ty), true) = (types.get(k), k < MAX_INPUT_FIELDS) else {
            return Parsed::Extra; // never: k < types.len() <= the maximum
        };
        // Leading blanks are not part of a field.
        while i < n && matches!(line[i], b' ' | b'\t') {
            i += 1;
        }
        let quoted = i < n && line[i] == b'"';
        if quoted {
            // A quoted field runs to the closing quote (or the end of the
            // line); only blanks may follow it.
            i += 1;
            start[k] = i;
            while i < n && line[i] != b'"' {
                i += 1;
            }
            flen[k] = i - start[k];
            if i < n {
                i += 1;
                while i < n && line[i] != b',' {
                    if !matches!(line[i], b' ' | b'\t') {
                        return Parsed::Redo; // text after a quoted field
                    }
                    i += 1;
                }
            }
        } else {
            // An unquoted field runs to the next comma, less trailing blanks.
            start[k] = i;
            while i < n && line[i] != b',' {
                i += 1;
            }
            let mut end = i;
            while end > start[k] && matches!(line.get(end - 1), Some(b' ' | b'\t')) {
                end -= 1;
            }
            flen[k] = end - start[k];
        }
        if ty != b'$' {
            if quoted {
                return Parsed::Redo; // a quoted field is not a number
            }
            let field = line.get(start[k]..start[k] + flen[k]).unwrap_or(&[]);
            value[k] = if field.is_empty() {
                0.0 // an empty field is 0
            } else {
                let s = num::scan_number(field);
                // Too large for a DOUBLE, not a number, not only one, or
                // too large for the variable.
                if s.overflow || s.consumed != field.len() || !fits(s.value, ty) {
                    return Parsed::Redo;
                }
                s.value
            };
        }
        k += 1;
        if i >= n {
            return if k < types.len() {
                Parsed::Redo // too few fields
            } else {
                Parsed::Done
            };
        }
        i += 1; // past the comma
        if k >= types.len() {
            return Parsed::Extra;
        }
    }
}

/// Write `msg` on a line of its own.
fn say(msg: &[u8]) {
    con_string(msg);
    unsafe { crate::file::_rt_file_print_newline(0) };
}

export! {
    /// LINE INPUT: one whole line, as typed, valid until the next read;
    /// empty at the end of the input.
    fn _rt_input_string() -> Str {
        let (len, _) = read_line();
        Str {
            ptr: input().line.as_ptr(),
            len,
        }
    }

    /// INPUT: read and check one line for a whole statement. Then
    /// `_rt_input_field_num` and `_rt_input_field_str` hand out the values.
    ///
    /// `prompt` already includes the "? " when the statement asks for one;
    /// `types` holds one character per variable: '$' string, 'I' INTEGER,
    /// 'L' LONG, 'S' SINGLE, 'D' DOUBLE.
    fn _rt_input_line(prompt: *const u8, prompt_len: i64, types: *const u8, count: i64) {
        let prompt = unsafe { Str { ptr: prompt, len: prompt_len as usize }.bytes() };
        let count = (count.max(0) as usize).min(MAX_INPUT_FIELDS);
        let types = unsafe { Str { ptr: types, len: count }.bytes() };
        loop {
            con_string(prompt);
            let (len, ended) = read_line();
            let inp = input();
            if ended {
                // Nothing more to read: every variable gets an empty value.
                inp.len[..count].fill(0);
                inp.value[..count].fill(0.0);
                return;
            }
            let line = inp.line.get(..len).unwrap_or(&[]);
            match parse(line, types, &mut inp.start, &mut inp.len, &mut inp.value) {
                Parsed::Done => return,
                Parsed::Extra => {
                    say(b"?Extra ignored");
                    return;
                }
                Parsed::Redo => say(b"?Redo from start"),
            }
        }
    }

    /// The value INPUT read for numeric variable `k`.
    fn _rt_input_field_num(k: i64) -> f64 {
        input().value.get(k as usize).copied().unwrap_or(0.0)
    }

    /// The text INPUT read for string variable `k`: a copy, since the line
    /// buffer is reused by the next read.
    fn _rt_input_field_str(k: i64) -> Str {
        let inp = input();
        let k = k as usize;
        let (Some(&start), Some(&len)) = (inp.start.get(k), inp.len.get(k)) else {
            return Str::EMPTY;
        };
        match inp.line.get(start..start + len) {
            Some(s) if !s.is_empty() => crate::string::strdup(s),
            _ => Str::EMPTY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(line: &str, types: &str) -> (Parsed, Vec<String>) {
        let (mut s, mut l, mut v) = ([0; 64], [0; 64], [0.0; 64]);
        let p = parse(line.as_bytes(), types.as_bytes(), &mut s, &mut l, &mut v);
        let fields = (0..types.len())
            .map(|k| line.get(s[k]..s[k] + l[k]).unwrap_or("").to_string())
            .collect();
        (p, fields)
    }

    #[test]
    fn strings_split_on_commas() {
        assert_eq!(
            run("a, b ,c", "$$$"),
            (Parsed::Done, vec!["a".into(), "b".into(), "c".into()])
        );
        assert_eq!(
            run(" \"x, y\" , z", "$$"),
            (Parsed::Done, vec!["x, y".into(), "z".into()])
        );
        assert_eq!(run("\"open", "$"), (Parsed::Done, vec!["open".into()]));
        assert_eq!(run("a,b,c", "$$").0, Parsed::Extra);
        assert_eq!(run("a", "$$").0, Parsed::Redo);
        assert_eq!(run("\"a\"b", "$").0, Parsed::Redo);
        assert_eq!(run("", "$"), (Parsed::Done, vec!["".into()]));
        assert_eq!(run("a,", "$$"), (Parsed::Done, vec!["a".into(), "".into()]));
    }

    #[test]
    fn a_quoted_field_is_no_number() {
        assert_eq!(run("\"1\"", "D").0, Parsed::Redo);
    }

    #[test]
    fn empty_numeric_fields_are_zero() {
        // Reaches no number reader: every field is empty.
        let (mut s, mut l, mut v) = ([0; 64], [0; 64], [9.0; 64]);
        assert_eq!(parse(b" , ", b"DI", &mut s, &mut l, &mut v), Parsed::Done);
        assert_eq!(&v[..2], &[0.0, 0.0]);
    }

    #[test]
    fn values_must_fit_their_type() {
        assert!(fits(32767.0, b'I'));
        assert!(fits(32767.4, b'I'));
        assert!(!fits(32767.5, b'I'));
        assert!(fits(-32768.5, b'I')); // ties to even: -32768
        assert!(!fits(-32769.0, b'I'));
        assert!(fits(2147483647.0, b'L'));
        assert!(!fits(2147483648.0, b'L'));
        assert!(!fits(1e300, b'L'));
        assert!(fits(3e38, b'S'));
        assert!(!fits(4e38, b'S'));
        assert!(fits(1e300, b'D'));
        assert_eq!(round_to_i64(2.5), 2);
        assert_eq!(round_to_i64(3.5), 4);
        assert_eq!(round_to_i64(-2.5), -2);
        assert_eq!(round_to_i64(f64::NAN), i64::MIN);
        assert_eq!(round_to_i64(1e19), i64::MIN);
        assert_eq!(round_to_i64(-9007199254740993.0), -9007199254740992);
    }
}
