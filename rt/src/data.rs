//! DATA, READ and RESTORE, and creating an array by using it (data.s)
//!
//! The compiler collects every DATA item into a table in the program, which
//! the program hands to `_rt_data_init` as it starts, with its length. The
//! runtime keeps the next item READ takes, from 0. An item is kept as
//! written, because
//! GW-BASIC decides what it is when it is READ: `DATA 007` is the string
//! "007" to READ A$ and the number 7 to READ A. A quoted item is a string
//! only.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::error::{self, _err_memory, _err_outofdata, _err_overflow, _err_syntax};
use crate::num::scan_number;
use crate::{Str, sys};
use core::ffi::{CStr, c_char};

/// One DATA item, as codegen's `define_data` lays it out.
#[repr(C)]
pub struct Item {
    /// 1 if the item was written in quotes, else 0.
    pub quoted: u64,
    /// The item's text, without quotes, NUL-terminated.
    pub text: *const c_char,
}

/// The program's DATA: where its table is, how long, and what READ takes
/// next. A program without DATA never sets them, and READ finds nothing.
mod program {
    use super::Item;
    use crate::{State, Word};

    pub static TABLE: State<*const Item> = State::new(core::ptr::null());
    pub static COUNT: Word = Word::new(0);
    pub static NEXT: Word = Word::new(0);

    pub fn item(i: u64) -> *const Item {
        unsafe { TABLE.get().add(i as usize) }
    }
    pub fn count() -> u64 {
        COUNT.get()
    }
    pub fn next() -> &'static Word {
        &NEXT
    }
}

/// The next item, and READ moves past it; `None` past the last.
fn take() -> Option<&'static Item> {
    let i = program::next().get();
    if i >= program::count() {
        return None;
    }
    program::next().set(i + 1);
    Some(unsafe { &*program::item(i) })
}

export! {
    /// The program's DATA: `count` items at `table`, which the program
    /// keeps for as long as it runs (READ returns views of their text).
    /// Called once, as the program starts, by a program with DATA; it
    /// passes them rather than the runtime naming them, so that the runtime
    /// needs nothing of a program to link -- or to run a program compiled
    /// into the compiler's own process.
    fn _rt_data_init(table: *const Item, count: i64) {
        unsafe { *program::TABLE.get() = table };
        program::COUNT.set(count as u64);
        program::NEXT.set(0);
    }

    /// READ into a numeric variable. The item's text must be a number and
    /// nothing else but blanks, read as VAL reads one; anything else -- a
    /// quoted item, or text that is not a number -- is GW-BASIC's Syntax
    /// error, reported for the READ on `line`. An empty item is 0. Reading
    /// past the last item is Out of DATA. Each error gives 0 when trapped.
    fn _rt_read_number(line: i64) -> f64 {
        let Some(item) = take() else {
            error::raise(&_err_outofdata[..], line);
            return 0.0;
        };
        let msg = 'bad: {
            if item.quoted != 0 {
                break 'bad &_err_syntax[..]; // a string, not a number
            }
            let text = unsafe { CStr::from_ptr(item.text) }.to_bytes();
            if text.is_empty() {
                return 0.0;
            }
            let s = scan_number(text);
            if s.overflow {
                break 'bad &_err_overflow[..];
            }
            // Only blanks may follow the number.
            let mut rest = text.iter().skip(s.consumed);
            if s.consumed == 0 || rest.any(|&c| c != b' ' && c != b'\t') {
                break 'bad &_err_syntax[..];
            }
            return s.value;
        };
        error::raise(msg, line);
        0.0
    }

    /// READ into a string variable: the item's text as written -- a quoted
    /// item without its quotes, an unquoted one as the DATA statement
    /// spelled it, so `DATA 007` reads "007". A view of the table, not a
    /// copy. Past the last item, Out of DATA, and an empty string when
    /// trapped.
    fn _rt_read_string(line: i64) -> Str {
        let Some(item) = take() else {
            error::raise(&_err_outofdata[..], line);
            return Str::EMPTY;
        };
        let text = unsafe { CStr::from_ptr(item.text) }.to_bytes();
        Str {
            ptr: text.as_ptr(),
            len: text.len(),
        }
    }

    /// RESTORE: READ next takes item `index`. RESTORE with no line passes
    /// 0, RESTORE n the index of the first item at or after line n.
    fn _rt_restore(index: i64) {
        program::next().set(index as u64);
    }

    /// Create an array by using it, as GW-BASIC does: called when an array
    /// is reached with no storage, never DIMmed or ERASEd. Every one of its
    /// `dims` dimensions runs from `base` (the OPTION BASE) to 10, and the
    /// `size`-byte elements start as zeros -- 0 and "" alike.
    ///
    /// `desc` is the descriptor: word 0 the element pointer, word 1+i
    /// dimension i's element count, word 1+dims+i its lower bound. Out of
    /// memory leaves the pointer null.
    fn _rt_array_autodim(desc: *mut u64, dims: i64, size: i64, base: i64) {
        let per = 11 - base; // elements per dimension: base..10
        let mut bytes = size;
        for i in 0..dims as usize {
            unsafe {
                *desc.add(1 + i) = per as u64;
                *desc.add(1 + dims as usize + i) = base as u64;
            }
            match bytes.checked_mul(per) {
                Some(b) => bytes = b,
                None => {
                    // Too large to address at all.
                    error::raise(&_err_memory[..], 0);
                    return;
                }
            }
        }
        let p = unsafe { sys::calloc(1, bytes as usize) }.cast::<u8>();
        if p.is_null() {
            error::raise(&_err_memory[..], 0);
            return;
        }
        unsafe { *desc = p as u64 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::num::testing::*;
    use std::sync::Mutex;

    /// The table is one, and tests run in parallel.
    static LOCK: Mutex<()> = Mutex::new(());

    /// Make `items` the DATA, as a program's start does, and READ from the
    /// first. The table lives on, as a program's does.
    fn data(items: &[(bool, &'static [u8])]) {
        let table: Vec<Item> = items
            .iter()
            .map(|&(quoted, text)| {
                assert_eq!(text.last(), Some(&0));
                Item {
                    quoted: quoted as u64,
                    text: text.as_ptr().cast(),
                }
            })
            .collect();
        let table = table.leak();
        unsafe { _rt_data_init(table.as_ptr(), table.len() as i64) };
    }

    fn read_s() -> Vec<u8> {
        unsafe { _rt_read_string(10).bytes() }.to_vec()
    }

    #[test]
    fn read_strings_and_numbers() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        data(&[
            (false, b"007\0"),
            (true, b"a, b\0"),
            (false, b"\0"),
            (false, b" 1 2 \0"),
            (false, b"&HFF\0"),
            (false, b"-1.5D2\t\0"),
        ]);
        assert_eq!(read_s(), b"007");
        assert_eq!(read_s(), b"a, b");
        assert_eq!(read_s(), b"");
        assert_eq!(read_s(), b" 1 2 ");
        unsafe {
            _rt_restore(0);
            assert_eq!(_rt_read_number(10), 7.0);
            _rt_restore(2);
            assert_eq!(_rt_read_number(10), 0.0); // an empty item is 0
            assert_eq!(_rt_read_number(10), 12.0);
            assert_eq!(_rt_read_number(10), 255.0);
            assert_eq!(_rt_read_number(10), -150.0);
            // A string READ returns a view of the table's text.
            _rt_restore(1);
            let r = _rt_read_string(10);
            assert_eq!(r.ptr, (*program::item(1)).text.cast());
        }
    }

    #[test]
    fn read_errors() {
        if in_child() {
            data(&[
                (true, b"1\0"),
                (false, b"1x\0"),
                (false, b"abc\0"),
                (false, b"1e999\0"),
                (false, b"+\0"),
            ]);
            for _ in 0..5 {
                assert_eq!(trapped(|| unsafe { _rt_read_number(10) }), (0.0, true));
            }
            assert_eq!(trapped(|| unsafe { _rt_read_number(10) }), (0.0, true));
            let (r, raised) = trapped(|| unsafe { _rt_read_string(10) });
            assert!(raised && r == Str::EMPTY);
            unsafe { _rt_read_number(30) };
            return;
        }
        expect_fatal("data::tests::read_errors", "?Out of DATA in 30\n");
    }

    #[test]
    fn read_of_a_quoted_number_is_a_syntax_error() {
        if in_child() {
            data(&[(true, b"5\0")]);
            unsafe { _rt_read_number(40) };
            return;
        }
        expect_fatal(
            "data::tests::read_of_a_quoted_number_is_a_syntax_error",
            "?Syntax error in 40\n",
        );
    }

    #[test]
    fn autodim_makes_base_to_10() {
        let mut desc = [0u64; 7];
        unsafe { _rt_array_autodim(desc.as_mut_ptr(), 3, 8, 1) };
        assert_ne!(desc[0], 0);
        assert_eq!(&desc[1..], &[10, 10, 10, 1, 1, 1]);
        let p = desc[0] as *mut u8;
        let all = unsafe { std::slice::from_raw_parts(p, 8 * 1000) };
        assert!(all.iter().all(|&b| b == 0));
        unsafe { sys::free(p.cast()) };

        let mut desc = [0u64; 3];
        unsafe { _rt_array_autodim(desc.as_mut_ptr(), 1, 16, 0) };
        assert_eq!(&desc[1..], &[11, 0]);
        unsafe { sys::free(desc[0] as *mut core::ffi::c_void) };
    }

    #[test]
    fn autodim_too_large_is_out_of_memory() {
        if in_child() {
            let mut desc = [0u64; 41];
            let ((), raised) =
                trapped(|| unsafe { _rt_array_autodim(desc.as_mut_ptr(), 20, 8, 0) });
            assert!(raised);
            assert_eq!(desc[0], 0);
            unsafe { _rt_array_autodim(desc.as_mut_ptr(), 20, 8, 0) };
            return;
        }
        expect_fatal(
            "data::tests::autodim_too_large_is_out_of_memory",
            "?Out of memory\n",
        );
    }
}
