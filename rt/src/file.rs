//! Files and the console as file 0: OPEN, PRINT #, INPUT #, RANDOM files, FIELD, GET/PUT, LOCK (file.s)
//!
//! BASIC refers to a file by number, 1 to 15; the console is number 0, so
//! PRINT and PRINT # are the same helpers with a different number, and
//! neither can drift from the other. Every file is a C stdio stream, opened
//! in binary mode, on every OS; the console is the C library's stdout, the
//! same `FILE*` math.rs writes its escapes to, so all console output stays
//! in order.
//!
//! A failed OPEN raises GW-BASIC's error for the cause (53, 64, 70, 75, 76).
//! How each number was opened is recorded, so PRINT # to a file opened FOR
//! INPUT, or INPUT # from one opened for writing, is "Bad file mode" (54)
//! rather than a write the C library quietly refuses.
//!
//! The helpers that report a bad file number, a bad mode or the end of a
//! file are not given the line, so they pass 0, and the error reports the
//! one generated code stored in `_err_line` before the call. When a handler
//! traps an error, the helper returns at once with a harmless result.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::error::{self, raise};
use crate::sys::{self, FILE};
use crate::{State, Str, num};
use core::ffi::c_char;
use core::ptr;

/// File numbers 0 (the console) to 15.
const FILES: usize = 16;

// How a number was opened.
const INPUT: i64 = 0;
const OUTPUT: i64 = 1;
const APPEND: i64 = 2;
const RANDOM: i64 = 3;

/// PRINT lays a line out in zones this wide; a comma moves to the next one.
const PRINT_ZONE: i64 = 14;

/// Longest INPUT # field kept.
const MAX_FIELD_LEN: usize = 1023;

/// Longest line LINE INPUT # returns at once: what fgets(buf, 1023) took.
const MAX_LINE_LEN: usize = 1022;

/// Largest record GW-BASIC allows.
const MAX_RECLEN: i64 = 32767;

/// Longest file name passed to the C library, its NUL included. Longer
/// names are "Bad file name", which is what Linux says of them (PATH_MAX).
const NAME_CAP: usize = 4096;

/// Everything known about each file number.
///
/// A file opened FOR RANDOM carries three things beyond its stream: a record
/// length, a record buffer of that length, and a cursor saying how much of
/// the buffer FIELD has handed out so far.
///
/// FIELD does not copy anything. It hands each variable the (pointer,
/// length) pair naming its slice of that buffer, and a BASIC string is
/// exactly such a pair, so the variable *is* a window onto the buffer. GET
/// overwrites the buffer and every field variable sees the new record at
/// once; LSET and RSET write through the window, so PUT emits what they
/// wrote. This is why LSET exists at all: plain assignment would allocate a
/// fresh string and the variable would quietly stop tracking the record.
///
/// A window outlives the OPEN that made it: nothing stops a program using a
/// FIELD variable after CLOSE, or after the number is opened again. So a
/// number's record buffer, once allocated, is never freed nor moved. It is
/// MAX_RECLEN bytes, enough for any record, and every later OPEN FOR RANDOM
/// of the number reuses it; an old window then shows the new file's record
/// (GW-BASIC calls such a variable undefined), but always names memory the
/// program may read and write. Freeing it on a re-OPEN, as the assembly
/// runtime did, left LSET writing freed memory. That is at most 15 buffers,
/// each allocated only when its number is first opened FOR RANDOM.
struct Files {
    /// The stream; null when not open. Slot 0 is unused: the console is
    /// always open, as `sys::stdout()`.
    stream: [*mut FILE; FILES],
    /// How each number was opened.
    mode: [i64; FILES],
    /// WIDTH: the line length output wraps at; 0 = no limit.
    width: [i64; FILES],
    /// Characters already on the current output line, so that TAB(n) knows
    /// how far to advance. Reset by a newline.
    col: [i64; FILES],
    /// The record buffer: MAX_RECLEN bytes, malloc'd by the first OPEN FOR
    /// RANDOM of the number and never freed (see above). The number is a
    /// random file only while `mode` says so too: `random_buf`.
    recbuf: [*mut u8; FILES],
    reclen: [i64; FILES],
    /// The last record read or written, 1-based; 0 = none.
    recnum: [i64; FILES],
    /// Bytes of the buffer FIELD has assigned so far.
    fieldoff: [i64; FILES],
}

static TABLE: State<Files> = State::new(Files {
    stream: [ptr::null_mut(); FILES],
    mode: {
        let mut m = [INPUT; FILES];
        m[0] = OUTPUT; // the console is written to
        m
    },
    width: [0; FILES],
    col: [0; FILES],
    recbuf: [ptr::null_mut(); FILES],
    reclen: [0; FILES],
    recnum: [0; FILES],
    fieldoff: [0; FILES],
});

/// The table, for one statement's use: no reference is held across a call.
fn files() -> &'static mut Files {
    unsafe { TABLE.get() }
}

/// INPUT #'s field and LINE INPUT #'s line, valid until the next read.
static FIELD_BUF: State<[u8; MAX_FIELD_LEN + 1]> = State::new([0; MAX_FIELD_LEN + 1]);

/// A file name with its NUL, for the C library.
static NAME_BUF: State<[u8; NAME_CAP]> = State::new([0; NAME_CAP]);

/// The slot for file number `n`, if there is one. Codegen checks the range
/// (but not under `--unsafe`), so one outside it is a number never opened.
fn slot(n: i64) -> Option<usize> {
    if (0..FILES as i64).contains(&n) {
        Some(n as usize)
    } else {
        None
    }
}

/// The stream for file number `n`; null when it is not open.
fn stream(n: i64) -> *mut FILE {
    match slot(n) {
        Some(0) => sys::stdout(),
        Some(i) => files().stream[i],
        None => ptr::null_mut(),
    }
}

fn mode(n: i64) -> i64 {
    slot(n).map_or(INPUT, |i| files().mode[i])
}

fn col(n: i64) -> i64 {
    slot(n).map_or(0, |i| files().col[i])
}

fn set_col(n: i64, c: i64) {
    if let Some(i) = slot(n) {
        files().col[i] = c;
    }
}

/// The record buffer of file number `n`, if it is open FOR RANDOM; null when
/// it is not. CLOSE leaves `mode` and the buffer as they were, so the stream
/// is what says the number is still open.
fn random_buf(n: i64) -> *mut u8 {
    match slot(n) {
        Some(i) if files().mode[i] == RANDOM && !files().stream[i].is_null() => files().recbuf[i],
        _ => ptr::null_mut(),
    }
}

fn width(n: i64) -> i64 {
    slot(n).map_or(0, |i| files().width[i])
}

/// The console's column, for POS: characters already written on its
/// current line, so column 1 is 0.
pub fn console_column() -> i64 {
    files().col[0]
}

/// Set the console's column, for CLS (0) and LOCATE (the column less 1),
/// so that a following TAB or PRINT zone counts from where the cursor is.
pub fn set_console_column(c: i64) {
    files().col[0] = c;
}

/// Write `bytes` to the console, as PRINT does: the column follows them.
pub fn con_string(bytes: &[u8]) {
    print_bytes(0, bytes);
}

/// Is file `n` open, and open for writing? Raises the error when not:
/// PRINT # to a file opened FOR INPUT used to be a write libc quietly
/// refused.
fn writable(n: i64) -> bool {
    if stream(n).is_null() {
        raise(&error::_err_badfile[..], 0);
        false
    } else if !matches!(mode(n), OUTPUT | APPEND) {
        raise(&error::_err_badmode[..], 0);
        false
    } else {
        true
    }
}

/// Is file `n` open for reading sequentially? The same, for INPUT # and
/// LINE INPUT #.
fn readable(n: i64) -> bool {
    if stream(n).is_null() {
        raise(&error::_err_badfile[..], 0);
        false
    } else if mode(n) != INPUT {
        raise(&error::_err_badmode[..], 0);
        false
    } else {
        true
    }
}

/// PRINT # of a string's bytes, all of them, NULs included.
///
/// Under a WIDTH, the text is cut where the line ends and continued on the
/// next, as GW-BASIC wraps it. The break is taken lazily, just before a
/// character that would not fit, so text that exactly fills a line is
/// followed by one newline, not two.
fn print_bytes(n: i64, mut s: &[u8]) {
    // An unassigned string is (NULL, 0): writing nothing is correct.
    if s.is_empty() || !writable(n) {
        return;
    }
    while !s.is_empty() {
        let mut take = s.len();
        let w = width(n);
        if w != 0 {
            let mut room = w - col(n);
            if room <= 0 {
                newline(n); // none left: start the next line
                room = width(n);
            }
            take = take.min(room as usize);
        }
        unsafe { sys::write(stream(n), &s[..take]) };
        set_col(n, col(n) + take as i64);
        s = &s[take..];
    }
}

/// End the line on file `n`.
fn newline(n: i64) {
    if writable(n) {
        unsafe { sys::write(stream(n), sys::NEWLINE) };
        set_col(n, 0);
    }
}

/// One character on file `n`. Under a WIDTH, a full line wraps before it.
fn print_char(n: i64, c: u8) {
    if !writable(n) {
        return;
    }
    let w = width(n);
    if w != 0 && col(n) as u64 >= w as u64 {
        newline(n);
    }
    unsafe { sys::write(stream(n), &[c]) };
    set_col(n, col(n) + 1);
}

/// SPC(count).
fn spc(n: i64, count: i64) {
    for _ in 0..count.max(0) {
        print_char(n, b' ');
    }
}

/// TAB(column). Columns are 1-based, as in GW-BASIC. If the sink is already
/// at or past the column, a newline is written first and the tab applies to
/// the new line.
fn tab(n: i64, column: i64) {
    let target = column.max(1) - 1; // characters before it
    if target < col(n) {
        newline(n);
    }
    spc(n, target - col(n));
}

/// Where PRINT's comma goes from column `col`, under width `width` (0 = no
/// limit): `Some(c)` to TAB(c), or `None` for the next line, because a zone
/// that would not fit whole on the line means the next line, as in
/// GW-BASIC.
fn zone_target(col: i64, width: i64) -> Option<i64> {
    let next = (col as u64 / PRINT_ZONE as u64 + 1) as i64 * PRINT_ZONE; // 0-based
    if width != 0 && (next + PRINT_ZONE) as u64 > width as u64 {
        None
    } else {
        Some(next + 1)
    }
}

/// PRINT of a number's text, as `num` formatted it, with PRINT's blank
/// after it. Under a WIDTH, a number that does not fit on the rest of the
/// line starts the next one, as in GW-BASIC, rather than being split.
fn print_number(n: i64, text: num::NumText) {
    let text = text.with_trailing_blank();
    let s = text.as_bytes();
    let (w, c) = (width(n), col(n));
    if w != 0 && c != 0 && (c + s.len() as i64) as u64 > w as u64 {
        newline(n);
    }
    print_bytes(n, s);
}

/// The next byte of file `n`, or -1 at its end or when it is not open.
fn getc(n: i64) -> i32 {
    let f = stream(n);
    if f.is_null() {
        -1
    } else {
        unsafe { sys::fgetc(f) }
    }
}

/// Push `c` back onto file `n`, to be read next.
fn ungetc(n: i64, c: i32) {
    unsafe { sys::ungetc(c, stream(n)) };
}

/// Read one INPUT # field into FIELD_BUF; `numeric` for a numeric item.
///
/// INPUT # reads comma-delimited fields, not whole lines. Leading blanks
/// and line breaks are skipped. A field either is quoted, in which case it
/// runs to the closing quote and everything up to the next delimiter is
/// discarded, or runs to the next comma, newline or end of file with
/// trailing blanks trimmed. The delimiter is consumed.
///
/// An unquoted field read for a number also ends at a blank, as GW-BASIC's
/// INPUT # does: `PRINT #1, 1; 2; 3` writes " 1  2  3 " on one line, and
/// that is three numbers. The blanks after such a number are separators,
/// and so is a comma or line break that follows them, so " 1 , 2" is still
/// two numbers; anything else is the start of the next field, and is put
/// back.
fn read_field(n: i64, numeric: bool) -> &'static [u8] {
    let buf = unsafe { FIELD_BUF.get() };
    let mut len = 0usize;
    if readable(n) {
        len = read_field_into(n, numeric, buf).min(MAX_FIELD_LEN);
    }
    buf[len] = 0;
    &buf[..len]
}

fn read_field_into(n: i64, numeric: bool, buf: &mut [u8; MAX_FIELD_LEN + 1]) -> usize {
    let mut len = 0usize;
    // Skip leading blanks, tabs and line breaks.
    let mut c = loop {
        match getc(n) {
            -1 => {
                // No field left before the end of the file: GW-BASIC's
                // "Input past end of file" (62), so that a loop that forgot
                // EOF() does not read zeros forever.
                raise(&error::_err_pastend[..], 0);
                return 0;
            }
            0x20 | 9 | 13 | 10 => continue,
            c => break c,
        }
    };
    // A comma here ends an empty field: "10,,20" is three fields, the
    // middle one 0 or "".
    if c == b',' as i32 {
        return 0;
    }
    if c == b'"' as i32 {
        loop {
            match getc(n) {
                -1 => return len,
                0x22 => break,
                c => {
                    if len < MAX_FIELD_LEN {
                        buf[len] = c as u8;
                        len += 1;
                    }
                }
            }
        }
        // Discard whatever separates the closing quote from the delimiter.
        loop {
            match getc(n) {
                -1 | 0x2c | 10 => return len,
                _ => {}
            }
        }
    }
    // Unquoted: this character and everything up to the delimiter.
    loop {
        if len < MAX_FIELD_LEN {
            buf[len] = c as u8;
            len += 1;
        }
        loop {
            c = getc(n);
            match c {
                -1 | 0x2c | 10 => {
                    // Trailing blanks are separators rather than data.
                    while len > 0 && matches!(buf.get(len - 1), Some(b' ' | b'\t')) {
                        len -= 1;
                    }
                    return len;
                }
                13 if !numeric => continue,
                0x20 | 9 if numeric => return number_end(n, len, false),
                13 if numeric => return number_end(n, len, true),
                _ => break,
            }
        }
    }
}

/// A number read by INPUT # has ended at a blank (`cr`: at a CR). More
/// blanks, then a comma or a line break, all belong to its delimiter; the
/// LF of a CRLF goes with the CR, since left behind it would be an empty
/// line to a LINE INPUT # that follows. Anything else starts the next
/// field, and is put back.
fn number_end(n: i64, len: usize, mut cr: bool) -> usize {
    if !cr {
        loop {
            match getc(n) {
                0x20 | 9 => continue,
                -1 | 0x2c | 10 => return len,
                13 => {
                    cr = true;
                    break;
                }
                c => {
                    ungetc(n, c);
                    return len;
                }
            }
        }
    }
    debug_assert!(cr);
    match getc(n) {
        -1 | 10 => {}
        c => ungetc(n, c),
    }
    len
}

/// LINE INPUT #: the next line of file `n` into FIELD_BUF, without its
/// newline, and without the CR before it in a file written with CRLF:
/// dropping only the LF handed every such line back with a stray CR. A line
/// longer than MAX_LINE_LEN comes back in pieces.
fn line_input(n: i64) -> &'static [u8] {
    let buf = unsafe { FIELD_BUF.get() };
    let mut len = 0usize;
    if readable(n) {
        let mut read_any = false;
        let mut saw_lf = false;
        while len < MAX_LINE_LEN {
            let c = getc(n);
            if c < 0 {
                break;
            }
            read_any = true;
            if c == 10 {
                saw_lf = true;
                break;
            }
            buf[len] = c as u8;
            len += 1;
        }
        if !read_any {
            // A read past the end is GW-BASIC's "Input past end of file",
            // so a loop missing its EOF() test says what is wrong.
            raise(&error::_err_pastend[..], 0);
        } else if saw_lf && len > 0 && buf.get(len - 1) == Some(&b'\r') {
            len -= 1;
        }
    }
    let len = len.min(MAX_LINE_LEN);
    buf[len] = 0;
    &buf[..len]
}

/// `bytes` with a NUL, in NAME_BUF; None when too long to name a file.
fn c_name(bytes: &[u8]) -> Option<*const c_char> {
    let buf = unsafe { NAME_BUF.get() };
    if bytes.len() >= NAME_CAP {
        return None;
    }
    buf[..bytes.len()].copy_from_slice(bytes);
    buf[bytes.len()] = 0;
    Some(buf.as_ptr().cast())
}

/// The error for an OPEN in `mode` that failed for `why`. A missing file is
/// "File not found" when it was to be read; when it was to be created, what
/// is missing is the directory.
fn open_error(why: sys::OpenFailure, mode: i64) -> &'static [u8] {
    use sys::OpenFailure::*;
    match why {
        NotFound if mode == INPUT => &error::_err_notfound,
        NotFound | PathNotFound => &error::_err_pathnotfound,
        Denied => &error::_err_permission,
        BadName => &error::_err_badname,
        Other => &error::_err_access,
    }
}

/// The checks every OPEN makes first, and the name for the C library: a
/// number still open must be CLOSEd first, and "" names no file. None when
/// an error was raised.
fn open_name(name: Str, n: i64) -> Option<*const c_char> {
    let Some(i) = slot(n) else {
        raise(&error::_err_badfile[..], 0);
        return None;
    };
    if i == 0 || !files().stream[i].is_null() {
        raise(&error::_err_alreadyopen[..], 0);
        return None;
    }
    let name = c_name(unsafe { name.bytes() }).filter(|_| name.len != 0);
    if name.is_none() {
        raise(&error::_err_badname[..], 0);
    }
    name
}

/// Record a stream just opened on number `n`. A new file has no WIDTH and
/// starts at column 1, whatever the last file on this number left behind;
/// its `mode` alone says whether it is a random one. The record buffer is
/// kept, not freed: FIELD variables may still point into it.
fn bind(n: usize, f: *mut FILE, mode: i64) {
    let n = n % FILES; // what `slot` checked, said so that indexing cannot panic
    let t = files();
    t.stream[n] = f;
    t.mode[n] = mode;
    t.width[n] = 0;
    t.col[n] = 0;
}

/// Seek file `n` to record `rec` (0 = the one after the last) for GET or
/// PUT: its stream, record length and buffer, or None when an error was
/// raised.
fn random_prepare(n: i64, rec: i64, line: i64) -> Option<(*mut FILE, usize, *mut u8)> {
    let f = stream(n);
    if f.is_null() {
        raise(&error::_err_badfile[..], line);
        return None;
    }
    let i = slot(n)?;
    let buf = random_buf(n);
    if buf.is_null() {
        raise(&error::_err_badmode[..], line);
        return None;
    }
    let t = files();
    let reclen = t.reclen[i];
    let rec = if rec > 0 {
        rec
    } else {
        t.recnum[i].wrapping_add(1)
    };
    t.recnum[i] = rec;
    // Records are 1-based, so record n starts at (n-1) * reclen.
    unsafe { sys::seek(f, (rec - 1).wrapping_mul(reclen), sys::SEEK_SET) };
    Some((f, reclen as usize, buf))
}

/// The byte range LOCK covers, as (start, length); a length of 0 is the
/// whole file. A first record of 0, or a file with no records, is the whole
/// file; otherwise records first..last, at least one.
fn lock_range(first: i64, last: i64, reclen: i64) -> (i64, i64) {
    if first <= 0 || reclen == 0 {
        return (0, 0);
    }
    let count = (last - first + 1).max(1);
    ((first - 1) * reclen, count * reclen)
}

/// LOCK or UNLOCK.
fn lock(n: i64, first: i64, last: i64, line: i64, on: bool) {
    let f = stream(n);
    if f.is_null() {
        raise(&error::_err_badfile[..], line);
        return;
    }
    let reclen = slot(n).map_or(0, |i| files().reclen[i]);
    let (start, len) = lock_range(first, last, reclen);
    if !unsafe { sys::lock(f, start, len, on) } {
        raise(&error::_err_permission[..], line);
    }
}

/// The first `width` bytes of `s`, low byte first, as a number's bits; None
/// when `s` is too short. CVI and the rest read MK*$'s bytes back so.
fn gather(s: &[u8], width: usize) -> Option<u64> {
    let bytes = s.get(..width)?;
    Some(
        bytes
            .iter()
            .enumerate()
            .fold(0, |v, (i, &b)| v | (b as u64) << (8 * i)),
    )
}

/// CVI/CVL/CVS/CVD's common part: the bits of the value in the string.
///
/// The string must be at least as wide as the type being read. Zero-padding
/// a short one instead would be worse than useless: two characters read as
/// a DOUBLE do not give 0, they give a denormal near 1e-319, which is a
/// plausible number no program would ever notice was wrong. GW-BASIC calls
/// this an illegal function call, and so does this.
fn cv(p: *const u8, len: i64, line: i64, width: usize) -> Option<u64> {
    let s = Str {
        ptr: p,
        len: len as usize,
    };
    let bits = if (len as u64) < width as u64 {
        None
    } else if p.is_null() {
        Some(0)
    } else {
        gather(unsafe { s.bytes() }, width)
    };
    if bits.is_none() {
        raise(&error::_err_domain[..], line);
    }
    bits
}

fn str_of(s: &'static [u8]) -> Str {
    Str {
        ptr: s.as_ptr(),
        len: s.len(),
    }
}

export! {
    /// OPEN name FOR INPUT/OUTPUT/APPEND AS #n; `mode` 0, 1 or 2.
    fn _rt_file_open(name: *const u8, len: i64, mode: i64, n: i64) {
        let Some(cname) = open_name(Str { ptr: name, len: len as usize }, n) else {
            return;
        };
        let how: &[u8] = match mode {
            INPUT => b"rb\0",
            OUTPUT => b"wb\0",
            _ => b"ab\0",
        };
        let f = unsafe { sys::open(cname, how.as_ptr().cast()) };
        // A failed open used to be stored anyway, so the program carried on
        // with a NULL handle.
        if f.is_null() {
            raise(open_error(unsafe { sys::open_failure(cname) }, mode), 0);
            return;
        }
        // A directory opens for reading on unix -- only the first read
        // fails -- so OPEN "." FOR INPUT used to succeed and INPUT # said
        // "Input past end of file", naming the wrong problem. It is
        // "Path/File access error" (75), as opening one for writing already
        // is: those modes fail in fopen itself, so only INPUT is checked.
        if mode == INPUT && unsafe { sys::is_directory(f) } {
            unsafe { sys::fclose(f) };
            raise(&error::_err_access[..], 0);
            return;
        }
        bind(n as usize, f, mode);
    }

    /// OPEN name FOR RANDOM AS #n LEN = reclen.
    ///
    /// Four arguments, not five: there is no mode, because reaching this
    /// entry point *is* the mode, and Win64 passes only four in registers.
    fn _rt_file_open_random(name: *const u8, len: i64, n: i64, reclen: i64) {
        // A record length outside 1..MAX_RECLEN is an Illegal function
        // call, as in GW-BASIC, refused before anything is created. A
        // missing LEN clause is not 0 here: codegen passes the default, 128.
        if !(1..=MAX_RECLEN).contains(&reclen) {
            raise(&error::_err_domain[..], 0);
            return;
        }
        let Some(cname) = open_name(Str { ptr: name, len: len as usize }, n) else {
            return;
        };
        let i = n as usize % FILES; // what `slot` checked
        // The number's buffer from an earlier OPEN, or its first one,
        // allocated before the file is opened: binding the file first left
        // it open, and unusable, when there was no memory for one.
        let mut buf = files().recbuf[i];
        if buf.is_null() {
            buf = unsafe { sys::malloc(MAX_RECLEN as usize) }.cast::<u8>();
            if buf.is_null() {
                raise(&error::_err_memory[..], 0);
                return;
            }
            files().recbuf[i] = buf;
        }
        // A random file is read *and* written, so it is opened for update.
        // "r+b" fails when the file does not exist yet, in which case it is
        // created.
        let mut f = unsafe { sys::open(cname, c"r+b".as_ptr()) };
        if f.is_null() {
            f = unsafe { sys::open(cname, c"w+b".as_ptr()) };
        }
        if f.is_null() {
            raise(open_error(unsafe { sys::open_failure(cname) }, RANDOM), 0);
            return;
        }
        bind(i, f, RANDOM);
        files().reclen[i] = reclen;
        // A new record reads as blanks, so a field never written holds
        // spaces rather than whatever the allocator or the last file left.
        unsafe { ptr::write_bytes(buf, b' ', reclen as usize) };
        let t = files();
        t.recnum[i] = 0;
        t.fieldoff[i] = 0;
    }

    /// CLOSE #n. The console is never closed.
    fn _rt_file_close(n: i64) {
        if let Some(i @ 1..) = slot(n) {
            let f = files().stream[i];
            if !f.is_null() {
                unsafe {
                    sys::fflush(f);
                    sys::fclose(f);
                }
                files().stream[i] = ptr::null_mut();
            }
        }
    }

    /// CLOSE: every open file.
    fn _rt_file_close_all() {
        for n in 1..FILES as i64 {
            unsafe { _rt_file_close(n) };
        }
    }

    /// PRINT # of a string.
    fn _rt_file_print_string(n: i64, p: *const u8, len: i64) {
        print_bytes(n, unsafe { Str { ptr: p, len: len as usize }.bytes() });
    }

    /// WRITE # of a DOUBLE: its digits, without PRINT's blanks.
    fn _rt_file_print_float(n: i64, x: f64) {
        print_bytes(n, num::fmt_double(x, false).as_bytes());
    }

    /// WRITE # of a SINGLE, which has only 7 significant digits to show.
    fn _rt_file_print_single(n: i64, x: f64) {
        print_bytes(n, num::fmt_double(x, true).as_bytes());
    }

    /// PRINT a DOUBLE: a blank where the sign would go, and a blank after.
    fn _rt_print_number(n: i64, x: f64) {
        print_number(n, num::fmt_basic(x, false));
    }

    /// PRINT a SINGLE, whose shorter digits are the reason PRINT keeps two
    /// entry points at all.
    fn _rt_print_number_single(n: i64, x: f64) {
        print_number(n, num::fmt_basic(x, true));
    }

    /// PRINT's comma: move to the start of the next 14-column print zone,
    /// padding with blanks, or to the next line past the last one.
    fn _rt_print_zone(n: i64) {
        match zone_target(col(n), width(n)) {
            Some(c) => tab(n, c),
            None => newline(n),
        }
    }

    /// SPC(count).
    fn _rt_file_print_spc(n: i64, count: i64) {
        spc(n, count);
    }

    /// TAB(column).
    fn _rt_file_print_tab(n: i64, column: i64) {
        tab(n, column);
    }

    /// One character, by its code.
    fn _rt_file_print_char(n: i64, c: i64) {
        print_char(n, c as u8);
    }

    /// End the line: at the end of a PRINT # not ended by ; or ,.
    fn _rt_file_print_newline(n: i64) {
        newline(n);
    }

    /// WIDTH [#n,] w: wrap output to file n at w columns. 255 means no
    /// limit, as in GW-BASIC, and is what every file starts with. The range
    /// 1-255 is checked by the caller.
    fn _rt_width(n: i64, w: i64) {
        if stream(n).is_null() {
            raise(&error::_err_badfile[..], 0);
            return;
        }
        files().width[n as usize] = if w == 255 { 0 } else { w };
    }

    /// INPUT # of a string: the next field, valid until the next read.
    fn _rt_file_input_string(n: i64) -> Str {
        str_of(read_field(n, false))
    }

    /// INPUT # of a number: the next field, read as VAL reads a string.
    /// Text that is not a number is 0, and one too large for a DOUBLE is
    /// Overflow.
    fn _rt_file_input_number(n: i64) -> f64 {
        let s = num::scan_number(read_field(n, true));
        if s.overflow {
            raise(&error::_err_overflow[..], 0);
            return 0.0;
        }
        s.value
    }

    /// LINE INPUT #: the next whole line, valid until the next read.
    fn _rt_file_line_input(n: i64) -> Str {
        str_of(line_input(n))
    }

    /// EOF(n): -1 when the file has been read to its end (or is not open),
    /// else 0. It peeks one character and pushes it back, since feof()
    /// reports the end only after a read has already failed, which would
    /// make EOF() lag by one line.
    fn _rt_file_eof(n: i64) -> i32 {
        match getc(n) {
            -1 => -1,
            c => {
                ungetc(n, c);
                0
            }
        }
    }

    /// LOC(n): for a random file the last record read or written; for a
    /// sequential one the position in 128-byte blocks, as GW-BASIC counts.
    fn _rt_file_loc(n: i64) -> f64 {
        let Some(i) = slot(n) else { return 0.0 };
        if !random_buf(n).is_null() {
            return files().recnum[i] as f64;
        }
        let f = stream(n);
        if f.is_null() {
            return 0.0;
        }
        match unsafe { sys::tell(f) } {
            pos if pos < 0 => 0.0,
            pos => (pos / 128) as f64,
        }
    }

    /// LOF(n): the file's length in bytes; 0 when it is not open.
    fn _rt_file_lof(n: i64) -> f64 {
        let f = stream(n);
        if f.is_null() {
            return 0.0;
        }
        unsafe {
            let pos = sys::tell(f);
            sys::seek(f, 0, sys::SEEK_END);
            let end = sys::tell(f);
            sys::seek(f, pos, sys::SEEK_SET);
            end as f64
        }
    }

    /// Start a fresh FIELD statement.
    fn _rt_field_reset(n: i64) {
        if let Some(i) = slot(n) {
            files().fieldoff[i] = 0;
        }
    }

    /// FIELD's next variable: the next `w` bytes of the record buffer. The
    /// fields of one record must fit inside it.
    fn _rt_field_next(n: i64, w: i64) -> Str {
        let buf = random_buf(n);
        if buf.is_null() {
            // FIELD on a number not open is Bad file number; on a file not
            // opened FOR RANDOM, which has no buffer to carve, Bad file mode.
            if stream(n).is_null() {
                raise(&error::_err_badfile[..], 0);
            } else {
                raise(&error::_err_badmode[..], 0);
            }
            return Str::EMPTY;
        }
        let i = n as usize % FILES; // what `random_buf` checked
        let w = w.max(0); // a negative width is empty, not a step back
        let off = files().fieldoff[i]; // 0..=reclen
        // Compared as room left, not as `off + w`: a width near 2^63 wrapped
        // that sum negative, which passed, and handed out a window reaching
        // far past the buffer. The assembly runtime had the same flaw.
        if w > files().reclen[i] - off {
            raise(&error::_err_fieldovf[..], 0);
            return Str::EMPTY;
        }
        files().fieldoff[i] = off + w;
        Str {
            ptr: unsafe { buf.add(off as usize) },
            len: w as usize,
        }
    }

    /// LSET: copy left-justified into a fixed-width field. The destination
    /// keeps its address and width; only the bytes change. A short source
    /// is padded with spaces, a long one cut on the right.
    fn _rt_lset(dst: *mut u8, dlen: i64, src: *const u8, slen: i64) {
        if dlen <= 0 {
            return;
        }
        let k = (slen as u64).min(dlen as u64) as usize;
        unsafe {
            if k > 0 {
                ptr::copy(src, dst, k);
            }
            ptr::write_bytes(dst.add(k), b' ', dlen as usize - k);
        }
    }

    /// RSET: the same, right-justified.
    fn _rt_rset(dst: *mut u8, dlen: i64, src: *const u8, slen: i64) {
        if dlen <= 0 {
            return;
        }
        let k = (slen as u64).min(dlen as u64) as usize;
        let pad = dlen as usize - k;
        unsafe {
            ptr::write_bytes(dst, b' ', pad);
            if k > 0 {
                ptr::copy(src, dst.add(pad), k);
            }
        }
    }

    /// GET #n[, rec]: read one record into the buffer. A record reaching
    /// past the end of the file is padded with NULs, so a field read from
    /// beyond the data is empty rather than holding the previous record.
    fn _rt_file_get(n: i64, rec: i64, line: i64) {
        if let Some((f, reclen, buf)) = random_prepare(n, rec, line) {
            unsafe {
                let got = sys::fread(buf.cast(), 1, reclen, f);
                ptr::write_bytes(buf.add(got), 0, reclen - got);
            }
        }
    }

    /// PUT #n[, rec]: write the buffer as one record, flushed so that LOF
    /// and a reader on another handle see it now.
    fn _rt_file_put(n: i64, rec: i64, line: i64) {
        if let Some((f, reclen, buf)) = random_prepare(n, rec, line) {
            unsafe {
                sys::fwrite(buf.cast(), 1, reclen, f);
                sys::fflush(f);
            }
        }
    }

    /// LOCK #n[, first [TO last]]: records first..last, or with 0 the whole
    /// file.
    fn _rt_lock(n: i64, first: i64, last: i64, line: i64) {
        lock(n, first, last, line, true);
    }

    /// UNLOCK, with LOCK's arguments.
    fn _rt_unlock(n: i64, first: i64, last: i64, line: i64) {
        lock(n, first, last, line, false);
    }

    /// MKI$/MKL$/MKS$/MKD$: a number's bytes, as a fresh string. The value
    /// arrives as raw bits so one helper serves all four widths: the caller
    /// has already coerced it to the type whose width it passes.
    fn _rt_mk(bits: i64, width: i64) -> Str {
        let bytes = bits.to_le_bytes();
        crate::string::strdup(&bytes[..(width.clamp(0, 8) as usize)])
    }

    /// CVI: MKI$'s bytes back as an INTEGER.
    fn _rt_cvi(p: *const u8, len: i64, line: i64) -> f64 {
        cv(p, len, line, 2).map_or(0.0, |b| b as u16 as i16 as f64)
    }

    /// CVL: MKL$'s bytes back as a LONG.
    fn _rt_cvl(p: *const u8, len: i64, line: i64) -> f64 {
        cv(p, len, line, 4).map_or(0.0, |b| b as u32 as i32 as f64)
    }

    /// CVS: MKS$'s bytes back as a SINGLE.
    fn _rt_cvs(p: *const u8, len: i64, line: i64) -> f64 {
        cv(p, len, line, 4).map_or(0.0, |b| f32::from_bits(b as u32) as f64)
    }

    /// CVD: MKD$'s bytes back as a DOUBLE.
    fn _rt_cvd(p: *const u8, len: i64, line: i64) -> f64 {
        cv(p, len, line, 8).map_or(0.0, f64::from_bits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The file table is shared, and tests run on threads.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A path for `name` in a fresh directory, removed when the guard is
    /// dropped. A test running in a child process uses the directory its
    /// parent made instead, and the parent removes it: the child may end
    /// with a fatal error, which exits without dropping anything.
    fn temp(name: &str) -> (Option<tempfile::TempDir>, String) {
        if let Some(dir) = std::env::var_os(crate::num::testing::TMP_VAR) {
            let path = std::path::Path::new(&dir).join(name);
            return (None, path.to_str().unwrap().to_string());
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name).to_str().unwrap().to_string();
        (Some(dir), path)
    }

    fn open(name: &str, mode: i64, n: i64) {
        unsafe { _rt_file_open(name.as_ptr(), name.len() as i64, mode, n) };
        assert!(!stream(n).is_null(), "{name} did not open");
    }

    fn put(n: i64, s: &str) {
        unsafe { _rt_file_print_string(n, s.as_ptr(), s.len() as i64) };
    }

    fn text(s: Str) -> String {
        String::from_utf8(unsafe { s.bytes() }.to_vec()).unwrap()
    }

    #[test]
    fn open_errors_map_to_gw_basic_numbers() {
        use sys::OpenFailure::*;
        assert_eq!(
            open_error(NotFound, INPUT).as_ptr(),
            error::_err_notfound.as_ptr()
        );
        assert_eq!(
            open_error(NotFound, OUTPUT).as_ptr(),
            error::_err_pathnotfound.as_ptr()
        );
        assert_eq!(
            open_error(NotFound, RANDOM).as_ptr(),
            error::_err_pathnotfound.as_ptr()
        );
        assert_eq!(
            open_error(PathNotFound, INPUT).as_ptr(),
            error::_err_pathnotfound.as_ptr()
        );
        assert_eq!(
            open_error(Denied, APPEND).as_ptr(),
            error::_err_permission.as_ptr()
        );
        assert_eq!(
            open_error(BadName, INPUT).as_ptr(),
            error::_err_badname.as_ptr()
        );
        assert_eq!(
            open_error(Other, INPUT).as_ptr(),
            error::_err_access.as_ptr()
        );
    }

    #[cfg(unix)]
    #[test]
    fn errno_says_why_an_open_failed() {
        let _g = serial();
        let missing = c"/nonexistent-xbrt/dir/file";
        assert!(unsafe { sys::open(missing.as_ptr(), c"rb".as_ptr()) }.is_null());
        assert_eq!(
            unsafe { sys::open_failure(missing.as_ptr()) },
            sys::OpenFailure::NotFound
        );
        let notdir = c"/dev/null/x";
        assert!(unsafe { sys::open(notdir.as_ptr(), c"rb".as_ptr()) }.is_null());
        assert_eq!(
            unsafe { sys::open_failure(notdir.as_ptr()) },
            sys::OpenFailure::PathNotFound
        );
        let long = std::ffi::CString::new("x".repeat(5000)).unwrap();
        assert!(unsafe { sys::open(long.as_ptr(), c"rb".as_ptr()) }.is_null());
        assert_eq!(
            unsafe { sys::open_failure(long.as_ptr()) },
            sys::OpenFailure::BadName
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_is_told_apart() {
        let f = unsafe { sys::open(c".".as_ptr(), c"rb".as_ptr()) };
        assert!(!f.is_null());
        assert!(unsafe { sys::is_directory(f) });
        unsafe { sys::fclose(f) };
        let f = unsafe { sys::open(c"Cargo.toml".as_ptr(), c"rb".as_ptr()) };
        assert!(!f.is_null());
        assert!(!unsafe { sys::is_directory(f) });
        unsafe { sys::fclose(f) };
    }

    #[test]
    fn print_zones_and_tabs() {
        assert_eq!(zone_target(0, 0), Some(15));
        assert_eq!(zone_target(13, 0), Some(15));
        assert_eq!(zone_target(14, 0), Some(29));
        assert_eq!(zone_target(70, 0), Some(85));
        // A zone that would not fit whole under a WIDTH: the next line.
        assert_eq!(zone_target(14, 42), Some(29));
        assert_eq!(zone_target(14, 41), None);
        assert_eq!(zone_target(0, 28), Some(15));
        assert_eq!(zone_target(0, 27), None);
        assert_eq!(zone_target(0, 13), None);
    }

    #[test]
    fn sequential_round_trip() {
        let _g = serial();
        let (_dir, path) = temp("seq.txt");
        open(&path, OUTPUT, 1);
        put(1, "abc");
        assert_eq!(col(1), 3);
        unsafe { _rt_print_zone(1) };
        assert_eq!(col(1), 14);
        put(1, "x");
        unsafe { _rt_file_print_tab(1, 20) };
        assert_eq!(col(1), 19);
        unsafe { _rt_file_print_char(1, b',' as i64) };
        unsafe { _rt_file_print_char(1, b'"' as i64) };
        put(1, "q, r");
        unsafe { _rt_file_print_char(1, b'"' as i64) };
        put(1, ",tail  ");
        unsafe { _rt_file_print_newline(1) };
        assert_eq!(col(1), 0);
        put(1, "10,,20\r\n");
        put(1, "a\0b\r\n");
        put(1, "last");
        unsafe { _rt_file_close(1) };
        assert!(stream(1).is_null());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            [
                &b"abc           x    ,\"q, r\",tail  "[..],
                sys::NEWLINE,
                b"10,,20\r\na\0b\r\nlast"
            ]
            .concat()
        );

        open(&path, INPUT, 2);
        assert_eq!(unsafe { _rt_file_lof(2) }, 50.0 + sys::NEWLINE.len() as f64);
        assert_eq!(unsafe { _rt_file_eof(2) }, 0);
        let field = || text(unsafe { _rt_file_input_string(2) });
        assert_eq!(field(), "abc           x");
        assert_eq!(field(), "q, r");
        assert_eq!(field(), "tail");
        assert_eq!(field(), "10");
        assert_eq!(field(), "");
        assert_eq!(field(), "20");
        // LINE INPUT keeps NULs, and drops the CR of a CRLF.
        assert_eq!(unsafe { _rt_file_line_input(2).bytes() }, b"a\0b");
        assert_eq!(unsafe { _rt_file_eof(2) }, 0);
        assert_eq!(text(unsafe { _rt_file_line_input(2) }), "last");
        assert_eq!(unsafe { _rt_file_eof(2) }, -1);
        assert_eq!(unsafe { _rt_file_loc(2) }, 0.0);
        unsafe { _rt_file_close_all() };
        assert!(stream(2).is_null());
    }

    #[test]
    fn numeric_fields_end_at_a_blank() {
        let _g = serial();
        let (_dir, path) = temp("nums.txt");
        std::fs::write(&path, " 1  2  3 \n 4 , 5\r\nx 6 y\r\nnext\n").unwrap();
        open(&path, INPUT, 3);
        let num = || {
            std::str::from_utf8(read_field(3, true))
                .unwrap()
                .to_string()
        };
        assert_eq!(num(), "1");
        assert_eq!(num(), "2");
        assert_eq!(num(), "3");
        assert_eq!(num(), "4");
        assert_eq!(num(), "5");
        assert_eq!(num(), "x");
        assert_eq!(num(), "6");
        assert_eq!(num(), "y");
        // The CR's LF went with it: no empty line before "next".
        assert_eq!(text(unsafe { _rt_file_line_input(3) }), "next");
        unsafe { _rt_file_close(3) };
    }

    #[test]
    fn width_wraps_lines() {
        let _g = serial();
        let (_dir, path) = temp("width.txt");
        open(&path, OUTPUT, 4);
        unsafe { _rt_width(4, 5) };
        put(4, "abcdefghijkl");
        unsafe { _rt_file_print_newline(4) };
        put(4, "abcde");
        unsafe { _rt_file_print_newline(4) };
        put(4, "abc");
        unsafe { _rt_file_print_spc(4, 4) };
        unsafe { _rt_width(4, 255) };
        put(4, "abcdefgh");
        unsafe { _rt_file_close(4) };
        let nl = std::str::from_utf8(sys::NEWLINE).unwrap();
        let want = ["abcde", "fghij", "kl", "abcde", "abc  ", "  abcdefgh"].join(nl);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), want);
    }

    #[test]
    fn random_files() {
        let _g = serial();
        let (_dir, path) = temp("random.dat");
        let _ = std::fs::remove_file(&path);
        unsafe { _rt_file_open_random(path.as_ptr(), path.len() as i64, 5, 8) };
        assert!(!stream(5).is_null());
        unsafe { _rt_field_reset(5) };
        let a = unsafe { _rt_field_next(5, 3) };
        let b = unsafe { _rt_field_next(5, 5) };
        assert_eq!(unsafe { b.ptr.offset_from(a.ptr) }, 3);
        assert_eq!(unsafe { a.bytes() }, b"   ");
        let set = |f: Str, s: &[u8], right: bool| unsafe {
            let args = (f.ptr as *mut u8, f.len as i64, s.as_ptr(), s.len() as i64);
            if right {
                _rt_rset(args.0, args.1, args.2, args.3)
            } else {
                _rt_lset(args.0, args.1, args.2, args.3)
            }
        };
        set(a, b"x", false);
        set(b, b"yz", true);
        unsafe { _rt_file_put(5, 2, 0) };
        set(a, b"toolong", false);
        set(b, b"", true);
        unsafe { _rt_file_put(5, 0, 0) };
        assert_eq!(unsafe { _rt_file_loc(5) }, 3.0);
        assert_eq!(unsafe { _rt_file_lof(5) }, 24.0);
        unsafe { _rt_file_get(5, 2, 0) };
        assert_eq!(unsafe { a.bytes() }, b"x  ");
        assert_eq!(unsafe { b.bytes() }, b"   yz");
        unsafe { _rt_file_get(5, 0, 0) };
        assert_eq!(unsafe { a.bytes() }, b"too");
        assert_eq!(unsafe { b.bytes() }, b"     ");
        // Record 1 was never written: the gap reads as NULs.
        unsafe { _rt_file_get(5, 1, 0) };
        assert_eq!(unsafe { a.bytes() }, b"\0\0\0");
        // Past the end of the file: NULs too.
        unsafe { _rt_file_get(5, 9, 0) };
        assert_eq!(unsafe { b.bytes() }, b"\0\0\0\0\0");
        assert_eq!(unsafe { _rt_file_loc(5) }, 9.0);
        unsafe { _rt_lock(5, 1, 2, 0) };
        unsafe { _rt_unlock(5, 1, 2, 0) };
        unsafe { _rt_lock(5, 0, 0, 0) };
        unsafe { _rt_unlock(5, 0, 0, 0) };
        unsafe { _rt_file_close(5) };
        // Reopened sequentially, the number is not a random file any more.
        open(&path, INPUT, 5);
        assert!(random_buf(5).is_null());
        unsafe { _rt_file_close(5) };
    }

    /// A FIELD width so large that the offset plus it overflows is a FIELD
    /// overflow, not a window reaching past the buffer: `off + w` wrapped
    /// negative and passed the `> reclen` test, handing out a string of
    /// length 2^63-1 at the buffer, and the next field an offset below it.
    #[test]
    fn a_huge_field_width_is_field_overflow() {
        use crate::num::testing::*;
        if in_child() {
            let (_dir, path) = temp("hugefield.dat");
            unsafe { _rt_file_open_random(path.as_ptr(), path.len() as i64, 6, 8) };
            unsafe { _rt_field_reset(6) };
            let a = unsafe { _rt_field_next(6, 3) };
            assert_eq!(a.len, 3);
            for w in [i64::MAX, i64::MAX - 2, i64::MAX - 3] {
                let (s, raised) = trapped(|| unsafe { _rt_field_next(6, w) });
                assert!(raised, "width {w} was not refused");
                assert_eq!(s, Str::EMPTY);
                assert_eq!(files().fieldoff[6], 3);
            }
            // What fits still fits, from where the last good field ended.
            let b = unsafe { _rt_field_next(6, 5) };
            assert_eq!(unsafe { b.ptr.offset_from(a.ptr) }, 3);
            unsafe { _rt_field_next(6, i64::MAX) };
            return;
        }
        expect_fatal(
            "file::tests::a_huge_field_width_is_field_overflow",
            "?FIELD overflow\n",
        );
    }

    /// A number opened FOR RANDOM and then closed keeps its mode and buffer,
    /// but is no random file: FIELD, GET and PUT are Bad file number (52),
    /// and LOC and LOF are 0, as for any number not open. FIELD used to
    /// carve the old buffer, and LOC report the old record.
    #[test]
    fn a_closed_random_file_is_not_open() {
        use crate::error::_rt_err;
        use crate::num::testing::*;
        if in_child() {
            let (_dir, path) = temp("closed.dat");
            unsafe { _rt_file_open_random(path.as_ptr(), path.len() as i64, 8, 8) };
            unsafe { _rt_file_put(8, 3, 0) };
            assert_eq!(unsafe { _rt_file_loc(8) }, 3.0);
            unsafe { _rt_file_close(8) };
            assert!(random_buf(8).is_null());
            let bad_file = |raised: bool| raised && unsafe { _rt_err() } == 52;
            unsafe { _rt_field_reset(8) };
            let (s, raised) = trapped(|| unsafe { _rt_field_next(8, 4) });
            assert!(bad_file(raised));
            assert_eq!(s, Str::EMPTY);
            assert!(bad_file(trapped(|| unsafe { _rt_file_get(8, 1, 0) }).1));
            assert!(bad_file(trapped(|| unsafe { _rt_file_put(8, 1, 0) }).1));
            assert_eq!(trapped(|| unsafe { _rt_file_loc(8) }), (0.0, false));
            assert_eq!(trapped(|| unsafe { _rt_file_lof(8) }), (0.0, false));
            // Open, but not FOR RANDOM: Bad file mode.
            open(&path, INPUT, 8);
            let (_, raised) = trapped(|| unsafe { _rt_field_next(8, 4) });
            assert!(raised && unsafe { _rt_err() } == 54);
            unsafe { _rt_file_close(8) };
            unsafe { _rt_field_next(8, 4) };
            return;
        }
        expect_fatal(
            "file::tests::a_closed_random_file_is_not_open",
            "?Bad file number\n",
        );
    }

    /// A FIELD variable is a window onto its file's record buffer, and
    /// outlives the OPEN that made it: a re-OPEN of the number, sequential or
    /// RANDOM, freed the buffer, and LSET then wrote freed memory and PRINT
    /// read it. Valgrind or ASan sees that; the asserts below see that the
    /// window still names the number's buffer.
    #[test]
    fn a_field_window_outlives_its_file() {
        let _g = serial();
        let (_dir, path) = temp("window.dat");
        let _ = std::fs::remove_file(&path);
        unsafe { _rt_file_open_random(path.as_ptr(), path.len() as i64, 7, 4) };
        unsafe { _rt_field_reset(7) };
        let a = unsafe { _rt_field_next(7, 4) };
        unsafe { _rt_lset(a.ptr as *mut u8, 4, b"ab".as_ptr(), 2) };
        unsafe { _rt_file_put(7, 1, 0) };
        unsafe { _rt_file_close(7) };
        // Reopened sequentially: not a random file, but the window must
        // still be memory the program may write.
        open(&path, INPUT, 7);
        unsafe { _rt_lset(a.ptr as *mut u8, 4, b"cd".as_ptr(), 2) };
        assert_eq!(unsafe { a.bytes() }, b"cd  ");
        unsafe { _rt_file_close(7) };
        // Reopened FOR RANDOM, with a longer record: the old window is still
        // valid memory, and reads the new (blank) record buffer.
        unsafe { _rt_file_open_random(path.as_ptr(), path.len() as i64, 7, 64) };
        assert_eq!(unsafe { a.bytes() }, b"    ");
        unsafe { _rt_file_get(7, 1, 0) };
        assert_eq!(unsafe { a.bytes() }, b"ab  ");
        unsafe { _rt_lset(a.ptr as *mut u8, 4, b"ef".as_ptr(), 2) };
        unsafe { _rt_file_close(7) };
        open(&path, OUTPUT, 7);
        unsafe { _rt_file_close(7) };
        assert_eq!(unsafe { a.bytes() }, b"ef  ");
    }

    #[test]
    fn lset_and_rset() {
        let mut buf = *b"XXXXXX";
        let p = buf.as_mut_ptr();
        unsafe { _rt_lset(p, 4, b"ab".as_ptr(), 2) };
        assert_eq!(&buf, b"ab  XX");
        unsafe { _rt_rset(p, 4, b"ab".as_ptr(), 2) };
        assert_eq!(&buf, b"  abXX");
        unsafe { _rt_rset(p, 4, b"abcdef".as_ptr(), 6) };
        assert_eq!(&buf, b"abcdXX");
        unsafe { _rt_lset(p, 4, ptr::null(), 0) };
        assert_eq!(&buf, b"    XX");
        unsafe { _rt_lset(p, 0, b"zz".as_ptr(), 2) };
        assert_eq!(&buf, b"    XX");
    }

    #[test]
    fn lock_ranges() {
        assert_eq!(lock_range(0, 0, 10), (0, 0));
        assert_eq!(lock_range(3, 0, 0), (0, 0));
        assert_eq!(lock_range(3, 5, 10), (20, 30));
        assert_eq!(lock_range(3, 1, 10), (20, 10));
    }

    #[test]
    fn mk_and_cv_round_trip() {
        let mk = |bits: i64, w: usize| bits.to_le_bytes()[..w].to_vec();
        let cvi = |s: &[u8]| unsafe { _rt_cvi(s.as_ptr(), s.len() as i64, 0) };
        let cvl = |s: &[u8]| unsafe { _rt_cvl(s.as_ptr(), s.len() as i64, 0) };
        let cvs = |s: &[u8]| unsafe { _rt_cvs(s.as_ptr(), s.len() as i64, 0) };
        let cvd = |s: &[u8]| unsafe { _rt_cvd(s.as_ptr(), s.len() as i64, 0) };
        assert_eq!(cvi(&mk(-2, 2)), -2.0);
        assert_eq!(cvi(&mk(32767, 2)), 32767.0);
        assert_eq!(cvi(b"AB"), 0x4241 as f64);
        assert_eq!(cvl(&mk(-100000, 4)), -100000.0);
        assert_eq!(cvs(&mk(1.5f32.to_bits() as i64, 4)), 1.5);
        assert_eq!(cvd(&mk(0.1f64.to_bits() as i64, 8)), 0.1);
        // Longer strings read their first bytes.
        assert_eq!(cvi(b"\x01\x00zzz"), 1.0);
        assert_eq!(gather(b"a", 2), None);
    }
}
