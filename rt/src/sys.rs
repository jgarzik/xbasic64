//! The C library, and the few places where operating systems differ
//!
//! Everything the runtime needs from outside is declared here, so the rest
//! of it is one source for every target. What differs -- how to reach the
//! standard streams, where errno lives, what ends a line -- is settled here
//! too, by `cfg`.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use core::ffi::{c_char, c_int, c_void};

/// The C library's `FILE`, only ever handled by pointer.
pub enum FILE {}

unsafe extern "C" {
    pub fn malloc(size: usize) -> *mut u8;
    pub fn calloc(n: usize, size: usize) -> *mut u8;
    pub fn realloc(p: *mut u8, size: usize) -> *mut u8;
    pub fn free(p: *mut u8);
    pub fn abort() -> !;
    pub fn exit(code: c_int) -> !;

    pub fn fflush(f: *mut FILE) -> c_int;
    pub fn fwrite(p: *const c_void, size: usize, n: usize, f: *mut FILE) -> usize;
    pub fn fprintf(f: *mut FILE, fmt: *const c_char, ...) -> c_int;
    pub fn snprintf(buf: *mut c_char, n: usize, fmt: *const c_char, ...) -> c_int;
    pub fn strtod(s: *const c_char, end: *mut *mut c_char) -> f64;
}

/// What ends a line of output: Windows programs have always written CRLF,
/// and the standard streams there are in binary mode so that nothing is
/// translated behind the runtime's back.
#[cfg(windows)]
pub const NEWLINE: &[u8] = b"\r\n";
#[cfg(not(windows))]
pub const NEWLINE: &[u8] = b"\n";

#[cfg(all(unix, not(target_vendor = "apple")))]
mod streams {
    use super::FILE;
    unsafe extern "C" {
        static stdout: *mut FILE;
        static stderr: *mut FILE;
    }
    pub fn out() -> *mut FILE {
        unsafe { stdout }
    }
    pub fn err() -> *mut FILE {
        unsafe { stderr }
    }
}

#[cfg(target_vendor = "apple")]
mod streams {
    use super::FILE;
    unsafe extern "C" {
        static __stdoutp: *mut FILE;
        static __stderrp: *mut FILE;
    }
    pub fn out() -> *mut FILE {
        unsafe { __stdoutp }
    }
    pub fn err() -> *mut FILE {
        unsafe { __stderrp }
    }
}

#[cfg(windows)]
mod streams {
    use super::FILE;
    unsafe extern "C" {
        fn __acrt_iob_func(index: u32) -> *mut FILE;
    }
    pub fn out() -> *mut FILE {
        unsafe { __acrt_iob_func(1) }
    }
    pub fn err() -> *mut FILE {
        unsafe { __acrt_iob_func(2) }
    }
}

pub use streams::{err as stderr, out as stdout};

/// Write `bytes` to `f`, all of them: a string's NUL bytes included.
///
/// # Safety
/// `f` must be an open stream.
pub unsafe fn write(f: *mut FILE, bytes: &[u8]) {
    if !bytes.is_empty() {
        unsafe { fwrite(bytes.as_ptr().cast(), 1, bytes.len(), f) };
    }
}

/// Flush every output stream, before the process writes to stderr or ends,
/// so that output already written is neither lost nor out of order.
pub fn flush_all() {
    unsafe { fflush(core::ptr::null_mut()) };
}

// ---- math: the clock and local time (TIMER, DATE$, TIME$) ----------------
//
// One reading of the clock gives whole seconds and the fraction, so the two
// cannot straddle a second: `gettimeofday` on unix, `_ftime64` on Windows
// (UCRT's `time` and `localtime_s` are inline wrappers, not exports, so the
// `_64` functions they wrap are named here). Local time goes through the C
// library on both, so TIMER, DATE$ and TIME$ all honour TZ.

/// The C library's `struct tm`. Only the leading ints are read here; glibc
/// and Darwin add `tm_gmtoff` and `tm_zone` after them, which `strftime`
/// may read, so they are part of the layout.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Tm {
    pub tm_sec: c_int,
    pub tm_min: c_int,
    pub tm_hour: c_int,
    pub tm_mday: c_int,
    pub tm_mon: c_int,
    pub tm_year: c_int,
    pub tm_wday: c_int,
    pub tm_yday: c_int,
    pub tm_isdst: c_int,
    #[cfg(unix)]
    pub tm_gmtoff: core::ffi::c_long,
    #[cfg(unix)]
    pub tm_zone: *const c_char,
}

unsafe extern "C" {
    pub fn strftime(buf: *mut c_char, max: usize, fmt: *const c_char, tm: *const Tm) -> usize;
}

#[cfg(unix)]
mod clock {
    use super::Tm;
    use core::ffi::{c_int, c_void};

    #[repr(C)]
    struct Timeval {
        tv_sec: i64,
        // Darwin's suseconds_t is 32 bits, followed by padding.
        #[cfg(target_vendor = "apple")]
        tv_usec: i32,
        #[cfg(not(target_vendor = "apple"))]
        tv_usec: i64,
    }

    unsafe extern "C" {
        fn gettimeofday(tv: *mut Timeval, tz: *mut c_void) -> c_int;
        fn localtime_r(t: *const i64, tm: *mut Tm) -> *mut Tm;
    }

    pub fn now() -> (i64, f64) {
        let mut tv = Timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        unsafe { gettimeofday(&mut tv, core::ptr::null_mut()) };
        (tv.tv_sec, tv.tv_usec as f64 / 1000000.0)
    }

    pub fn local(t: i64, tm: &mut Tm) {
        unsafe { localtime_r(&t, tm) };
    }
}

#[cfg(windows)]
mod clock {
    use super::Tm;
    use core::ffi::c_int;

    #[repr(C)]
    struct Timeb64 {
        time: i64,
        millitm: u16,
        timezone: i16,
        dstflag: i16,
    }

    unsafe extern "C" {
        fn _ftime64(tb: *mut Timeb64);
        fn _localtime64_s(tm: *mut Tm, t: *const i64) -> c_int;
    }

    pub fn now() -> (i64, f64) {
        let mut tb = Timeb64 {
            time: 0,
            millitm: 0,
            timezone: 0,
            dstflag: 0,
        };
        unsafe { _ftime64(&mut tb) };
        (tb.time, tb.millitm as f64 / 1000.0)
    }

    pub fn local(t: i64, tm: &mut Tm) {
        unsafe { _localtime64_s(tm, &t) };
    }
}

/// The time now: seconds since the epoch, and the fraction of the current
/// second (microseconds on unix, milliseconds on Windows).
pub fn now() -> (i64, f64) {
    clock::now()
}

/// `t`, seconds since the epoch, broken down in the local zone.
pub fn local_time(t: i64) -> Tm {
    let mut tm = Tm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 0,
        tm_mon: 0,
        tm_year: 0,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
        #[cfg(unix)]
        tm_gmtoff: 0,
        #[cfg(unix)]
        tm_zone: core::ptr::null(),
    };
    clock::local(t, &mut tm);
    tm
}
// ---- end math -------------------------------------------------------------
