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

// UCRT's printf family are inline functions in its headers; their exported
// definitions are in legacy_stdio_definitions.lib, which programs link and
// so must the runtime's unit tests.
#[cfg_attr(windows, link(name = "legacy_stdio_definitions"))]
unsafe extern "C" {
    pub fn malloc(size: usize) -> *mut c_void;
    pub fn calloc(n: usize, size: usize) -> *mut c_void;
    pub fn realloc(p: *mut c_void, size: usize) -> *mut c_void;
    pub fn free(p: *mut c_void);
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

// ---------------------------------------------------------------------------
// Files, console input and console setup (file.rs, input.rs, print.rs)
//
// Every file is a C stdio stream on every OS, opened in binary mode: the
// runtime writes CRLF itself where Windows wants it. What differs -- the
// standard input stream, errno, 64-bit seeking, record locking, telling a
// directory from a file, and why an open failed -- is settled below.
// ---------------------------------------------------------------------------

unsafe extern "C" {
    pub fn fclose(f: *mut FILE) -> c_int;
    pub fn fread(p: *mut c_void, size: usize, n: usize, f: *mut FILE) -> usize;
    pub fn fgetc(f: *mut FILE) -> c_int;
    pub fn ungetc(c: c_int, f: *mut FILE) -> c_int;
}

pub const SEEK_SET: c_int = 0;
pub const SEEK_END: c_int = 2;

#[cfg(all(unix, not(target_vendor = "apple")))]
mod stdin_stream {
    use super::FILE;
    unsafe extern "C" {
        static stdin: *mut FILE;
    }
    pub fn get() -> *mut FILE {
        unsafe { stdin }
    }
}

#[cfg(target_vendor = "apple")]
mod stdin_stream {
    use super::FILE;
    unsafe extern "C" {
        static __stdinp: *mut FILE;
    }
    pub fn get() -> *mut FILE {
        unsafe { __stdinp }
    }
}

#[cfg(windows)]
mod stdin_stream {
    use super::FILE;
    unsafe extern "C" {
        fn __acrt_iob_func(index: u32) -> *mut FILE;
    }
    pub fn get() -> *mut FILE {
        unsafe { __acrt_iob_func(0) }
    }
}

pub use stdin_stream::get as stdin;

/// Why an OPEN failed, as far as the runtime tells causes apart: each is
/// one of GW-BASIC's error numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenFailure {
    /// The file does not exist (53, or 76 when it was to be created).
    NotFound,
    /// A directory on the way to it does not exist (76).
    PathNotFound,
    /// It exists, and may not be opened so (70).
    Denied,
    /// The name cannot name a file (64).
    BadName,
    /// Anything else, a directory included (75).
    Other,
}

#[cfg(unix)]
mod unix {
    use super::{FILE, OpenFailure};
    use core::ffi::{c_char, c_int};

    unsafe extern "C" {
        fn fopen(name: *const c_char, mode: *const c_char) -> *mut FILE;
        fn fileno(f: *mut FILE) -> c_int;
        fn fseeko(f: *mut FILE, off: i64, whence: c_int) -> c_int;
        fn ftello(f: *mut FILE) -> i64;
        fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    }

    #[cfg(not(target_vendor = "apple"))]
    unsafe extern "C" {
        fn __errno_location() -> *mut c_int;
        fn syscall(n: core::ffi::c_long, ...) -> core::ffi::c_long;
    }
    #[cfg(target_vendor = "apple")]
    unsafe extern "C" {
        fn __error() -> *mut c_int;
        fn fstat(fd: c_int, buf: *mut core::ffi::c_void) -> c_int;
    }

    pub fn errno() -> c_int {
        #[cfg(not(target_vendor = "apple"))]
        let p = unsafe { __errno_location() };
        #[cfg(target_vendor = "apple")]
        let p = unsafe { __error() };
        unsafe { *p }
    }

    /// fseek with a 64-bit offset.
    ///
    /// # Safety
    /// `f` must be an open stream.
    pub unsafe fn seek(f: *mut FILE, off: i64, whence: c_int) -> c_int {
        unsafe { fseeko(f, off, whence) }
    }

    /// ftell with a 64-bit result.
    ///
    /// # Safety
    /// `f` must be an open stream.
    pub unsafe fn tell(f: *mut FILE) -> i64 {
        unsafe { ftello(f) }
    }

    /// fopen.
    ///
    /// # Safety
    /// Both must be NUL-terminated.
    pub unsafe fn open(name: *const c_char, mode: *const c_char) -> *mut FILE {
        unsafe { fopen(name, mode) }
    }

    // errno values. Linux's are the same on every architecture it runs
    // these programs on; macOS differs only in ENAMETOOLONG.
    const EPERM: c_int = 1;
    const ENOENT: c_int = 2;
    const EACCES: c_int = 13;
    const ENOTDIR: c_int = 20;
    const EINVAL: c_int = 22;
    const ETXTBSY: c_int = 26;
    const EROFS: c_int = 30;
    #[cfg(not(target_vendor = "apple"))]
    const ENAMETOOLONG: c_int = 36;
    #[cfg(target_vendor = "apple")]
    const ENAMETOOLONG: c_int = 63;

    /// Why the open of `_name` just made failed, from errno.
    ///
    /// # Safety
    /// `_name` must be NUL-terminated, as for `windows::open_failure`.
    pub unsafe fn open_failure(_name: *const c_char) -> OpenFailure {
        match errno() {
            ENOENT => OpenFailure::NotFound,
            ENOTDIR => OpenFailure::PathNotFound,
            EACCES | EPERM | EROFS | ETXTBSY => OpenFailure::Denied,
            ENAMETOOLONG | EINVAL => OpenFailure::BadName,
            _ => OpenFailure::Other,
        }
    }

    /// Is the open stream `f` a directory? One opens for reading on unix,
    /// and only the first read fails, with EISDIR.
    ///
    /// fstat, but not glibc's `fstat` or `stat`: glibc exports those
    /// symbols only from 2.33 (before, they were inline wrappers around
    /// `__fxstat`), so a program linked against either would not start on
    /// an older system. The system call has always been there, so on Linux
    /// it is made directly, as the assembly runtime made it, and `st_mode`
    /// read from the kernel's `struct stat` for the architecture. macOS's
    /// libSystem has always exported `fstat`. When fstat fails nothing is
    /// known, and the file is taken not to be a directory.
    ///
    /// # Safety
    /// `f` must be an open stream.
    pub unsafe fn is_directory(f: *mut FILE) -> bool {
        const S_IFMT: u32 = 0o170000;
        const S_IFDIR: u32 = 0o040000;
        // Larger than any architecture's struct stat, and 8-byte aligned.
        let mut buf = [0u64; 32];
        let fd = unsafe { fileno(f) };
        // SYS_fstat, and st_mode, a u32 at offset 24.
        #[cfg(all(not(target_vendor = "apple"), target_arch = "x86_64"))]
        let (ok, mode) = (
            unsafe { syscall(5, fd, buf.as_mut_ptr()) } == 0,
            buf[3] as u32,
        );
        // SYS_fstat, and st_mode, a u32 at offset 16.
        #[cfg(all(not(target_vendor = "apple"), target_arch = "aarch64"))]
        let (ok, mode) = (
            unsafe { syscall(80, fd, buf.as_mut_ptr()) } == 0,
            buf[2] as u32,
        );
        // st_mode is a u16 at offset 4, after a 32-bit st_dev.
        #[cfg(target_vendor = "apple")]
        let (ok, mode) = (
            unsafe { fstat(fd, buf.as_mut_ptr().cast()) } == 0,
            (buf[0] >> 32) as u16 as u32,
        );
        ok && mode & S_IFMT == S_IFDIR
    }

    /// `struct flock`, whose layout is the OS's.
    #[cfg(not(target_vendor = "apple"))]
    #[repr(C)]
    struct Flock {
        l_type: i16,
        l_whence: i16,
        l_start: i64,
        l_len: i64,
        l_pid: i32,
    }
    #[cfg(target_vendor = "apple")]
    #[repr(C)]
    struct Flock {
        l_start: i64,
        l_len: i64,
        l_pid: i32,
        l_type: i16,
        l_whence: i16,
    }
    #[cfg(not(target_vendor = "apple"))]
    const F_SETLK: c_int = 6;
    #[cfg(not(target_vendor = "apple"))]
    const F_WRLCK: i16 = 1;
    #[cfg(target_vendor = "apple")]
    const F_SETLK: c_int = 8;
    #[cfg(target_vendor = "apple")]
    const F_WRLCK: i16 = 3;
    const F_UNLCK: i16 = 2;

    /// LOCK (`lock`) or UNLOCK `len` bytes of `f` from `start`, a `len` of
    /// 0 being the whole file. False when refused.
    ///
    /// POSIX advisory locks through fcntl(F_SETLK). "Advisory" is the
    /// honest word: they hold against other processes that also lock,
    /// which is exactly what GW-BASIC's LOCK was for, and do nothing
    /// against a process that simply writes.
    ///
    /// # Safety
    /// `f` must be an open stream.
    pub unsafe fn lock(f: *mut FILE, start: i64, len: i64, lock: bool) -> bool {
        let fl = Flock {
            l_type: if lock { F_WRLCK } else { F_UNLCK },
            l_whence: 0, // SEEK_SET
            l_start: start,
            l_len: len,
            l_pid: 0,
        };
        unsafe { fcntl(fileno(f), F_SETLK, &fl as *const Flock) >= 0 }
    }
}

#[cfg(windows)]
mod windows {
    use super::{FILE, OpenFailure};
    use core::ffi::{c_char, c_int};

    unsafe extern "C" {
        fn _fsopen(name: *const c_char, mode: *const c_char, share: c_int) -> *mut FILE;
        fn _errno() -> *mut c_int;
        fn __doserrno() -> *mut u32;
        fn _fileno(f: *mut FILE) -> c_int;
        fn _get_osfhandle(fd: c_int) -> isize;
        fn _setmode(fd: c_int, mode: c_int) -> c_int;
        fn _fseeki64(f: *mut FILE, off: i64, whence: c_int) -> c_int;
        fn _ftelli64(f: *mut FILE) -> i64;
    }

    /// OVERLAPPED, which carries LockFileEx's byte offset.
    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: isize,
    }

    unsafe extern "system" {
        fn GetFileAttributesA(name: *const c_char) -> u32;
        fn LockFileEx(
            h: isize,
            flags: u32,
            reserved: u32,
            lo: u32,
            hi: u32,
            ov: *mut Overlapped,
        ) -> i32;
        fn UnlockFileEx(h: isize, reserved: u32, lo: u32, hi: u32, ov: *mut Overlapped) -> i32;
        pub fn GetStdHandle(which: u32) -> isize;
        pub fn GetConsoleMode(h: isize, mode: *mut u32) -> i32;
        pub fn SetConsoleMode(h: isize, mode: u32) -> i32;
    }

    pub const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    pub const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 4;

    pub fn errno() -> c_int {
        unsafe { *_errno() }
    }

    /// fseek with a 64-bit offset.
    ///
    /// # Safety
    /// `f` must be an open stream.
    pub unsafe fn seek(f: *mut FILE, off: i64, whence: c_int) -> c_int {
        unsafe { _fseeki64(f, off, whence) }
    }

    /// ftell with a 64-bit result.
    ///
    /// # Safety
    /// `f` must be an open stream.
    pub unsafe fn tell(f: *mut FILE) -> i64 {
        unsafe { _ftelli64(f) }
    }

    /// fopen, sharing the file with readers only (`_SH_DENYWR`), as the
    /// assembly runtime's CreateFileA did with FILE_SHARE_READ: a second
    /// writer is refused, which is "Permission denied".
    ///
    /// # Safety
    /// Both must be NUL-terminated.
    pub unsafe fn open(name: *const c_char, mode: *const c_char) -> *mut FILE {
        const _SH_DENYWR: c_int = 0x20;
        unsafe { _fsopen(name, mode, _SH_DENYWR) }
    }

    /// Why the open just made failed. errno alone cannot say: the CRT maps
    /// a missing directory and a bad name to ENOENT, as it does a missing
    /// file. So this reads `_doserrno`, the Win32 error the CRT set errno
    /// from, and maps it as the assembly runtime mapped GetLastError.
    ///
    /// # Safety
    /// `name` must be NUL-terminated: a directory is told apart by name.
    pub unsafe fn open_failure(name: *const c_char) -> OpenFailure {
        match unsafe { *__doserrno() } {
            2 => OpenFailure::NotFound,     // ERROR_FILE_NOT_FOUND
            3 => OpenFailure::PathNotFound, // ERROR_PATH_NOT_FOUND
            5 => {
                // ERROR_ACCESS_DENIED is also what opening a directory
                // gives, and a directory is not a file to be denied: it is
                // "Path/File access error", as on unix.
                let attrs = unsafe { GetFileAttributesA(name) };
                if attrs != u32::MAX && attrs & 0x10 != 0 {
                    OpenFailure::Other // FILE_ATTRIBUTE_DIRECTORY
                } else {
                    OpenFailure::Denied
                }
            }
            32 | 19 => OpenFailure::Denied, // sharing violation, write protect
            123 | 161 | 206 => OpenFailure::BadName, // invalid, bad path, too long
            _ => OpenFailure::Other,
        }
    }

    /// Windows refuses to open a directory at all, so an open stream never
    /// is one.
    ///
    /// # Safety
    /// None needed; `unsafe` to match unix.
    pub unsafe fn is_directory(_f: *mut FILE) -> bool {
        false
    }

    /// LOCK or UNLOCK through LockFileEx/UnlockFileEx on the stream's
    /// handle. Win32 needs a length, so a `len` of 0, the whole file, is
    /// the largest range there is. LOCKFILE_FAIL_IMMEDIATELY makes a held
    /// lock an error rather than a wait, which is what GW-BASIC programs
    /// expect.
    ///
    /// # Safety
    /// `f` must be an open stream.
    pub unsafe fn lock(f: *mut FILE, start: i64, len: i64, lock: bool) -> bool {
        let len = if len == 0 { i64::MAX } else { len } as u64;
        let mut ov = Overlapped {
            internal: 0,
            internal_high: 0,
            offset: start as u32,
            offset_high: (start as u64 >> 32) as u32,
            event: 0,
        };
        let h = unsafe { _get_osfhandle(_fileno(f)) };
        let (lo, hi) = (len as u32, (len >> 32) as u32);
        let ok = if lock {
            // LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY
            unsafe { LockFileEx(h, 3, 0, lo, hi, &mut ov) }
        } else {
            unsafe { UnlockFileEx(h, 0, lo, hi, &mut ov) }
        };
        ok != 0
    }

    /// Put file descriptor `fd` in binary mode, so nothing is translated.
    ///
    /// # Safety
    /// None beyond the CRT's own checks.
    pub unsafe fn binary_mode(fd: c_int) {
        const _O_BINARY: c_int = 0x8000;
        unsafe { _setmode(fd, _O_BINARY) };
    }
}

#[cfg(unix)]
pub use unix::{errno, is_directory, lock, open, open_failure, seek, tell};
#[cfg(windows)]
pub use windows::{
    ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE,
    SetConsoleMode, binary_mode,
};
#[cfg(windows)]
pub use windows::{errno, is_directory, lock, open, open_failure, seek, tell};
