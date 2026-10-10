//! Console setup at program start (print.s, _rt_platform_init)
//!
//! The printing itself is in file.rs: the console is file 0, the C
//! library's stdout, so PRINT and PRINT # are the same helpers with a
//! different number.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

export! {
    /// Prepare the console, before the program's first statement.
    ///
    /// Nothing to do on unix: stdout is open, and file 0 reaches it.
    ///
    /// On Windows the standard streams go into binary mode, since the
    /// runtime writes CRLF itself and a text-mode stream would make each a
    /// CR CR LF; stdin too, so that INPUT sees the bytes typed or piped, as
    /// it did reading the handle directly. And VT processing is turned on,
    /// so that the escape sequences CLS, LOCATE and COLOR write are acted on
    /// rather than printed literally. That is best effort: a failure means
    /// stdout is not a console -- redirected to a file, say -- and the
    /// escapes are then just bytes in the file, which is what any terminal
    /// program does.
    fn _rt_platform_init() {
        #[cfg(windows)]
        {
            use crate::sys;
            unsafe {
                sys::binary_mode(0);
                sys::binary_mode(1);
                sys::binary_mode(2);
                let h = sys::GetStdHandle(sys::STD_OUTPUT_HANDLE);
                let mut mode = 0u32;
                if sys::GetConsoleMode(h, &mut mode) != 0 {
                    sys::SetConsoleMode(h, mode | sys::ENABLE_VIRTUAL_TERMINAL_PROCESSING);
                }
            }
        }
    }
}
