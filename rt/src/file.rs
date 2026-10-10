//! Files and the console as file 0: OPEN, PRINT #, INPUT #, RANDOM files, FIELD, GET/PUT, LOCK (file.s)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

// ---- STUB for math.rs: replaced by the file module's own at merge --------
//
// The console's column (what the asm keeps in `_file_col[0]`): characters
// already written on the current line, so column 1 is 0. math.rs reads it
// for POS and sets it for CLS and LOCATE.

static CONSOLE_COLUMN: crate::State<i64> = crate::State::new(0);

/// Characters written on the console's current line: 0 at its start.
pub fn console_column() -> i64 {
    unsafe { *CONSOLE_COLUMN.get() }
}

/// Set the console's column, as a count of characters before the cursor.
pub fn set_console_column(c: i64) {
    unsafe { *CONSOLE_COLUMN.get() = c }
}
// ---- end STUB --------------------------------------------------------------
