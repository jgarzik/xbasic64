//! String function tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_string_functions() {
    // Test LEN, LEFT$, RIGHT$, MID$, CHR$, ASC, VAL, STR$, INSTR
    let output = compile_and_run(
        r#"
PRINT LEN("Hello")
PRINT LEFT$("Hello", 2)
PRINT RIGHT$("Hello", 2)
PRINT MID$("Hello", 2, 3)
PRINT CHR$(65)
PRINT ASC("A")
X = VAL("42"): PRINT X + 8
PRINT STR$(100)
PRINT INSTR("Hello World", "World")
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "5", "len");
    assert_eq!(lines[1], "He", "left$");
    assert_eq!(lines[2], "lo", "right$");
    assert_eq!(lines[3], "ell", "mid$");
    assert_eq!(lines[4], "A", "chr$");
    assert_eq!(lines[5], "65", "asc");
    assert_eq!(lines[6], "50", "val");
    assert_eq!(lines[7], "100", "str$");
    assert_eq!(lines[8], "7", "instr");
}

#[test]
fn test_nested_string_calls() {
    // Test LEFT$, RIGHT$, MID$ with nested function calls
    let output = compile_and_run(
        r#"
FUNCTION GetStart()
    GetStart = 2
END FUNCTION

FUNCTION GetLen()
    GetLen = 3
END FUNCTION

A$ = "HELLO"
B$ = "WORLD"
PRINT LEFT$(A$ + B$, LEN(A$))
PRINT RIGHT$(A$ + B$, LEN(B$))
PRINT MID$("ABCDEF", GetStart(), GetLen())
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "HELLO", "left$ with len()");
    assert_eq!(lines[1], "WORLD", "right$ with len()");
    assert_eq!(lines[2], "BCD", "mid$ with functions");
}

#[test]
fn test_string_concat_multiple() {
    // Test string concatenation with multiple operands
    let output = compile_and_run(
        r#"
A$ = "Hello"
B$ = " "
C$ = "World"
PRINT A$ + B$ + C$
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "Hello World");
}

/// Every relational operator on strings. These used to compile to a
/// floating-point compare of registers that never held the operands, so `<`,
/// `>`, `<=` and `>=` always yielded false and `=` always yielded true.
#[test]
fn test_string_comparison_operators() {
    let output = compile_and_run(
        r#"
PRINT ("abc" < "abd")
PRINT ("abd" < "abc")
PRINT ("abc" > "abd")
PRINT ("abd" > "abc")
PRINT ("abc" = "abc")
PRINT ("abc" = "abd")
PRINT ("abc" <> "abd")
PRINT ("abc" <> "abc")
PRINT ("abc" <= "abc")
PRINT ("abc" >= "abc")
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        vec!["-1", "0", "0", "-1", "-1", "0", "-1", "0", "-1", "-1"],
        "BASIC booleans are -1 for true, 0 for false"
    );
}

/// A prefix sorts before the longer string, comparison is by byte value (so
/// case-sensitive), and the empty/unassigned string compares as empty.
#[test]
fn test_string_comparison_edge_cases() {
    let output = compile_and_run(
        r#"
IF "ab" < "abc" THEN PRINT "prefix-lt" ELSE PRINT "prefix-bad"
IF "A" < "a" THEN PRINT "case-lt" ELSE PRINT "case-bad"
E$ = ""
IF E$ < "a" THEN PRINT "empty-lt" ELSE PRINT "empty-bad"
IF Z$ = "" THEN PRINT "unassigned-empty" ELSE PRINT "unassigned-bad"
IF "ab" + "c" = "abc" THEN PRINT "concat-eq" ELSE PRINT "concat-bad"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        vec![
            "prefix-lt",
            "case-lt",
            "empty-lt",
            "unassigned-empty",
            "concat-eq"
        ]
    );
}

/// String comparison in anger: sorting an array, which is what silently
/// produced wrong answers before.
#[test]
fn test_string_sort() {
    let output = compile_and_run(
        r#"
DIM S$(4)
S$(0) = "pear"
S$(1) = "apple"
S$(2) = "fig"
S$(3) = "cherry"
S$(4) = "banana"
FOR I = 0 TO 3
FOR J = 0 TO 3 - I
IF S$(J) > S$(J+1) THEN
T$ = S$(J)
S$(J) = S$(J+1)
S$(J+1) = T$
END IF
NEXT J
NEXT I
FOR I = 0 TO 4
PRINT S$(I)
NEXT I
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        vec!["apple", "banana", "cherry", "fig", "pear"],
        "bubble sort by string comparison"
    );
}
