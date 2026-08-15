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
