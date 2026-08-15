//! File I/O tests

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run_with_files;
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
        assert_eq!(lines, vec!["Hello, File!", "42"]);
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
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "one");
    assert_eq!(lines[1], "two");
    assert_eq!(lines[2], "three");
    assert_eq!(lines[3], "lines:3");

    // Text files carry the host's line terminator, so LOF counts CRLF on
    // Windows and LF elsewhere: 11 characters of text plus three terminators.
    let terminator = if cfg!(windows) { 2 } else { 1 };
    assert_eq!(
        lines[4],
        format!("bytes:{}", 11 + 3 * terminator),
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
    assert_eq!(output.trim(), "10/20/30");
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
    assert_eq!(output.trim(), "1020[ab]");
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
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "[one][two][three]");
    assert_eq!(lines[1], "335", "no line may keep its carriage return");
}

/// A SINGLE written to a file reads back the way the console prints it.
///
/// `gen_print_expr` picks `_rt_print_single` for a SINGLE so that only the ~7
/// digits it carries are shown; `gen_print_expr_to_file`, a copy that never
/// got that fix, always used the double helper, so `PRINT #1, A!` wrote
/// 0.3333333432674408 where `PRINT A!` gives 0.33333334.
#[test]
#[ignore = "unified in phase 2"]
fn test_print_file_single_matches_console() {
    let output = compile_and_run_with_files(
        "A! = 1 / 3\nPRINT A!\nOPEN \"s.txt\" FOR OUTPUT AS #1\nPRINT #1, A!\nCLOSE #1\nOPEN \"s.txt\" FOR INPUT AS #1\nLINE INPUT #1, L$\nCLOSE #1\nPRINT L$\n",
        |_| Ok(()),
    )
    .unwrap()
    .0;
    let lines: Vec<&str> = output.trim().lines().collect();
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
#[ignore = "unified in phase 2"]
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

/// Numbers too: a CRLF file must not leave a CR to derail the next field.
#[test]
fn test_crlf_numeric_fields() {
    let output = compile_and_run_with_files(
        "OPEN \"crlfn.txt\" FOR INPUT AS #1\nINPUT #1, A, B\nINPUT #1, C\nCLOSE #1\nPRINT A; \"/\"; B; \"/\"; C\n",
        |dir| fs::write(dir.join("crlfn.txt"), "10,20\r\n30\r\n").map_err(|e| e.to_string()),
    )
    .unwrap()
    .0;
    assert_eq!(output.trim(), "10/20/30");
}
