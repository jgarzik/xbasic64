//! The runtime library every compiled BASIC program links against
//!
//! Generated code calls the helpers here (`_rt_*`, listed with their
//! signatures in the compiler's `src/rtsig.rs`) and reads and writes the
//! error-trapping words in `error` by symbol. The program in turn defines
//! the DATA table the runtime reads.
//!
//! It is `no_std` and calls the C library through the declarations in
//! `sys`, so it is one portable source for every target: the compiler's
//! `build.rs` builds it with rustc as a staticlib for the target, and embeds
//! that. Numbers are formatted with the C library's `snprintf`, not
//! `core::fmt`, which would add tens of kilobytes to every program.
//!
//! The unit tests run with std, as an ordinary crate.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

#![cfg_attr(not(test), no_std)]
// The symbol names are the ABI generated code links against.
#![allow(non_upper_case_globals)]

/// Define helpers generated code calls, by their symbol names.
///
/// On x86-64 every helper uses the System V convention, on every OS, and
/// codegen calls it so: a string comes back as a two-word struct in
/// rax:rdx, which is what Cranelift's two-value return is, whereas the
/// Windows C convention returns it through a hidden pointer. Elsewhere it is
/// the platform's C convention, which returns two words in two registers.
///
/// Each is `unsafe` to call: its contract is its signature in the
/// compiler's `src/rtsig.rs`, and the pointers it is given must be valid.
macro_rules! export {
    ($(
        $(#[$meta:meta])*
        fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block
    )*) => {$(
        $(#[$meta])*
        #[cfg(target_arch = "x86_64")]
        #[allow(clippy::missing_safety_doc)]
        #[unsafe(no_mangle)]
        pub unsafe extern "sysv64" fn $name($($arg: $ty),*) $(-> $ret)? $body

        $(#[$meta])*
        #[cfg(not(target_arch = "x86_64"))]
        #[allow(clippy::missing_safety_doc)]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name($($arg: $ty),*) $(-> $ret)? $body
    )*};
}

pub mod error;
pub mod sys;

/// A string as generated code passes it: a pointer and a length, with no
/// terminator. Returned by value it is two words in two registers.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Str {
    pub ptr: *const u8,
    pub len: usize,
}

impl Str {
    pub const EMPTY: Str = Str {
        ptr: core::ptr::null(),
        len: 0,
    };

    /// The bytes. A null pointer with length 0 is the empty string.
    ///
    /// # Safety
    /// `ptr` must point at `len` readable bytes that outlive the slice.
    pub unsafe fn bytes<'a>(self) -> &'a [u8] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
        }
    }
}

/// A word generated code reads and writes by symbol, as a plain 8-byte
/// object in writable data.
#[repr(transparent)]
pub struct Word(core::cell::UnsafeCell<u64>);

// The runtime is single-threaded, as every BASIC program is.
unsafe impl Sync for Word {}

impl Word {
    pub const fn new(v: u64) -> Word {
        Word(core::cell::UnsafeCell::new(v))
    }
    pub fn get(&self) -> u64 {
        unsafe { *self.0.get() }
    }
    pub fn set(&self, v: u64) {
        unsafe { *self.0.get() = v }
    }
}

/// Private runtime state: a value only this library touches.
#[repr(transparent)]
pub struct State<T>(core::cell::UnsafeCell<T>);

unsafe impl<T> Sync for State<T> {}

impl<T> State<T> {
    pub const fn new(v: T) -> State<T> {
        State(core::cell::UnsafeCell::new(v))
    }
    /// The value, to read or change.
    ///
    /// # Safety
    /// No other reference to it may be live: the runtime is single-threaded
    /// and no helper holds one across a call that could take another.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get(&self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }
}

/// `s` with a NUL appended, as a fixed-size array: for C strings defined as
/// statics, whose length must be part of their type.
pub const fn cstr<const N: usize>(s: &str) -> [u8; N] {
    let b = s.as_bytes();
    assert!(b.len() + 1 == N);
    let mut a = [0u8; N];
    let mut i = 0;
    while i < b.len() {
        a[i] = b[i];
        i += 1;
    }
    a
}

/// A fresh buffer of `len` bytes for a string result the caller owns and
/// frees: every string a helper returns that is not a slice of another
/// comes from here, which is what rtsig's `fresh` means. Out of memory is
/// raised, and an empty string returned, when there is none.
pub fn alloc_str(len: usize) -> *mut u8 {
    let p = unsafe { sys::malloc(len.max(1)) };
    if p.is_null() {
        error::raise(&error::_err_memory[..], 0);
    }
    p
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { sys::abort() }
}
