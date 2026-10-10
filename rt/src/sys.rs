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
