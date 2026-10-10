//! File I/O tests

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::{compile_and_run, compile_and_run_raw, compile_and_run_with_files};
use std::fs;

#[test]
fn test_file_write() {
    let source = r#"
OPEN "output.txt" FOR OUTPUT AS #1
PRINT #1, "Hello, File!"
PRINT #1, 42
CLOSE #1
PRINT "done"
"#;

    let (output, tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert!(output.contains("done"), "Output was: {}", output);

    let file_path = tmp.path().join("output.txt");
    if file_path.exists() {
        let file_contents = fs::read_to_string(&file_path).unwrap();
        let lines: Vec<&str> = file_contents.lines().collect();
        assert_eq!(lines, vec!["Hello, File!", " 42 "]);
    }
}

#[test]
fn test_file_read() {
    let source = r#"
OPEN "input.txt" FOR INPUT AS #1
INPUT #1, X
INPUT #1, Y
CLOSE #1
PRINT X + Y
"#;

    let (output, _tmp) = compile_and_run_with_files(source, |path| {
        fs::write(path.join("input.txt"), "10\n20\n").map_err(|e| e.to_string())
    })
    .unwrap();
    assert!(output.contains("30"), "Output was: {}", output);
}

#[test]
fn test_file_append() {
    let source = r#"
OPEN "data.txt" FOR APPEND AS #2
PRINT #2, "Line 3"
CLOSE #2
PRINT "appended"
"#;

    let (output, tmp) = compile_and_run_with_files(source, |path| {
        fs::write(path.join("data.txt"), "Line 1\nLine 2\n").map_err(|e| e.to_string())
    })
    .unwrap();

    assert!(output.contains("appended"), "Output was: {}", output);

    let file_path = tmp.path().join("data.txt");
    if file_path.exists() {
        let file_contents = fs::read_to_string(&file_path).unwrap();
        let lines: Vec<&str> = file_contents.lines().collect();
        assert_eq!(lines, vec!["Line 1", "Line 2", "Line 3"]);
    }
}

/// LANGREF documents `LINE INPUT #1, Text$`, but it never parsed: the file
/// number form was missing entirely from LINE INPUT.
#[test]
fn test_line_input_from_file() {
    let source = r#"
OPEN "l.txt" FOR OUTPUT AS #1
PRINT #1, "a whole line, with commas"
CLOSE #1
OPEN "l.txt" FOR INPUT AS #1
LINE INPUT #1, T$
CLOSE #1
PRINT "["; T$; "]"
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(output.trim(), "[a whole line, with commas]");
}

/// A file number may be any numeric expression; only a literal used to parse.
#[test]
fn test_file_number_expression() {
    let source = r#"
F% = 1
OPEN "e.txt" FOR OUTPUT AS #F%
PRINT #F%, "written"
CLOSE #F%
OPEN "e.txt" FOR INPUT AS #(F% + 0)
LINE INPUT #1, A$
CLOSE #1
PRINT A$
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(output.trim(), "written");
}

/// LANGREF documents bare CLOSE as "close all files"; it was a parse error.
#[test]
fn test_bare_close() {
    let source = r#"
OPEN "a.txt" FOR OUTPUT AS #1
OPEN "b.txt" FOR OUTPUT AS #2
PRINT #1, "one"
PRINT #2, "two"
CLOSE
OPEN "a.txt" FOR INPUT AS #1
LINE INPUT #1, X$
CLOSE
PRINT X$
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(output.trim(), "one");
}

/// EOF() makes read-until-end loops possible; without it the documented file
/// I/O support could not actually be used to read a file of unknown length.
#[test]
fn test_eof_and_lof() {
    let source = r#"
OPEN "d.txt" FOR OUTPUT AS #1
PRINT #1, "one"
PRINT #1, "two"
PRINT #1, "three"
CLOSE #1
OPEN "d.txt" FOR INPUT AS #1
N = 0
WHILE NOT EOF(1)
LINE INPUT #1, L$
PRINT L$
N = N + 1
WEND
CLOSE #1
PRINT "lines:"; N
OPEN "d.txt" FOR INPUT AS #1
PRINT "bytes:"; LOF(1)
CLOSE #1
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "one");
    assert_eq!(lines[1], "two");
    assert_eq!(lines[2], "three");
    assert_eq!(lines[3], "lines: 3");

    // Text files carry the host's line terminator, so LOF counts CRLF on
    // Windows and LF elsewhere: 11 characters of text plus three terminators.
    let terminator = if cfg!(windows) { 2 } else { 1 };
    assert_eq!(
        lines[4],
        // `PRINT "bytes:"; N` -- the number brings its own leading blank.
        format!("bytes: {}", 11 + 3 * terminator),
        "3 lines plus their {} terminators",
        if terminator == 2 { "CRLF" } else { "LF" }
    );
}

/// `WRITE #` separates values with commas only.
///
/// Each comma in the source became both a separator and a literal tab, so the
/// file held `10\t,20\t,"ab"` -- which no INPUT # could read back.
#[test]
fn test_write_file_separators() {
    let output = compile_and_run_with_files(
        "OPEN \"wsep.txt\" FOR OUTPUT AS #1\nWRITE #1, 10, 20, \"ab\"\nCLOSE #1\nOPEN \"wsep.txt\" FOR INPUT AS #1\nLINE INPUT #1, L$\nCLOSE #1\nPRINT \"[\"; L$; \"]\"\n",
        |_| Ok(()),
    )
    .unwrap()
    .0;
    assert_eq!(output.trim(), "[10,20,\"ab\"]");
}

/// `INPUT #` reads one comma-delimited field per variable, not one line.
///
/// Reading a line per variable made `INPUT #1, A, B` on "10,20" yield 10
/// twice and leave the second line unread.
#[test]
fn test_input_file_multiple_fields() {
    let output = compile_and_run_with_files(
        "OPEN \"nums.txt\" FOR INPUT AS #1\nINPUT #1, A, B\nINPUT #1, C\nCLOSE #1\nPRINT A; \"/\"; B; \"/\"; C\n",
        |dir| fs::write(dir.join("nums.txt"), "10,20\n30\n").map_err(|e| e.to_string()),
    )
    .unwrap()
    .0;
    assert_eq!(output.trim(), "10 / 20 / 30");
}

/// A quoted field may contain the delimiter, and blanks around a field are
/// separators rather than data.
#[test]
fn test_input_file_quoted_fields() {
    let output = compile_and_run_with_files(
        "OPEN \"q.txt\" FOR INPUT AS #1\nINPUT #1, X$, Y$\nINPUT #1, Z$, W$\nCLOSE #1\nPRINT \"[\"; X$; \"][\"; Y$; \"][\"; Z$; \"][\"; W$; \"]\"\n",
        |dir| {
            fs::write(dir.join("q.txt"), "a, b\n\"c,d\" , e\n").map_err(|e| e.to_string())
        },
    )
    .unwrap()
    .0;
    assert_eq!(output.trim(), "[a][b][c,d][e]");
}

/// What WRITE # writes, INPUT # reads back.
#[test]
fn test_write_file_round_trip() {
    let output = compile_and_run_with_files(
        "OPEN \"wrt.txt\" FOR OUTPUT AS #1\nWRITE #1, 10, 20, \"ab\"\nCLOSE #1\nOPEN \"wrt.txt\" FOR INPUT AS #1\nINPUT #1, A, B, C$\nCLOSE #1\nPRINT A; B; \"[\"; C$; \"]\"\n",
        |_| Ok(()),
    )
    .unwrap()
    .0;
    assert_eq!(output.trim(), "10  20 [ab]");
}

/// LINE INPUT # still takes the whole line, commas and all.
#[test]
fn test_line_input_takes_whole_line() {
    let output = compile_and_run_with_files(
        "OPEN \"l.txt\" FOR INPUT AS #1\nLINE INPUT #1, L$\nLINE INPUT #1, M$\nCLOSE #1\nPRINT \"[\"; L$; \"][\"; M$; \"]\"\n",
        |dir| fs::write(dir.join("l.txt"), "a, b\nc\n").map_err(|e| e.to_string()),
    )
    .unwrap()
    .0;
    assert_eq!(output.trim(), "[a, b][c]");
}

/// A file written on the other platform must read correctly.
///
/// Text files carry the host's line terminator, so a CRLF file is the ordinary
/// case for anything produced on Windows. `LINE INPUT #` stripped only the LF
/// and handed back a trailing CR on every line, which then reappeared in every
/// comparison and every LEN.
#[test]
fn test_crlf_file_reads_without_stray_carriage_returns() {
    let output = compile_and_run_with_files(
        "OPEN \"crlf.txt\" FOR INPUT AS #1\nINPUT #1, A$, B$\nLINE INPUT #1, C$\nCLOSE #1\nPRINT \"[\"; A$; \"][\"; B$; \"][\"; C$; \"]\"\nPRINT LEN(A$); LEN(B$); LEN(C$)\n",
        |dir| {
            fs::write(dir.join("crlf.txt"), "one,two\r\nthree\r\n").map_err(|e| e.to_string())
        },
    )
    .unwrap()
    .0;
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "[one][two][three]");
    assert_eq!(lines[1], "3  3  5", "no line may keep its carriage return");
}

/// A SINGLE written to a file reads back the way the console prints it.
///
/// `gen_print_expr` picks `_rt_print_single` for a SINGLE so that only the ~7
/// digits it carries are shown; `gen_print_expr_to_file`, a copy that never
/// got that fix, always used the double helper, so `PRINT #1, A!` wrote
/// 0.3333333432674408 where `PRINT A!` gives 0.33333334.
#[test]
fn test_print_file_single_matches_console() {
    let output = compile_and_run_with_files(
        "A! = 1 / 3\nPRINT A!\nOPEN \"s.txt\" FOR OUTPUT AS #1\nPRINT #1, A!\nCLOSE #1\nOPEN \"s.txt\" FOR INPUT AS #1\nLINE INPUT #1, L$\nCLOSE #1\nPRINT L$\n",
        |_| Ok(()),
    )
    .unwrap()
    .0;
    let lines = crate::common::lines(&output);
    assert_eq!(
        lines[0], lines[1],
        "a SINGLE must render the same to a file as to the console"
    );
}

/// TAB and SPC position within a file, and write nothing to the console.
///
/// Only the console printer special-cases them; the file copy evaluated
/// `TAB(10)` as an ordinary call, which positioned *stdout* and then wrote the
/// call's dummy 0 into the file, giving "a0b0c".
#[test]
fn test_print_file_tab_spc() {
    let output = compile_and_run_with_files(
        "OPEN \"t.txt\" FOR OUTPUT AS #1\nPRINT #1, \"a\"; TAB(10); \"b\"; SPC(3); \"c\"\nCLOSE #1\nOPEN \"t.txt\" FOR INPUT AS #1\nLINE INPUT #1, L$\nCLOSE #1\nPRINT \"[\"; L$; \"]\"\n",
        |_| Ok(()),
    )
    .unwrap()
    .0;
    assert_eq!(
        output.trim(),
        "[a        b   c]",
        "TAB/SPC must pad the file, and must not write to the console"
    );
}

/// A fresh file starts at column 1, whatever the last one on that number left.
///
/// The column tracker is indexed by file number, so closing a file part-way
/// through a line and opening another on the same number handed the new file
/// the old column: TAB then thought it had to start a line, and wrote a
/// newline into a file nothing had written to yet.
#[test]
fn test_open_resets_the_column() {
    let output = compile_and_run_with_files(
        "OPEN \"c1.txt\" FOR OUTPUT AS #1\nPRINT #1, \"hello\";\nCLOSE #1\nOPEN \"c2.txt\" FOR OUTPUT AS #1\nPRINT #1, TAB(3); \"x\"\nCLOSE #1\nOPEN \"c2.txt\" FOR INPUT AS #1\nLINE INPUT #1, A$\nCLOSE #1\nPRINT \"[\"; A$; \"]\"\n",
        |_| Ok(()),
    )
    .unwrap()
    .0;
    assert_eq!(
        output.trim(),
        "[  x]",
        "TAB measured from the new file's start"
    );
}

/// Numbers too: a CRLF file must not leave a CR to derail the next field.
#[test]
fn test_crlf_numeric_fields() {
    let output = compile_and_run_with_files(
        "OPEN \"crlfn.txt\" FOR INPUT AS #1\nINPUT #1, A, B\nINPUT #1, C\nCLOSE #1\nPRINT A; \"/\"; B; \"/\"; C\n",
        |dir| fs::write(dir.join("crlfn.txt"), "10,20\r\n30\r\n").map_err(|e| e.to_string()),
    )
    .unwrap()
    .0;
    assert_eq!(output.trim(), "10 / 20 / 30");
}

// ---------------------------------------------------------------------------
// Random-access files
//
// The property that matters throughout is that a FIELD variable is a *window*
// onto the record buffer, not a copy of it: GET must change what every field
// variable reads, and LSET must change what the next PUT writes.

/// The round trip the feature exists for: write records, read them back by
/// number, and get the same values.
#[test]
fn test_random_record_round_trip() {
    let source = r#"
OPEN "r.dat" FOR RANDOM AS #1 LEN = 32
FIELD #1, 20 AS NM$, 4 AS AG$, 8 AS PAY$
LSET NM$ = "Alice"
LSET AG$ = MKI$(30)
LSET PAY$ = MKD$(50000.5)
PUT #1, 1
LSET NM$ = "Bob"
LSET AG$ = MKI$(45)
LSET PAY$ = MKD$(61234.25)
PUT #1, 2
GET #1, 1
PRINT RTRIM$(NM$); "/"; CVI(AG$); "/"; CVD(PAY$)
GET #1, 2
PRINT RTRIM$(NM$); "/"; CVI(AG$); "/"; CVD(PAY$)
CLOSE #1
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(
        output.lines().collect::<Vec<_>>(),
        vec!["Alice/ 30 / 50000.5 ", "Bob/ 45 / 61234.25 "]
    );
}

/// Records are fixed-length, so the file is exactly records x record length
/// however short the data written into them was.
#[test]
fn test_random_records_are_fixed_length() {
    let source = r#"
OPEN "f.dat" FOR RANDOM AS #1 LEN = 16
FIELD #1, 16 AS S$
LSET S$ = "a"
PUT #1, 1
LSET S$ = "bb"
PUT #1, 3
PRINT LOF(1)
CLOSE #1
"#;
    let (output, tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(output.trim(), "48", "three 16-byte records");
    let bytes = fs::read(tmp.path().join("f.dat")).unwrap();
    assert_eq!(bytes.len(), 48);
    assert_eq!(&bytes[0..16], b"a               ", "LSET pads with spaces");
    // Record 2 was never written, so it is whatever the file system supplies
    // for a hole; only its length is guaranteed.
    assert_eq!(&bytes[32..48], b"bb              ");
}

/// LSET is left-justified and RSET right-justified, and both truncate on the
/// right when the value is too wide for the field.
#[test]
fn test_lset_and_rset_justification() {
    let source = r#"
OPEN "j.dat" FOR RANDOM AS #1 LEN = 8
FIELD #1, 8 AS S$
LSET S$ = "ab"
PRINT "["; S$; "]"
RSET S$ = "ab"
PRINT "["; S$; "]"
LSET S$ = "0123456789"
PRINT "["; S$; "]"
RSET S$ = "0123456789"
PRINT "["; S$; "]"
CLOSE #1
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(
        output.lines().collect::<Vec<_>>(),
        vec!["[ab      ]", "[      ab]", "[01234567]", "[01234567]"]
    );
}

/// A FIELD variable keeps its width no matter what is written through it, and
/// the widths partition the record.
#[test]
fn test_field_widths_are_fixed() {
    let source = r#"
OPEN "w.dat" FOR RANDOM AS #1 LEN = 20
FIELD #1, 5 AS A$, 15 AS B$
PRINT LEN(A$); LEN(B$)
LSET A$ = "x"
PRINT LEN(A$)
CLOSE #1
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(output.lines().collect::<Vec<_>>(), vec![" 5  15 ", " 5 "]);
}

/// GET refreshes every field variable at once, because they all point into the
/// one buffer it overwrites.
#[test]
fn test_get_refreshes_all_fields() {
    let source = r#"
OPEN "g.dat" FOR RANDOM AS #1 LEN = 10
FIELD #1, 5 AS A$, 5 AS B$
LSET A$ = "one"
LSET B$ = "two"
PUT #1, 1
LSET A$ = "three"
LSET B$ = "four"
PUT #1, 2
GET #1, 1
PRINT RTRIM$(A$); "-"; RTRIM$(B$)
CLOSE #1
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(output.trim(), "one-two");
}

/// Rewriting one field of a record leaves the others as they were on disk,
/// which only works if GET loaded them into the buffer PUT writes back.
#[test]
fn test_record_update_preserves_other_fields() {
    let source = r#"
OPEN "u.dat" FOR RANDOM AS #1 LEN = 24
FIELD #1, 20 AS NM$, 4 AS AG$
LSET NM$ = "Alice"
LSET AG$ = MKI$(30)
PUT #1, 1
GET #1, 1
LSET NM$ = "Alicia"
PUT #1, 1
GET #1, 1
PRINT RTRIM$(NM$); "/"; CVI(AG$)
CLOSE #1
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(output.trim(), "Alicia/ 30");
}

/// GET and PUT without a record number walk forward one record at a time.
#[test]
fn test_get_put_default_to_the_next_record() {
    let source = r#"
OPEN "n.dat" FOR RANDOM AS #1 LEN = 4
FIELD #1, 4 AS S$
LSET S$ = "aa"
PUT #1, 1
LSET S$ = "bb"
PUT #1
LSET S$ = "cc"
PUT #1
GET #1, 1
PRINT RTRIM$(S$); LOC(1)
GET #1
PRINT RTRIM$(S$); LOC(1)
GET #1
PRINT RTRIM$(S$); LOC(1)
CLOSE #1
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(
        output.lines().collect::<Vec<_>>(),
        vec!["aa 1 ", "bb 2 ", "cc 3 "]
    );
}

/// The MK*$ / CV* pair must round-trip every type at its own width, including
/// the extremes that distinguish a signed 16-bit value from a wider one.
#[test]
fn test_mk_cv_round_trips() {
    let source = r#"
PRINT LEN(MKI$(1)); LEN(MKL$(1)); LEN(MKS$(1)); LEN(MKD$(1))
PRINT CVI(MKI$(32767)); CVI(MKI$(-32768)); CVI(MKI$(0))
PRINT CVL(MKL$(2147483647)); CVL(MKL$(-2147483647))
PRINT CVS(MKS$(3.5)); CVD(MKD$(2.25))
"#;
    let output = crate::common::compile_and_run(source).unwrap();
    assert_eq!(
        output.lines().collect::<Vec<_>>(),
        vec![
            " 2  4  4  8 ",
            " 32767 -32768  0 ",
            " 2147483647 -2147483647 ",
            " 3.5  2.25 "
        ]
    );
}

/// A string too short for the conversion is an error, not a value.
///
/// Zero-padding would be the tempting thing to do and the wrong one: two
/// characters read as a DOUBLE give a denormal near 1e-319, a number plausible
/// enough that nobody would ever spot it. GW-BASIC rejects the call, and so
/// does this.
#[test]
fn test_cv_of_a_short_string_is_rejected() {
    let run = compile_and_run_raw("PRINT CVD(\"ab\")\n", "").unwrap();
    assert_eq!(run.exit_code, Some(1));
    assert!(
        run.stderr.contains("Illegal function call"),
        "stderr was: {}",
        run.stderr
    );
}

/// Reading past the end of the file yields a zero-filled record, not the
/// previous one.
#[test]
fn test_get_past_end_of_file_is_zero_filled() {
    let source = r#"
OPEN "p.dat" FOR RANDOM AS #1 LEN = 8
FIELD #1, 8 AS S$
LSET S$ = "data"
PUT #1, 1
GET #1, 9
PRINT CVD(S$); LEN(S$)
CLOSE #1
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(output.trim(), "0  8");
}

/// LOCK and UNLOCK are accepted in all their forms and leave the file usable.
#[test]
fn test_lock_and_unlock() {
    let source = r#"
OPEN "l.dat" FOR RANDOM AS #1 LEN = 8
FIELD #1, 8 AS S$
LSET S$ = "locked"
PUT #1, 1
LOCK #1, 1
UNLOCK #1, 1
LOCK #1, 1 TO 4
UNLOCK #1, 1 TO 4
LOCK #1
UNLOCK #1
GET #1, 1
PRINT RTRIM$(S$)
CLOSE #1
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(output.trim(), "locked");
}

/// A random file with no LEN clause takes GW-BASIC's default record length.
#[test]
fn test_default_record_length_is_128() {
    let source = r#"
OPEN "d.dat" FOR RANDOM AS #1
FIELD #1, 4 AS S$
LSET S$ = "abcd"
PUT #1, 1
PRINT LOF(1)
CLOSE #1
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(output.trim(), "128");
}

/// A record length outside 1 to 32767 is "Illegal function call" (5), as in
/// GW-BASIC, raised before anything is opened or created.
///
/// It used to be fixed up silently: `LEN = 40000` was cut to 32767 and
/// `LEN = 0` (or anything below 1) became 128, so a program whose record
/// length was computed wrongly ran on with records of a size it never asked
/// for. The limits themselves still work.
#[test]
fn test_record_length_out_of_range_is_refused() {
    let (output, tmp) = compile_and_run_with_files(
        r#"
ON ERROR GOTO 100
N = 0
OPEN "a.dat" FOR RANDOM AS #1 LEN = 40000
OPEN "b.dat" FOR RANDOM AS #1 LEN = 0
OPEN "c.dat" FOR RANDOM AS #1 LEN = -1
OPEN "d.dat" FOR RANDOM AS #1 LEN = N
OPEN "e.dat" FOR RANDOM AS #1 LEN = 32767
OPEN "f.dat" FOR RANDOM AS #2 LEN = 1
FIELD #1, 32767 AS A$
FIELD #2, 1 AS B$
PRINT "opened"
END
100 PRINT ERR; ERL
RESUME NEXT
"#,
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec!["5  4", "5  5", "5  6", "5  7", "opened"]
    );
    for name in ["a.dat", "b.dat", "c.dat", "d.dat"] {
        assert!(!tmp.path().join(name).exists(), "{name} was created");
    }

    let run = compile_and_run_raw(
        "10 PRINT \"x\"\n20 OPEN \"r.dat\" FOR RANDOM AS #1 LEN = 40000\n",
        "",
    )
    .expect("should compile");
    assert_eq!(run.exit_code, Some(1));
    assert_eq!(run.stderr.trim(), "?Illegal function call in 20");
}

/// A record buffer starts blank, so a field never written is spaces rather
/// than whatever the allocator last held.
#[test]
fn test_unwritten_fields_are_blank() {
    let source = r#"
OPEN "b.dat" FOR RANDOM AS #1 LEN = 6
FIELD #1, 3 AS A$, 3 AS B$
LSET A$ = "xy"
PRINT "["; B$; "]"
CLOSE #1
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(output.trim(), "[   ]");
}

/// Ordinary assignment to a fielded variable rebinds it, which is exactly why
/// LSET exists; the record buffer is left alone.
#[test]
fn test_plain_assignment_severs_the_field_binding() {
    let source = r#"
OPEN "s.dat" FOR RANDOM AS #1 LEN = 8
FIELD #1, 8 AS S$
LSET S$ = "original"
S$ = "new"
PUT #1, 1
GET #1, 1
PRINT "["; S$; "]"
CLOSE #1
"#;
    let (output, tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    // After the plain assignment S$ is an ordinary string, so GET no longer
    // reaches it -- and the record still holds what LSET put there.
    assert_eq!(output.trim(), "[new]");
    assert_eq!(fs::read(tmp.path().join("s.dat")).unwrap(), b"original");
}

/// Writing to or reading from a file number that was never OPENed is a
/// diagnosed abort, not a crash.
///
/// `_rt_file_print_string`, `_rt_file_print_char`, `_rt_file_print_newline` and
/// `_rt_file_line_input` all loaded the handle-table slot and passed it
/// straight to fprintf or fgets, so a NULL took the process down with SIGSEGV
/// and exit 139 -- outside anything the harness can interpret.
/// `_rt_file_getc` and `_rt_file_eof` had guarded all along; these four had not.
#[test]
fn test_unopened_file_number_is_diagnosed_not_fatal() {
    for source in [
        "PRINT #3, \"x\"\n",
        "PRINT #3, 5\n",
        "PRINT #3,\n",
        "LINE INPUT #3, A$\n",
    ] {
        let run = crate::common::compile_and_run_raw(source, "").expect("should compile");
        assert_eq!(
            run.exit_code,
            Some(1),
            "{source:?} should abort cleanly, not crash; stderr: {}",
            run.stderr
        );
        assert!(
            run.stderr.contains("Bad file number"),
            "{source:?} should say why: {}",
            run.stderr
        );
    }
}

/// A failed OPEN is reported rather than stored.
///
/// The fopen/CreateFileA result was written into the handle table whatever it
/// was, so opening a file that is not there "succeeded", `EOF()` answered -1,
/// and the mistake surfaced later as a crash or as silence.
#[test]
fn test_open_of_a_missing_file_is_diagnosed() {
    let run = compile_and_run_raw(
        "OPEN \"definitely-not-here.txt\" FOR INPUT AS #1\nPRINT \"opened\"\n",
        "",
    )
    .expect("should compile");
    assert_eq!(run.exit_code, Some(1), "stderr: {}", run.stderr);
    assert!(run.stderr.contains("File not found"), "{}", run.stderr);
    assert!(
        !run.stdout.contains("opened"),
        "the OPEN must not appear to succeed: {}",
        run.stdout
    );
}

/// Re-using a file number that is still open is refused, not silently rebound.
#[test]
fn test_open_on_an_already_open_number_is_diagnosed() {
    let run = compile_and_run_raw(
        "OPEN \"a.txt\" FOR OUTPUT AS #1\nOPEN \"b.txt\" FOR OUTPUT AS #1\n",
        "",
    )
    .expect("should compile");
    assert_eq!(run.exit_code, Some(1), "stderr: {}", run.stderr);
    assert!(
        run.stderr.contains("File already open"),
        "stderr: {}",
        run.stderr
    );
}

/// Reading past the end is an error, not an endless supply of empty strings.
///
/// A loop that forgets its `EOF()` test used to run on quietly rather than say
/// what was wrong.
#[test]
fn test_reading_past_the_end_is_diagnosed() {
    let run = compile_and_run_raw(
        "OPEN \"e.txt\" FOR OUTPUT AS #1\nPRINT #1, \"one\"\nCLOSE #1\n\
         OPEN \"e.txt\" FOR INPUT AS #1\nLINE INPUT #1, A$\nLINE INPUT #1, B$\n",
        "",
    )
    .expect("should compile");
    assert_eq!(run.exit_code, Some(1), "stderr: {}", run.stderr);
    assert!(
        run.stderr.contains("Input past end of file"),
        "stderr: {}",
        run.stderr
    );
}

/// RESET closes every open file, as a bare CLOSE does: what was written is
/// flushed, and the numbers are free to open again.
#[test]
fn test_reset_closes_every_file() {
    let output = compile_and_run(
        r#"
OPEN "a.txt" FOR OUTPUT AS #1
OPEN "b.txt" FOR OUTPUT AS #2
PRINT #1, "one"
PRINT #2, "two"
RESET
OPEN "a.txt" FOR INPUT AS #1
OPEN "b.txt" FOR INPUT AS #2
LINE INPUT #1, A$
LINE INPUT #2, B$
PRINT A$; B$
"#,
    )
    .unwrap();
    assert_eq!(crate::common::lines(&output), vec!["onetwo"]);
}

/// Each way an OPEN can fail is GW-BASIC's own error, not always 53.
///
/// Every failure used to be "File not found": a missing directory, a read-only
/// file and an empty name all said the same, so a handler testing ERR could not
/// tell them apart. A RANDOM open that failed stored a null handle and carried
/// on without saying anything at all.
#[test]
fn test_open_failures_name_their_cause() {
    let (output, _dir) = compile_and_run_with_files(
        r#"
ON ERROR GOTO 100
OPEN "missing.txt" FOR INPUT AS #1
OPEN "no-such-dir/x.txt" FOR OUTPUT AS #1
OPEN "no-such-dir/x.txt" FOR APPEND AS #1
OPEN "no-such-dir/x.dat" FOR RANDOM AS #1 LEN = 16
OPEN "" FOR OUTPUT AS #1
OPEN "" FOR RANDOM AS #1
OPEN "readonly.txt" FOR OUTPUT AS #1
OPEN "ok.dat" FOR RANDOM AS #2 LEN = 8
OPEN "ok.dat" FOR RANDOM AS #2 LEN = 8
END
100 PRINT ERR
RESUME NEXT
"#,
        |dir| {
            let ro = dir.join("readonly.txt");
            fs::write(&ro, "x").map_err(|e| e.to_string())?;
            let mut perms = fs::metadata(&ro).map_err(|e| e.to_string())?.permissions();
            perms.set_readonly(true);
            fs::set_permissions(&ro, perms).map_err(|e| e.to_string())
        },
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec!["53", "76", "76", "76", "64", "64", "70", "55"]
    );
}

/// A directory is not a file: opening one is "Path/File access error" (75).
/// Linux says EISDIR. Windows says access denied, which was reported as
/// "Permission denied" (70) until the runtime learned to ask whether the name
/// is a directory; "File not found" was never right.
#[test]
fn test_open_of_a_directory_is_an_access_error() {
    let output = compile_and_run(
        "ON ERROR GOTO 100\nOPEN \".\" FOR OUTPUT AS #1\nOPEN \".\" FOR APPEND AS #1\nOPEN \".\" FOR RANDOM AS #1\nEND\n100 PRINT ERR\nRESUME NEXT\n",
    )
    .unwrap();
    assert_eq!(crate::common::lines(&output), vec!["75", "75", "75"]);
}

/// Opening a directory FOR INPUT is "Path/File access error" (75), as opening
/// it for output is.
///
/// On Linux fopen(".", "r") succeeds -- only the first read fails, with
/// EISDIR -- so the OPEN went through, EOF() answered -1 at once and the first
/// INPUT # said "Input past end of file", which names the wrong problem. The
/// number is left free, so a later OPEN on it works. Unix only: on Windows
/// opening a directory as a file already fails in the open itself.
#[cfg(unix)]
#[test]
fn test_open_of_a_directory_for_input_is_an_access_error() {
    let output = compile_and_run(
        r#"
ON ERROR GOTO 100
OPEN "." FOR INPUT AS #1
PRINT "after"
OPEN "f.txt" FOR OUTPUT AS #1
PRINT #1, 7
CLOSE #1
OPEN "f.txt" FOR INPUT AS #1
INPUT #1, A
PRINT A
END
100 PRINT ERR
RESUME NEXT
"#,
    )
    .unwrap();
    assert_eq!(crate::common::lines(&output), vec!["75", "after", "7"]);

    let run = compile_and_run_raw("10 PRINT \"x\"\n20 OPEN \".\" FOR INPUT AS #1\n", "")
        .expect("should compile");
    assert_eq!(run.exit_code, Some(1));
    assert_eq!(run.stderr.trim(), "?Path/File access error in 20");
}

/// Reading a file opened for writing, or writing one opened for reading, is
/// "Bad file mode" (54). PRINT # to an INPUT file used to be a write libc
/// quietly refused, and INPUT # from an unopened number read an empty field.
#[test]
fn test_file_mode_is_checked() {
    let output = compile_and_run(
        r#"
ON ERROR GOTO 100
OPEN "f.txt" FOR OUTPUT AS #1
PRINT #1, "data"
INPUT #1, A$
LINE INPUT #1, A$
CLOSE #1
OPEN "f.txt" FOR INPUT AS #1
PRINT #1, "more"
WRITE #1, 5
INPUT #2, B$
OPEN "r.dat" FOR RANDOM AS #3 LEN = 4
PRINT #3, "x"
LINE INPUT #1, C$
PRINT C$
END
100 PRINT ERR
RESUME NEXT
"#,
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec!["54", "54", "54", "54", "52", "54", "data"]
    );
}

/// The checks do not get in the way of a file used the way it was opened,
/// including a number reused for a different mode after CLOSE.
#[test]
fn test_reopened_numbers_take_their_new_mode() {
    let output = compile_and_run(
        r#"
OPEN "r.dat" FOR RANDOM AS #1 LEN = 4
CLOSE #1
OPEN "f.txt" FOR OUTPUT AS #1
PRINT #1, "one"
CLOSE #1
OPEN "f.txt" FOR APPEND AS #1
PRINT #1, "two"
CLOSE #1
OPEN "f.txt" FOR INPUT AS #1
LINE INPUT #1, A$
INPUT #1, B$
PRINT A$; B$
"#,
    )
    .unwrap();
    assert_eq!(crate::common::lines(&output), vec!["onetwo"]);
}

/// INPUT # reads a numeric field's text as VAL reads a string, and reading
/// when no field is left is "Input past end of file" (62). It used to read an
/// empty field there, so a loop that forgot EOF() read zeros forever.
///
/// Unlike VAL, a blank ends a number read by INPUT #, so "1 2" is two numbers
/// rather than 12: see `test_input_hash_number_ends_at_a_space`.
#[test]
fn test_input_hash_numbers_and_the_end_of_the_file() {
    let output = compile_and_run(
        r#"
OPEN "n.txt" FOR OUTPUT AS #1
PRINT #1, "1 2, &HFF, 1D2, abc"
CLOSE #1
ON ERROR GOTO 100
OPEN "n.txt" FOR INPUT AS #1
INPUT #1, A, B, C, D, E
PRINT A; B; C; D; E
INPUT #1, F
PRINT "not reached"
END
100 PRINT "ERR"; ERR
"#,
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec!["1  2  255  100  0", "ERR 62"]
    );
}

/// A number read by INPUT # ends at a space, as in GW-BASIC: leading blanks
/// and line breaks are skipped, and the number then runs to a space, a comma,
/// CR or LF.
///
/// The field used to run to the next comma or line end with blanks inside it
/// ignored, as VAL ignores them. So `PRINT #1, 1; 2; 3`, which writes
/// " 1  2  3 " on one line, read back as the single number 123 and the next
/// INPUT # failed with "Input past end of file"; " 1  2 -3 " read as 12.
#[test]
fn test_input_hash_number_ends_at_a_space() {
    let output = compile_and_run(
        r#"
OPEN "s.txt" FOR OUTPUT AS #1
PRINT #1, 1; 2; 3
PRINT #1, 1; 2; -3
PRINT #1, 4.5; -6
CLOSE #1
OPEN "s.txt" FOR INPUT AS #1
INPUT #1, A, B, C
PRINT A; B; C
INPUT #1, A, B, C
PRINT A; B; C
INPUT #1, A, B
PRINT A; B; EOF(1)
"#,
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec!["1  2  3", "1  2 -3", "4.5 -6 -1"]
    );
}

/// The other separators still separate, and a blank before one belongs to
/// it: " 4 , 5 " is two numbers, not 4 then an empty field. A line read after
/// the last number on a line is the next line, whether the file's lines end
/// in LF or CRLF, and a string item still runs to its comma, blanks and all.
#[test]
fn test_input_hash_number_separators() {
    let (output, _tmp) = compile_and_run_with_files(
        r#"
OPEN "sep.txt" FOR INPUT AS #1
INPUT #1, A, B, C
INPUT #1, D, E
INPUT #1, F
LINE INPUT #1, L$
INPUT #1, G
LINE INPUT #1, K$
INPUT #1, S$, H
LINE INPUT #1, M$
PRINT A; B; C; D; E; F
PRINT "["; L$; "]"; G; "["; K$; "]"; "["; S$; "]"; H; "["; M$; "]"
"#,
        |dir| {
            fs::write(
                dir.join("sep.txt"),
                "1,2\n3\n 4 , 5 \n6\r\nline\n7  \r\nnext\na b,8\nlast\n",
            )
            .map_err(|e| e.to_string())
        },
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec!["1  2  3  4  5  6", "[line] 7 [next][a b] 8 [last]"]
    );
}

/// GW-BASIC's `OPEN "O", #1, "f"`, and file numbers without `#` where it
/// allows them: OPEN's AS, CLOSE (with a list), FIELD, GET and PUT.
#[test]
fn test_legacy_open_and_file_numbers() {
    let source = r#"
OPEN "O", #1, "a.txt"
PRINT #1, 5
CLOSE 1
OPEN "a.txt" FOR INPUT AS 2
OPEN "a", 3, "b.txt"
INPUT #2, A
PRINT #3, A * 2
CLOSE #2, 3
OPEN "I", 1, "b.txt"
INPUT #1, B
PRINT A; B
CLOSE
OPEN "R", #1, "r.dat", 16
FIELD 1, 16 AS R$
LSET R$ = "rec"
PUT 1, 1
GET #1, 1
PRINT LEFT$(R$, 3)
CLOSE 1
"#;
    let (output, _tmp) = compile_and_run_with_files(source, |_| Ok(())).unwrap();
    assert_eq!(crate::common::lines(&output), &["5  10", "rec"]);

    let err = crate::common::compile_only("OPEN \"X\", 1, \"f\"\n").unwrap_err();
    assert!(err.contains("not O, I, A or R"), "got: {}", err.stderr);
}
