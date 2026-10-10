//! Print statement tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_print_combined() {
    // Test print with strings, numbers, and multiple statements
    let output = compile_and_run(
        r#"
PRINT "Hello, World!"
PRINT 42
PRINT "A"
PRINT "B"
PRINT "C"
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "Hello, World!", "string");
    assert_eq!(lines[1], "42", "number");
    assert_eq!(lines[2], "A", "multi-a");
    assert_eq!(lines[3], "B", "multi-b");
    assert_eq!(lines[4], "C", "multi-c");
}

/// A value far too wide for its field must not run past the runtime's buffers.
///
/// `sprintf` of 1D300 into a "##.##" field wrote over 300 characters into a
/// 160-byte buffer, destroying every global laid out after it.
#[test]
fn test_using_overflow_does_not_corrupt_globals() {
    let output = compile_and_run(
        "A = 111\nB = 222\nC = 333\nPRINT USING \"##.##\"; 1D300\nPRINT A\nPRINT B\nPRINT C\n",
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines.len(), 4);
    // Too wide for the field, so GW-BASIC's '%' marker precedes the full value.
    assert!(lines[0].starts_with('%'), "got {}", lines[0]);
    assert_eq!(&lines[1..], ["111", "222", "333"]);
}

/// An absurdly wide field is clamped rather than overrunning the output buffer.
#[test]
fn test_using_absurd_field_width() {
    let format = "#".repeat(400);
    let source = format!("PRINT USING \"{format}\"; 7\nPRINT \"after\"\n");
    let output = compile_and_run(&source).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0].len(), 255);
    assert!(lines[0].ends_with('7'));
    assert_eq!(lines[1], "after");
}

/// PRINT USING used to print an uninitialized variable's garbage: USING was not
/// a keyword, so it lexed as an identifier and the format string was printed
/// verbatim after it.
#[test]
fn test_print_using_numeric() {
    let output = compile_and_run("PRINT USING \"##.##\"; 3.14159\nPRINT USING \"###\"; 42\nPRINT USING \"Total: ##.## units\"; 3.5\nPRINT USING \"## and ##\"; 1; 2\n").unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], " 3.14", "rounded and right-justified");
    assert_eq!(lines[1], " 42");
    assert_eq!(lines[2], "Total:  3.50 units");
    assert_eq!(lines[3], " 1 and  2");
}

/// Leading and trailing sign fields.
#[test]
fn test_print_using_signs() {
    let output = compile_and_run("PRINT USING \"+###\"; 42\nPRINT USING \"+###\"; -42\nPRINT USING \"###-\"; -42\nPRINT USING \"###-\"; 42\nPRINT USING \"####\"; -42\n").unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], " +42", "+ shows a sign for positives");
    assert_eq!(lines[1], " -42");
    assert_eq!(lines[2], " 42-", "trailing sign");
    assert_eq!(lines[3], "  42", "- shows nothing for positives");
    assert_eq!(lines[4], " -42");
}

/// Comma grouping, currency and fill characters.
#[test]
fn test_print_using_decorations() {
    let output = compile_and_run("PRINT USING \"#######,\"; 1234567\nPRINT USING \"$$###.##\"; 12.5\nPRINT USING \"**####\"; 42\nPRINT USING \"**$###.##\"; 12.5\n").unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "1,234,567", "field widens for the separators");
    assert_eq!(lines[1], "  $12.50", "currency floats against the digits");
    assert_eq!(lines[2], "****42", "asterisk fill");
    assert_eq!(lines[3], "***$12.50");
}

/// Exponential form, the overflow marker, and `_` escaping.
#[test]
fn test_print_using_misc_numeric() {
    let output = compile_and_run(
        "PRINT USING \"##.##^^^^\"; 1234.5\nPRINT USING \"##\"; 12345\nPRINT USING \"_####\"; 42\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], " 1.23E+03");
    assert_eq!(lines[1], "%12345", "too wide: printed in full after %");
    assert_eq!(lines[2], "# 42", "_ escapes the next character");
}

/// String fields: whole, first character, and fixed width.
#[test]
fn test_print_using_strings() {
    let output = compile_and_run("PRINT USING \"[&]\"; \"abcdefg\"\nPRINT USING \"[!]\"; \"abcdefg\"\nPRINT USING \"[\\   \\]\"; \"abcdefg\"\nPRINT USING \"[\\   \\]\"; \"ab\"\n").unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "[abcdefg]", "& takes the whole string");
    assert_eq!(lines[1], "[a]", "! takes the first character");
    assert_eq!(lines[2], "[abcde]", "fixed width truncates");
    assert_eq!(lines[3], "[ab   ]", "fixed width pads");
}

/// When values remain after the format is exhausted, the format restarts.
#[test]
fn test_print_using_format_repeats() {
    let output = compile_and_run("PRINT USING \"##;\"; 1; 2; 3\n").unwrap();
    assert_eq!(output.trim(), "1; 2; 3;");
}

/// TAB() moves to a column and SPC() emits spaces. Both were unimplemented and
/// failed at link time.
#[test]
fn test_tab_and_spc() {
    let output = compile_and_run(
        "PRINT TAB(5); \"x\"\nPRINT \"a\"; SPC(3); \"b\"\nPRINT \"ab\"; TAB(10); \"c\"\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "    x", "TAB(5) puts x in column 5");
    assert_eq!(lines[1], "a   b");
    assert_eq!(
        lines[2], "ab       c",
        "TAB accounts for text already printed"
    );
}

/// `LOCATE` positions the cursor, and `COLOR` sets the colours.
///
/// Both are written as ANSI escapes, which is the model `CLS` already uses on
/// both platforms. The test reads the escape bytes out of stdout rather than
/// looking at a terminal.
#[test]
fn test_locate_and_color_emit_escapes() {
    let out = crate::common::compile_and_run_raw(
        "LOCATE 5, 10\nPRINT \"x\";\nCOLOR 14, 1\nPRINT \"y\";\n",
        "",
    )
    .expect("should compile");
    out.assert_ran_to_completion("LOCATE then COLOR");
    assert!(
        out.stdout.contains("\u{1b}[5;10H"),
        "LOCATE 5,10 should home the cursor there: {:?}",
        out.stdout
    );
    assert!(
        out.stdout.contains('x') && out.stdout.contains('y'),
        "the text still prints: {:?}",
        out.stdout
    );
    assert!(
        out.stdout.contains("\u{1b}[") && out.stdout.contains('m'),
        "COLOR should emit an SGR sequence: {:?}",
        out.stdout
    );
}

/// `LOCATE` with only a row leaves the column alone, as GW-BASIC does: VPA
/// moves the row and nothing else.
#[test]
fn test_locate_row_only() {
    let out = crate::common::compile_and_run_raw("LOCATE 7\n", "").expect("should compile");
    out.assert_ran_to_completion("LOCATE with a row only");
    assert_eq!(out.stdout, "\u{1b}[7d", "row given, column preserved");
}

/// And with only a column, the row stays: CHA moves the column alone. It used
/// to go to row 1.
#[test]
fn test_locate_column_only() {
    let out = crate::common::compile_and_run_raw("LOCATE , 10\nPRINT POS(0)\n", "")
        .expect("should compile");
    out.assert_ran_to_completion("LOCATE with a column only");
    assert!(
        out.stdout.starts_with("\u{1b}[10G"),
        "column given, row preserved: {:?}",
        out.stdout
    );
    assert!(
        out.stdout.trim_end().ends_with("10"),
        "POS follows the cursor: {:?}",
        out.stdout
    );
}

/// COLOR sets only what it is given. A foreground also states blink, which
/// 16-31 turn on; an omitted background is left alone instead of turning black.
/// And the hues are GW-BASIC's: 1 is blue and 4 red, where ANSI has them the
/// other way round -- COLOR 1 used to print red.
#[test]
fn test_color_sets_only_what_it_is_given() {
    for (source, expected) in [
        ("COLOR 14\n", "\u{1b}[25;93m"),
        ("COLOR , 1\n", "\u{1b}[44m"),
        ("COLOR 14, 1\n", "\u{1b}[25;93;44m"),
        ("COLOR 7, 9\n", "\u{1b}[25;37;104m"),
        ("COLOR 4, 6\n", "\u{1b}[25;31;43m"),
        ("COLOR 30\n", "\u{1b}[5;93m"),
        ("COLOR 16, 0\n", "\u{1b}[5;30;40m"),
    ] {
        let out = crate::common::compile_and_run_raw(source, "").expect("should compile");
        out.assert_ran_to_completion(source);
        assert_eq!(out.stdout, expected, "{source:?}");
    }
}

/// Values a screen cannot show are Illegal function call, as in GW-BASIC.
#[test]
fn test_locate_and_color_arguments_are_range_checked() {
    for source in [
        "COLOR 32\n",
        "COLOR -1\n",
        "COLOR , 16\n",
        "LOCATE 0\n",
        "LOCATE , 0\n",
    ] {
        let run = crate::common::compile_and_run_raw(source, "").expect("should compile");
        assert_eq!(run.exit_code, Some(1), "{source:?}: {}", run.stderr);
        assert!(
            run.stderr.contains("Illegal function call"),
            "{source:?}: {}",
            run.stderr
        );
    }
}

/// `POS(0)` reports the column the next character will go to, counting from 1.
#[test]
fn test_pos_reports_the_column() {
    let output = compile_and_run(
        r#"
PRINT POS(0)
PRINT "abc";
PRINT POS(0)
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "1", "a fresh line starts at column 1");
    assert!(
        lines[1].ends_with('4'),
        "after \"abc\" the column is 4: {lines:?}"
    );
}

/// `CLS` puts the cursor at home, so the column tracker must agree.
///
/// It did not: after clearing, `TAB` still believed the column it had before
/// and emitted a spurious newline to reach a column already passed.
#[test]
fn test_cls_resets_the_column() {
    let out = crate::common::compile_and_run_raw(
        "PRINT \"0123456789012345678901234567890123456789\";\nCLS\nPRINT TAB(5); \"X\"\n",
        "",
    )
    .expect("should compile");
    // Before this line the test could not fail on a crash: a program that died
    // in CLS left nothing after the escape, which is exactly what the
    // assertion below wants to see.
    out.assert_ran_to_completion("PRINT then CLS then TAB");
    let after_cls = out.stdout.rsplit("\u{1b}[H").next().unwrap_or("");
    assert!(
        !after_cls.starts_with('\n'),
        "TAB after CLS must not wrap to a new line: {:?}",
        out.stdout
    );
}

/// Every console statement's program runs to completion.
///
/// Blunt on purpose. These helpers are written twice, and the Win64 half is
/// the one no developer runs -- CI is the only place it executes at all. The
/// tests above read what each statement *wrote*, which a crash can satisfy by
/// writing nothing; this one only asks whether the program survived, which a
/// crash cannot.
#[test]
fn test_console_statements_run_to_completion() {
    for (what, source) in [
        ("CLS", "CLS\n"),
        ("CLS after PRINT", "PRINT \"x\"\nCLS\n"),
        ("CLS twice", "CLS\nCLS\n"),
        ("LOCATE", "LOCATE 2, 5\n"),
        ("LOCATE row only", "LOCATE 3\n"),
        ("LOCATE column only", "LOCATE , 8\n"),
        ("COLOR", "COLOR 14, 1\n"),
        ("POS", "PRINT POS(0)\n"),
        ("BEEP", "BEEP\n"),
        ("TIMER", "PRINT TIMER\n"),
        ("RANDOMIZE", "RANDOMIZE 42\n"),
        ("RANDOMIZE TIMER", "RANDOMIZE TIMER\n"),
        ("RND", "PRINT RND\n"),
        ("DATE$ and TIME$", "PRINT DATE$\nPRINT TIME$\n"),
        ("FRE", "PRINT FRE(0)\n"),
        (
            "the lot together",
            "CLS\nCOLOR 14, 1\nLOCATE 2, 5\nPRINT \"x\"; POS(0)\n",
        ),
    ] {
        let run = crate::common::compile_and_run_raw(source, "")
            .unwrap_or_else(|e| panic!("{what} should compile: {e}"));
        run.assert_ran_to_completion(what);
    }
}

/// `LOCATE` also sets the column the tracker believes, for the same reason.
#[test]
fn test_locate_sets_the_column() {
    let output = compile_and_run("LOCATE 3, 12\nPRINT POS(0)\n").unwrap();
    // The escape sequence LOCATE wrote precedes the number on the same line.
    assert!(
        output.trim().ends_with("12"),
        "POS should follow LOCATE: {output:?}"
    );
}

// ---------------------------------------------------------------------------
// GW-BASIC number formatting
//
// A number carries its own spacing: a leading blank where the sign would go if
// it is not negative, and a trailing blank always. Values below one drop the
// leading zero. Without these a listing's output runs together -- `PRINT 1;2`
// gave "12" -- and nothing lines up.

/// Every number is written with a sign position and a trailing space.
#[test]
fn test_numbers_carry_their_own_spacing() {
    let out = crate::common::compile_and_run_raw(
        "PRINT 1\nPRINT -1\nPRINT 1; 2; 3\nPRINT 1; -2; 3\n",
        "",
    )
    .expect("should compile");
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(
        lines[0], " 1 ",
        "a positive number is blank-signed and blank-followed"
    );
    assert_eq!(lines[1], "-1 ", "a negative one uses the sign position");
    assert_eq!(
        lines[2], " 1  2  3 ",
        "two blanks between, one on each side"
    );
    assert_eq!(
        lines[3], " 1 -2  3 ",
        "the minus takes the leading blank's place"
    );
}

/// A value below one drops the leading zero, as GW-BASIC does.
#[test]
fn test_no_leading_zero_below_one() {
    let out = crate::common::compile_and_run_raw("PRINT 0.5\nPRINT -0.5\nPRINT 0.125\n", "")
        .expect("should compile");
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines[0], " .5 ");
    assert_eq!(lines[1], "-.5 ");
    assert_eq!(lines[2], " .125 ");
}

/// Exponents use GW-BASIC's letters: D for double, E for single.
#[test]
fn test_exponent_letter() {
    let out = crate::common::compile_and_run_raw("PRINT 1E20\nA! = 1.5E-10\nPRINT A!\n", "")
        .expect("should compile");
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert!(lines[0].contains('D'), "a double uses D: {:?}", lines[0]);
    assert!(
        !lines[0].contains('e'),
        "never C's lowercase e: {:?}",
        lines[0]
    );
    assert!(lines[1].contains('E'), "a single uses E: {:?}", lines[1]);
}

/// `STR$` is what PRINT writes, without the trailing blank -- so it keeps the
/// leading one. `MID$(STR$(N), 2)` is the idiom that depends on it.
#[test]
fn test_str_dollar_keeps_the_sign_position() {
    let out = crate::common::compile_and_run_raw(
        "PRINT \"[\"; STR$(5); \"]\"\nPRINT \"[\"; STR$(-5); \"]\"\nPRINT \"[\"; MID$(STR$(42), 2); \"]\"\n",
        "",
    )
    .expect("should compile");
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines[0], "[ 5]");
    assert_eq!(lines[1], "[-5]");
    assert_eq!(
        lines[2], "[42]",
        "MID$(...,2) strips the blank, not a digit"
    );
}

/// A comma moves to the next 14-column print zone, padding with blanks.
#[test]
fn test_comma_print_zones() {
    let out = crate::common::compile_and_run_raw("PRINT 1, 2\nPRINT \"ab\", \"cd\"\n", "")
        .expect("should compile");
    let lines: Vec<&str> = out.stdout.lines().collect();
    // Zone two begins at column 15, and the second number's own sign blank
    // sits there -- so its digit lands at 16, as in GW-BASIC.
    assert_eq!(lines[0], " 1             2 ");
    assert_eq!(lines[1], "ab            cd");
    assert!(!out.stdout.contains('\t'), "zones are blanks, never a tab");
}

/// `WRITE` is not `PRINT`: its numbers carry no padding at all.
#[test]
fn test_write_numbers_are_unpadded() {
    let out =
        crate::common::compile_and_run_raw("WRITE 1, -2, \"x\"\n", "").expect("should compile");
    assert_eq!(out.stdout.lines().next().unwrap(), "1,-2,\"x\"");
}

/// `PRINT USING` owns its own layout and gains nothing.
#[test]
fn test_print_using_is_unaffected() {
    let out = crate::common::compile_and_run_raw("PRINT USING \"###.##\"; 3.5\n", "")
        .expect("should compile");
    assert_eq!(out.stdout.lines().next().unwrap(), "  3.50");
}

/// Numbers print as GW-BASIC prints them: rounded to 7 significant digits for
/// a SINGLE and 16 for a DOUBLE, in fixed point when that fits in so many
/// digits and with an E (SINGLE) or D (DOUBLE) exponent otherwise.
///
/// The thresholds are QuickBASIC's documented ones: 1.1E-6 shows as .0000011,
/// 1.1E-7 as 1.1E-07. This used to print the shortest decimal that read back
/// exactly -- 17 digits for a DOUBLE, `.1 + .2` as .30000000000000004 -- and
/// any whole number in full, whatever its type.
#[test]
fn test_numbers_print_with_gw_basic_digits() {
    let output = compile_and_run(
        r#"
PRINT .1 + .2
X! = 1.1E-6: PRINT X!
X! = 1.1E-7: PRINT X!
X! = 1234567: PRINT X!
X! = 12345678: PRINT X!
X! = 1E7: PRINT X!
X! = 2 / 3: PRINT X!
PRINT 1E15
PRINT 1E16
PRINT 1.5E-300
PRINT 1.1E-15
PRINT 1.1E-16
PRINT 9.9999999999999999
X! = -1E-10: PRINT X!
PRINT 1E100
"#,
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec![
            ".3",
            ".0000011",
            "1.1E-07",
            "1234567",
            "1.234568E+07",
            "1E+07",
            ".6666667",
            "1000000000000000",
            "1D+16",
            "1.5D-300",
            ".0000000000000011",
            "1.1D-16",
            "10",
            "-1E-10",
            "1D+100"
        ]
    );
}

/// WIDTH sets the line length output wraps at, as in GW-BASIC: long text is
/// cut and continued, a number that does not fit moves to the next line whole,
/// and a comma past the last full zone starts a new line. 255 means no limit,
/// which is also where every program and every file starts.
#[test]
fn test_width_wraps_output() {
    let output = compile_and_run(
        r#"
WIDTH 10
PRINT "abcdefghijklmnopqrstuvwxyz"
PRINT "12345"; 678
PRINT "12345"; 6789
PRINT "1234567890"
PRINT "a", "b"
WIDTH 30
PRINT "a", "b", "c"
WIDTH 255
PRINT STRING$(300, "x")
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines,
        vec![
            "abcdefghij",
            "klmnopqrst",
            "uvwxyz",
            "12345 678 ",
            "12345",
            " 6789 ",
            "1234567890",
            "a",
            "b",
            "a             b",
            "c",
            &"x".repeat(300),
        ]
    );
}

/// WIDTH applies to a file by number, and OPEN starts a file without one.
#[test]
fn test_width_on_a_file() {
    let output = compile_and_run(
        r#"
OPEN "w.txt" FOR OUTPUT AS #1
WIDTH #1, 4
PRINT #1, "abcdefgh"
CLOSE #1
OPEN "w.txt" FOR APPEND AS #1
PRINT #1, "abcdefgh"
CLOSE #1
OPEN "w.txt" FOR INPUT AS #1
WHILE NOT EOF(1)
LINE INPUT #1, A$
PRINT A$
WEND
"#,
    )
    .unwrap();
    assert_eq!(
        crate::common::lines(&output),
        vec!["abcd", "efgh", "abcdefgh"]
    );
}

/// A width outside 1-255 is Illegal function call; a file not open is Bad file
/// number.
#[test]
fn test_width_arguments_are_checked() {
    for (source, msg) in [
        ("WIDTH 0\n", "Illegal function call"),
        ("WIDTH 256\n", "Illegal function call"),
        ("WIDTH #2, 40\n", "Bad file number"),
    ] {
        let run = crate::common::compile_and_run_raw(source, "").unwrap();
        assert_eq!(run.exit_code, Some(1), "{source:?}: {}", run.stderr);
        assert!(run.stderr.contains(msg), "{source:?}: {}", run.stderr);
    }
}

/// The format of PRINT USING may be any string expression, read as the
/// statement runs, and `PRINT #n, USING` writes to a file. Both used to be
/// refused, because the format was parsed when compiling.
#[test]
fn test_print_using_with_a_computed_format_and_a_file() {
    let output = compile_and_run(
        r###"
F$ = "##.##"
PRINT USING F$; 3.14159
PRINT USING "[" + "&" + "]"; "ok"
FOR W = 2 TO 4
PRINT USING STRING$(W, "#"); 7
NEXT W
OPEN "u.txt" FOR OUTPUT AS #1
PRINT #1, USING "Total: $$###.##"; 12.5
PRINT #1, USING F$; 1; 2;
PRINT #1, "!"
CLOSE #1
OPEN "u.txt" FOR INPUT AS #1
LINE INPUT #1, A$
LINE INPUT #1, B$
PRINT A$
PRINT B$
"###,
    )
    .unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines,
        vec![
            " 3.14",
            "[ok]",
            " 7",
            "  7",
            "   7",
            "Total:   $12.50",
            " 1.00 2.00!"
        ]
    );
}

/// The text after the last value stops at the next field, as GW-BASIC's does,
/// and widths and digits are clamped to what the runtime lays out.
#[test]
fn test_print_using_stops_at_the_next_unused_field() {
    let wide = "#".repeat(600);
    let output = compile_and_run(&format!(
        "PRINT USING \"A=## B=## C\"; 5\nPRINT USING \"{wide}\"; 1\nPRINT USING \"#.{}\"; 1\n",
        "#".repeat(200)
    ))
    .unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "A= 5 B=");
    assert_eq!(lines[1].len(), 255, "a width is at most 255");
    assert_eq!(
        lines[2],
        format!("1.{}", "0".repeat(40)),
        "at most 40 decimals"
    );
}

/// A value of the wrong kind for its field is Type mismatch (13), and a format
/// with no field to put a value in is Illegal function call, as in GW-BASIC.
#[test]
fn test_print_using_errors() {
    for (source, msg) in [
        ("PRINT USING \"##\"; \"text\"\n", "Type mismatch"),
        ("PRINT USING \"&\"; 5\n", "Type mismatch"),
        ("PRINT USING \"no fields\"; 5\n", "Illegal function call"),
    ] {
        let run = crate::common::compile_and_run_raw(source, "").unwrap();
        assert_eq!(run.exit_code, Some(1), "{source:?}: {}", run.stderr);
        assert!(run.stderr.contains(msg), "{source:?}: {}", run.stderr);
    }
}
