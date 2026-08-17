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
    let lines: Vec<&str> = output.trim().lines().collect();
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
    let lines: Vec<&str> = output.trim().lines().collect();
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

/// `LOCATE` with only a row leaves the column alone, as GW-BASIC does.
#[test]
fn test_locate_row_only() {
    let out = crate::common::compile_and_run_raw("LOCATE 7\n", "").expect("should compile");
    out.assert_ran_to_completion("LOCATE with a row only");
    assert!(
        out.stdout.contains("\u{1b}[7;"),
        "row given, column preserved: {:?}",
        out.stdout
    );
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
    let lines: Vec<&str> = output.trim().lines().collect();
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
