//! RND, RANDOMIZE, TIMER, DATE$, TIME$, BEEP, CLS, LOCATE, COLOR, POS (math.s)
//!
//! Ported from math.s. The generator is xorshift64; the clock and local time
//! come from `sys`; the console is driven with ANSI escapes written to the
//! C library's stdout, the stream every other console write goes through.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::file::{console_column, set_console_column};
use crate::{State, Str, sys};
use core::ffi::{c_char, c_int};

// `crate::file` owns the console's column (the asm's `_file_col[0]`), and
// is expected to provide:
//
//   pub fn console_column() -> i64      characters already written on the
//                                       console's current line, 0 at its start
//   pub fn set_console_column(c: i64)   set that count
//
// POS reads it; CLS sets it to 0, and LOCATE with a column to that column
// less one. Nothing here writes through the file module, so these escapes
// do not move it themselves.

/// The generator's starting state; also what a seed that hashes to zero
/// becomes, since zero is xorshift's fixed point.
const SEED: u64 = 0x12345678DEADBEEF;

/// xorshift64's state.
static RNG_STATE: State<u64> = State::new(SEED);
/// The last number RND returned, for RND(0).
static RNG_LAST: State<f64> = State::new(0.0);

const CLS_SEQ: &[u8] = b"\x1b[2J\x1b[H";
const LOCATE_FMT: &[u8] = b"\x1b[%d;%dH\0";
const LOCATE_ROW_FMT: &[u8] = b"\x1b[%dd\0";
const LOCATE_COL_FMT: &[u8] = b"\x1b[%dG\0";
const COLOR_FMT: &[u8] = b"\x1b[%d;%d;%dm\0";
const COLOR_FG_FMT: &[u8] = b"\x1b[%d;%dm\0";
const COLOR_BG_FMT: &[u8] = b"\x1b[%dm\0";
const DATE_FMT: &[u8] = b"%m-%d-%Y\0";
const TIME_FMT: &[u8] = b"%H:%M:%S\0";

/// GW-BASIC colour 0-7 -> ANSI colour: black blue green cyan red magenta
/// brown white. GW-BASIC's 1 is blue and 4 red, ANSI's the other way round.
const GW_TO_ANSI: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

/// An escape sequence, formatted in place.
struct Esc {
    buf: [u8; 64],
    len: usize,
}

impl Esc {
    fn new() -> Esc {
        Esc {
            buf: [0; 64],
            len: 0,
        }
    }

    /// Format with `fmt` (NUL-terminated) and up to three ints. The values
    /// go through `%d` as the asm passed them: as C ints, a wider one
    /// truncated.
    fn format(&mut self, fmt: &[u8], a: c_int, b: c_int, c: c_int) {
        let n = unsafe {
            sys::snprintf(
                self.buf.as_mut_ptr().cast::<c_char>(),
                self.buf.len(),
                fmt.as_ptr().cast(),
                a,
                b,
                c,
            )
        };
        self.len = (n.max(0) as usize).min(self.buf.len() - 1);
    }

    fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

/// LOCATE's escape: CUP moves both coordinates, VPA only the row, CHA only
/// the column; -1 (any negative) is omitted. Neither: nothing.
fn locate_seq(row: i64, col: i64) -> Esc {
    let mut e = Esc::new();
    if row >= 0 && col >= 0 {
        e.format(LOCATE_FMT, row as c_int, col as c_int, 0);
    } else if row >= 0 {
        e.format(LOCATE_ROW_FMT, row as c_int, 0, 0);
    } else if col >= 0 {
        e.format(LOCATE_COL_FMT, col as c_int, 0, 0);
    }
    e
}

/// COLOR's escape. The background 0-7 is ANSI 40-47 and 8-15 is 100-107;
/// the foreground 0-7 is 30-37 and 8-15 is 90-97, and 16-31 is the same
/// colour blinking, so every foreground also sets or clears blink (SGR 5 /
/// 25). A negative one is omitted, and left as it was.
fn color_seq(fg: i64, bg: i64) -> Esc {
    let ansi = |c: i64, base: c_int| {
        let a = GW_TO_ANSI[(c & 7) as usize] as c_int + base;
        if c < 8 { a } else { a + 60 }
    };
    let mut e = Esc::new();
    let bg = if bg >= 0 { ansi(bg, 40) } else { -1 };
    if fg >= 0 {
        let (blink, fg) = if fg >= 16 { (5, fg - 16) } else { (25, fg) };
        let fg = ansi(fg, 30);
        if bg >= 0 {
            e.format(COLOR_FMT, blink, fg, bg);
        } else {
            e.format(COLOR_FG_FMT, blink, fg, 0);
        }
    } else if bg >= 0 {
        e.format(COLOR_BG_FMT, bg, 0, 0);
    }
    e
}

/// Write `bytes` to the console: the C library's stdout, which every other
/// console write goes through too, so they stay in order.
#[cfg(not(test))]
fn console(bytes: &[u8]) {
    unsafe { sys::write(sys::stdout(), bytes) };
}

// Tests capture what would have been written, rather than clear the
// terminal running them.
#[cfg(test)]
std::thread_local! {
    static CONSOLE: core::cell::RefCell<Vec<u8>> = const { core::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn console(bytes: &[u8]) {
    CONSOLE.with(|c| c.borrow_mut().extend_from_slice(bytes));
}

/// The next number from xorshift64, in [0, 1): its top 52 bits as the
/// mantissa of a double in [1, 2), less 1.
fn next() -> f64 {
    let rng = unsafe { RNG_STATE.get() };
    let mut s = *rng;
    s ^= s << 13;
    s ^= s >> 7;
    s ^= s << 17;
    *rng = s;
    let x = f64::from_bits((s >> 12) | 0x3FF0000000000000) - 1.0;
    unsafe { *RNG_LAST.get() = x };
    x
}

/// `fmt` applied to the local time now, as a fresh string.
fn strftime_now(fmt: &[u8]) -> Str {
    let tm = sys::local_time(sys::now().0);
    let mut buf = [0u8; 64];
    let n = unsafe {
        sys::strftime(
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len(),
            fmt.as_ptr().cast(),
            &tm,
        )
    };
    // Fresh: the caller may hold another such result.
    crate::string::strdup(buf.get(..n).unwrap_or(&[]))
}

export! {
    /// RND(x): GW-BASIC's argument selects the behaviour. RND(<0) reseeds
    /// from that value and returns the next number, RND(0) returns the
    /// previous number again, and RND(>0) -- a bare RND, and NaN -- the next.
    fn _rt_rnd(x: f64) -> f64 {
        if x == 0.0 {
            return unsafe { *RNG_LAST.get() };
        }
        if x < 0.0 {
            unsafe { _rt_randomize(x) };
        }
        next()
    }

    /// RANDOMIZE seed. A bare RANDOMIZE is compiled as RANDOMIZE TIMER.
    ///
    /// The seed's bit pattern goes through a splitmix64 avalanche rather than
    /// being used directly: BASIC seeds are small integers, and xorshift64
    /// started from a small state gives a visibly poor first few values.
    fn _rt_randomize(seed: f64) {
        let mut z = seed.to_bits().wrapping_add(0x9E3779B97F4A7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^= z >> 31;
        unsafe { *RNG_STATE.get() = if z == 0 { SEED } else { z } };
    }

    /// BEEP: ring the terminal bell.
    fn _rt_beep() {
        console(b"\x07");
    }

    /// DATE$: the date as MM-DD-YYYY, GW-BASIC's shape.
    fn _rt_date() -> Str {
        strftime_now(DATE_FMT)
    }

    /// TIME$: the time as HH:MM:SS.
    fn _rt_time() -> Str {
        strftime_now(TIME_FMT)
    }

    /// POS(n): the column the next character goes to, from 1.
    fn _rt_pos() -> i32 {
        (console_column() + 1) as i32
    }

    /// LOCATE row, col: move the cursor; -1 = omitted, which stays where it
    /// is. A column moves the console's column too, so that a following TAB
    /// or PRINT zone counts from where the cursor is; a row alone does not.
    fn _rt_locate(row: i64, col: i64) {
        console(locate_seq(row, col).bytes());
        if col >= 0 {
            set_console_column(col - 1);
        }
    }

    /// COLOR foreground, background; -1 = omitted, left as it was.
    fn _rt_color(fg: i64, bg: i64) {
        console(color_seq(fg, bg).bytes());
    }

    /// TIMER: seconds since local midnight, with the fraction, from one
    /// reading of the clock: 0 <= t < 86401 across a leap second.
    fn _rt_timer() -> f64 {
        let (secs, frac) = sys::now();
        let tm = sys::local_time(secs);
        let s = tm.tm_hour * 3600 + tm.tm_min * 60 + tm.tm_sec;
        s as f64 + frac
    }

    /// CLS: clear the screen and home the cursor, and the console's column
    /// with it, so that a later TAB does not count from the old column.
    fn _rt_cls() {
        set_console_column(0);
        console(CLS_SEQ);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RND scaled to the integer its 52 bits are, as the asm runtime printed
    /// them: `PRINT STR$(RND * 4503599627370496#)`.
    fn bits(x: f64) -> u64 {
        (x * 4503599627370496.0) as u64
    }

    #[test]
    fn rnd_sequence_matches_the_asm() {
        // The only test touching the generator, so that none race on it.
        unsafe {
            assert_eq!(bits(_rt_rnd(1.0)), 4215999074463851);
            assert_eq!(bits(_rt_rnd(1.0)), 4463028026673686);
            assert_eq!(bits(_rt_rnd(1.0)), 601748377359072);
            assert_eq!(bits(_rt_rnd(0.0)), 601748377359072);
            _rt_randomize(42.0);
            assert_eq!(bits(_rt_rnd(1.0)), 1233981780618662);
            assert_eq!(bits(_rt_rnd(1.0)), 1084824174261292);
            assert_eq!(bits(_rt_rnd(1.0)), 466904231042207);
            assert_eq!(bits(_rt_rnd(-7.0)), 39952231855205);
            assert_eq!(bits(_rt_rnd(-7.0)), 39952231855205);
            assert_eq!(bits(_rt_rnd(-0.0)), 39952231855205);
            _rt_randomize(0.0);
            assert_eq!(bits(_rt_rnd(1.0)), 1801078981577815);
            // NaN is "next", like a positive argument.
            let a = _rt_rnd(f64::NAN);
            assert!((0.0..1.0).contains(&a));
            assert_ne!(_rt_rnd(f64::NAN), a);
        }
    }

    #[test]
    fn cls_sequence() {
        assert_eq!(CLS_SEQ, b"\x1b[2J\x1b[H");
    }

    #[test]
    fn locate_sequences() {
        assert_eq!(locate_seq(5, 10).bytes(), b"\x1b[5;10H");
        assert_eq!(locate_seq(5, -1).bytes(), b"\x1b[5d");
        assert_eq!(locate_seq(-1, 10).bytes(), b"\x1b[10G");
        assert_eq!(locate_seq(-1, -1).bytes(), b"");
        assert_eq!(locate_seq(0, 0).bytes(), b"\x1b[0;0H");
        // %d of a wider value: truncated to a C int, as printf saw it.
        assert_eq!(locate_seq(1 << 32 | 3, 1).bytes(), b"\x1b[3;1H");
    }

    fn written() -> Vec<u8> {
        CONSOLE.with(|c| core::mem::take(&mut *c.borrow_mut()))
    }

    #[test]
    fn console_control_writes_and_moves_the_column() {
        // The only test touching the console's column.
        written();
        set_console_column(7);
        unsafe { _rt_locate(-1, -1) };
        assert_eq!(unsafe { _rt_pos() }, 8);
        unsafe { _rt_locate(3, -1) };
        assert_eq!(unsafe { _rt_pos() }, 8);
        unsafe { _rt_locate(-1, 20) };
        assert_eq!(unsafe { _rt_pos() }, 20);
        unsafe { _rt_locate(2, 5) };
        assert_eq!(unsafe { _rt_pos() }, 5);
        unsafe { _rt_color(-1, -1) };
        unsafe { _rt_color(1, -1) };
        unsafe { _rt_cls() };
        assert_eq!(unsafe { _rt_pos() }, 1);
        unsafe { _rt_beep() };
        assert_eq!(
            written(),
            b"\x1b[3d\x1b[20G\x1b[2;5H\x1b[25;34m\x1b[2J\x1b[H\x07"
        );
    }

    #[test]
    fn color_sequences() {
        // Foreground and background, each through the hue table.
        assert_eq!(color_seq(1, 4).bytes(), b"\x1b[25;34;41m");
        assert_eq!(color_seq(7, 0).bytes(), b"\x1b[25;37;40m");
        // Bright halves.
        assert_eq!(color_seq(9, 12).bytes(), b"\x1b[25;94;101m");
        assert_eq!(color_seq(15, 8).bytes(), b"\x1b[25;97;100m");
        // Blinking: 16-31.
        assert_eq!(color_seq(17, 2).bytes(), b"\x1b[5;34;42m");
        assert_eq!(color_seq(31, 15).bytes(), b"\x1b[5;97;107m");
        // One omitted.
        assert_eq!(color_seq(14, -1).bytes(), b"\x1b[25;93m");
        assert_eq!(color_seq(20, -1).bytes(), b"\x1b[5;31m");
        assert_eq!(color_seq(-1, 3).bytes(), b"\x1b[46m");
        assert_eq!(color_seq(-1, 11).bytes(), b"\x1b[106m");
        // Neither.
        assert_eq!(color_seq(-1, -1).bytes(), b"");
        // Out of range, as the asm computes it: the low three bits pick the
        // hue, and 8 or more is bright.
        assert_eq!(color_seq(32, 16).bytes(), b"\x1b[5;90;100m");
        // Every hue.
        for c in 0..8 {
            let want = [0, 4, 2, 6, 1, 5, 3, 7][c as usize];
            let s = color_seq(c, c);
            let want = format!("\x1b[25;{};{}m", 30 + want, 40 + want);
            assert_eq!(s.bytes(), want.as_bytes());
        }
    }

    #[test]
    fn timer_is_seconds_since_midnight() {
        let t = unsafe { _rt_timer() };
        assert!((0.0..86401.0).contains(&t), "{t}");
        let tm = sys::local_time(sys::now().0);
        let s = (tm.tm_hour * 3600 + tm.tm_min * 60 + tm.tm_sec) as f64;
        // Within a few seconds of the clock read again (allowing midnight).
        assert!((s - t).abs() < 5.0 || (s - t).abs() > 86000.0, "{s} {t}");
    }

    fn owned(s: Str) -> Vec<u8> {
        let v = unsafe { s.bytes() }.to_vec();
        unsafe { sys::free(s.ptr.cast_mut().cast()) };
        v
    }

    #[test]
    fn date_and_time_shapes() {
        let d = owned(unsafe { _rt_date() });
        assert_eq!(d.len(), 10, "{d:?}");
        for (i, &c) in d.iter().enumerate() {
            if i == 2 || i == 5 {
                assert_eq!(c, b'-');
            } else {
                assert!(c.is_ascii_digit());
            }
        }
        let t = owned(unsafe { _rt_time() });
        assert_eq!(t.len(), 8, "{t:?}");
        for (i, &c) in t.iter().enumerate() {
            if i == 2 || i == 5 {
                assert_eq!(c, b':');
            } else {
                assert!(c.is_ascii_digit());
            }
        }
        let month = (d[0] - b'0') * 10 + d[1] - b'0';
        assert!((1..=12).contains(&month));
        let hour = (t[0] - b'0') * 10 + t[1] - b'0';
        assert!(hour < 24);
    }
}
