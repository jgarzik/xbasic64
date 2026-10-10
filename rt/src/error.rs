//! Runtime errors: raising, trapping state, and the fatal report
//!
//! One path for every runtime check. A check passes a message -- one of the
//! statics below, NUL-terminated -- and the BASIC line, and the message
//! carries GW-BASIC's error number through `ERR_CODES`.
//!
//! When ON ERROR has armed a handler and none is running, a raised error is
//! trapped: `_err_pending` is set and the raising helper returns normally,
//! through its own epilogue; generated code tests `_err_pending` after every
//! call that can fail. Otherwise the error is reported on stderr as
//! `?message in line` and the program exits with status 1.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::sys;
use crate::{Word, cstr};
use core::ffi::c_char;

macro_rules! messages {
    ($($name:ident = $text:literal;)*) => {$(
        #[unsafe(no_mangle)]
        pub static $name: [u8; $text.len() + 1] = cstr($text);
    )*};
}

// Named by generated code, which passes them to `_rt_error`.
messages! {
    _err_syntax = "Syntax error";
    _err_subscript = "Subscript out of range";
    _err_typemismatch = "Type mismatch";
    _err_div0 = "Division by zero";
    _err_domain = "Illegal function call";
    _err_overflow = "Overflow";
    _err_dupdef = "Duplicate definition";
    _err_memory = "Out of memory";
    _err_gosub = "GOSUB stack overflow";
    _err_noreturn = "RETURN without GOSUB";
    _err_outofdata = "Out of DATA";
    _err_badfile = "Bad file number";
    _err_badmode = "Bad file mode";
    _err_fieldovf = "FIELD overflow";
    _err_permission = "Permission denied";
    _err_notfound = "File not found";
    _err_alreadyopen = "File already open";
    _err_pastend = "Input past end of file";
    _err_badname = "Bad file name";
    _err_access = "Path/File access error";
    _err_pathnotfound = "Path not found";
    _err_unprintable = "Unprintable error";
    // RESUME's own failure. Never trapped: a handler that trapped it would be
    // re-entered by its own RESUME, forever.
    _err_resnoerr = "RESUME without error";
}

/// GW-BASIC's error numbers, so that a listing's `IF ERR = 53` means what it
/// meant in 1983. Searched by number too, for the ERROR statement -- so
/// where two messages share a number the first listed is the one ERROR n
/// raises.
static ERR_CODES: [(&[u8], u64); 21] = [
    (&_err_syntax, 2),
    (&_err_noreturn, 3),
    (&_err_outofdata, 4),
    (&_err_domain, 5),
    (&_err_overflow, 6),
    (&_err_memory, 7),
    (&_err_gosub, 7),
    (&_err_subscript, 9),
    (&_err_dupdef, 10),
    (&_err_div0, 11),
    (&_err_typemismatch, 13),
    (&_err_fieldovf, 50),
    (&_err_badfile, 52),
    (&_err_notfound, 53),
    (&_err_badmode, 54),
    (&_err_alreadyopen, 55),
    (&_err_pastend, 62),
    (&_err_badname, 64),
    (&_err_permission, 70),
    (&_err_access, 75),
    (&_err_pathnotfound, 76),
];

// Error-trapping state, read and written by generated code. All zero means
// "not trapping".

/// Which ON ERROR target, numbered from 1; 0 = trapping off.
#[unsafe(no_mangle)]
pub static _err_handler: Word = Word::new(0);
/// 1 from a trapped error until its handler runs; 2 while procedures unwind
/// toward a RESUME label; 3 while they unwind to a GOSUB in main.
#[unsafe(no_mangle)]
pub static _err_pending: Word = Word::new(0);
/// Nonzero while a handler runs.
#[unsafe(no_mangle)]
pub static _err_active: Word = Word::new(0);
/// The line now running: codegen stores it per statement when trapping,
/// else before calls that can fail.
#[unsafe(no_mangle)]
pub static _err_line: Word = Word::new(0);

/// What ERR returns.
static ERR_CODE: Word = Word::new(0);
/// What ERL returns: `_err_line` as it was when the error was trapped. The
/// handler's own statements overwrite `_err_line` the moment it runs.
static ERR_ERL: Word = Word::new(0);

/// Raise the error `msg` on `line` (0: the line in `_err_line`). Returns only
/// when a handler traps it.
pub fn raise(msg: &'static [u8], line: i64) {
    raise_with_code(msg.as_ptr(), code_of(msg.as_ptr()), line);
}

/// The number of the `_err_*` message at `msg`, found by its address; 0
/// for any other.
fn code_of(msg: *const u8) -> u64 {
    ERR_CODES
        .iter()
        .find(|(m, _)| core::ptr::eq(m.as_ptr(), msg))
        .map_or(0, |&(_, n)| n)
}

/// The common path, with the number ERR is to report.
fn raise_with_code(msg: *const u8, code: u64, line: i64) {
    // An error already on its way to the handler wins: its helper may carry
    // on to the end of what it was doing, and each step fails the same way.
    if _err_pending.get() != 0 {
        return;
    }
    // Trapped only when ON ERROR armed a handler and none is running:
    // GW-BASIC does not trap an error raised inside a handler, which is also
    // what stops handler -> error -> handler from looping forever.
    if _err_handler.get() == 0 || _err_active.get() != 0 {
        unsafe { fatal(msg, line) }
    }
    ERR_CODE.set(code);
    ERR_ERL.set(_err_line.get());
    _err_active.set(1);
    _err_pending.set(1);
}

/// Report `msg` on stderr and exit with status 1.
///
/// # Safety
/// `msg` must be NUL-terminated.
unsafe fn fatal(msg: *const u8, line: i64) -> ! {
    // Most helpers pass no line; generated code stored the one running.
    let line = if line != 0 {
        line
    } else {
        _err_line.get() as i64
    };
    // In the unit tests, an error nothing expected would exit the whole test
    // binary without naming the test; a panic names it. A test expecting
    // the exit runs in a child process (`num::testing::expect_fatal`).
    #[cfg(test)]
    if !crate::num::testing::in_child() {
        let text = unsafe { core::ffi::CStr::from_ptr(msg.cast()) };
        panic!("unexpected fatal error: {text:?} in {line}");
    }
    sys::flush_all();
    let msg = msg.cast::<c_char>();
    unsafe {
        if line != 0 {
            sys::fprintf(sys::stderr(), FMT_LINE.as_ptr().cast(), msg, line);
        } else {
            sys::fprintf(sys::stderr(), FMT_BARE.as_ptr().cast(), msg);
        }
        sys::exit(1)
    }
}

#[cfg(windows)]
const FMT_LINE: &[u8] = b"?%s in %lld\r\n\0";
#[cfg(windows)]
const FMT_BARE: &[u8] = b"?%s\r\n\0";
#[cfg(windows)]
const FMT_STOP_LINE: &[u8] = b"Break in %lld\r\n\0";
#[cfg(windows)]
const FMT_STOP_BARE: &[u8] = b"Break\r\n\0";
#[cfg(not(windows))]
const FMT_LINE: &[u8] = b"?%s in %lld\n\0";
#[cfg(not(windows))]
const FMT_BARE: &[u8] = b"?%s\n\0";
#[cfg(not(windows))]
const FMT_STOP_LINE: &[u8] = b"Break in %lld\n\0";
#[cfg(not(windows))]
const FMT_STOP_BARE: &[u8] = b"Break\n\0";

export! {
    /// ERR: the number of the error that was trapped, 0 if none.
    fn _rt_err() -> i32 {
        ERR_CODE.get() as i32
    }

    /// ERL: the line the trapped error happened on, 0 if none.
    fn _rt_erl() -> i32 {
        ERR_ERL.get() as i32
    }

    /// The ERROR statement: raise GW-BASIC error number `n`. ERR is `n`
    /// itself, whatever message it maps to, so `ERROR 200 ... IF ERR = 200`
    /// works for an error of the program's own.
    fn _rt_error_num(n: i64, line: i64) {
        let msg = ERR_CODES
            .iter()
            .find(|&&(_, c)| c as i64 == n)
            .map_or(&_err_unprintable[..], |&(m, _)| m);
        raise_with_code(msg.as_ptr(), n as u64, line);
    }

    /// Raise the error `msg` (one of the `_err_*` messages) on `line`, 0
    /// for the line in `_err_line`. Returns only when a handler traps it.
    fn _rt_error(msg: *const u8, line: i64) {
        raise_with_code(msg, code_of(msg), line);
    }

    /// Report `msg` and exit, without consulting the handler: for the errors
    /// that must never be trapped.
    fn _rt_fatal(msg: *const u8, line: i64) -> ! {
        unsafe { fatal(msg, line) }
    }

    /// STOP: say where, and end the program normally, as END does.
    /// GW-BASIC prints "Break in 20" and returns to its prompt; a compiled
    /// program has no prompt.
    fn _rt_stop(line: i64) -> ! {
        sys::flush_all();
        unsafe {
            if line != 0 {
                sys::fprintf(sys::stderr(), FMT_STOP_LINE.as_ptr().cast(), line);
            } else {
                sys::fprintf(sys::stderr(), FMT_STOP_BARE.as_ptr().cast());
            }
            _rt_end()
        }
    }

    /// END: terminate the program from any frame, flushing every stream.
    fn _rt_end() -> ! {
        unsafe { sys::exit(0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_c_strings() {
        assert_eq!(&_err_div0[..], b"Division by zero\0");
        assert_eq!(_err_memory.last(), Some(&0));
    }

    #[test]
    fn a_message_carries_its_number() {
        let n = |m: &[u8]| {
            ERR_CODES
                .iter()
                .find(|(x, _)| x.as_ptr() == m.as_ptr())
                .unwrap()
                .1
        };
        assert_eq!(n(&_err_subscript), 9);
        assert_eq!(n(&_err_notfound), 53);
        // ERROR 7 raises the first message listed with 7.
        let first7 = ERR_CODES.iter().find(|&&(_, c)| c == 7).unwrap().0;
        assert_eq!(first7.as_ptr(), _err_memory.as_ptr());
    }

    #[test]
    fn a_trapped_error_sets_err_and_erl_once() {
        _err_handler.set(1);
        _err_line.set(30);
        raise(&_err_div0, 0);
        assert_eq!(_err_pending.get(), 1);
        assert_eq!(unsafe { _rt_err() }, 11);
        assert_eq!(unsafe { _rt_erl() }, 30);
        // The first error wins while one is pending.
        raise(&_err_overflow, 0);
        assert_eq!(unsafe { _rt_err() }, 11);
        unsafe { _rt_error_num(200, 0) };
        assert_eq!(unsafe { _rt_err() }, 11);
        _err_pending.set(0);
        _err_active.set(0);
        unsafe { _rt_error_num(200, 0) };
        assert_eq!(unsafe { _rt_err() }, 200);
        _err_pending.set(0);
        _err_active.set(0);
        _err_handler.set(0);
        _err_line.set(0);
    }
}
