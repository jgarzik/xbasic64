//! String function tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

/// Assignment must not leave two variables sharing bytes.
///
/// A string assignment copies its value, because most string expressions hand
/// back a pointer into something that outlives the statement. Expressions that
/// have already allocated skip that copy -- and this is the property that has
/// to survive the skip: whatever `MID$ =` edits, it edits alone.
#[test]
fn test_string_assignment_never_aliases() {
    let output = compile_and_run(
        r#"
A$ = "hello"
B$ = A$
MID$(B$, 1, 1) = "J"
PRINT A$
PRINT B$
C$ = LEFT$(A$, 3)
MID$(C$, 1, 1) = "Z"
PRINT A$
PRINT C$
G$ = MID$(A$, 2, 3)
MID$(G$, 1, 1) = "X"
PRINT A$
PRINT G$
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "hello", "the source of a plain assignment");
    assert_eq!(lines[1], "Jello");
    assert_eq!(lines[2], "hello", "the source of a LEFT$ slice");
    assert_eq!(lines[3], "Zel");
    assert_eq!(lines[4], "hello", "the source of a MID$ slice");
    assert_eq!(lines[5], "Xll");
}

/// ...including for the expressions that own their result and are not copied.
///
/// A concatenation or a UCASE$ has already allocated, so assigning it does not
/// copy again. The variable still owns the buffer alone, so assigning *from*
/// it copies as usual.
#[test]
fn test_allocating_expressions_still_own_their_result() {
    let output = compile_and_run(
        r#"
A$ = "hello"
D$ = A$ + "!"
E$ = D$
MID$(E$, 1, 1) = "Q"
PRINT D$
PRINT E$
F$ = UCASE$(A$)
MID$(F$, 1, 1) = "j"
PRINT A$
PRINT F$
H$ = SPACE$(3) + "x"
PRINT LEN(H$)
K$ = STRING$(2, 65) + CHR$(66)
PRINT K$
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "hello!", "a concatenation is not aliased by E$");
    assert_eq!(lines[1], "Qello!");
    assert_eq!(
        lines[2], "hello",
        "UCASE$ did not write through to its source"
    );
    assert_eq!(lines[3], "jELLO");
    assert_eq!(lines[4], "4");
    assert_eq!(lines[5], "AAB");
}

/// The same for array elements and for a string built in a loop.
#[test]
fn test_string_arrays_and_accumulation_do_not_alias() {
    let output = compile_and_run(
        r#"
DIM A$(3)
S$ = ""
FOR I = 1 TO 3
  S$ = S$ + "ab"
  A$(I) = S$
NEXT I
MID$(S$, 1, 1) = "Z"
PRINT S$
PRINT A$(1)
PRINT A$(2)
PRINT A$(3)
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines[0], "Zbabab", "the accumulator, first byte edited");
    assert_eq!(lines[1], "ab");
    assert_eq!(lines[2], "abab");
    assert_eq!(lines[3], "ababab", "unaffected by the edit to S$");
}

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
    let lines = crate::common::lines(&output);
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
    let lines = crate::common::lines(&output);
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
    let lines = crate::common::lines(&output);
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
    let lines = crate::common::lines(&output);
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
    let lines = crate::common::lines(&output);
    assert_eq!(
        lines,
        vec!["apple", "banana", "cherry", "fig", "pear"],
        "bubble sort by string comparison"
    );
}

/// String builtins that used to abort the compiler: every unrecognized
/// $-suffixed call was assumed to be an array access.
#[test]
fn test_string_builders() {
    let output = compile_and_run(
        "PRINT \"[\"; SPACE$(3); \"]\"\nPRINT STRING$(5, 42)\nPRINT STRING$(3, \"x\")\nPRINT LEN(SPACE$(4))\n",
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["[   ]", "*****", "xxx", "4"]);
}

/// Trimming and case conversion.
#[test]
fn test_string_trim_and_case() {
    let output = compile_and_run(
        "PRINT \"[\"; LTRIM$(\"   abc\"); \"]\"\nPRINT \"[\"; RTRIM$(\"abc   \"); \"]\"\nPRINT UCASE$(\"Hello, World!\")\nPRINT LCASE$(\"Hello, World!\")\nA$ = \"  Mixed  \"\nPRINT \"[\" + LTRIM$(RTRIM$(A$)) + \"]\"\n",
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(
        lines,
        vec![
            "[abc]",
            "[abc]",
            "HELLO, WORLD!",
            "hello, world!",
            "[Mixed]"
        ]
    );
}

/// UCASE$ must copy rather than modify in place: the source may be a shared
/// .data literal.
#[test]
fn test_case_conversion_does_not_mutate_source() {
    let output =
        compile_and_run("A$ = \"abc\"\nB$ = UCASE$(A$)\nPRINT A$\nPRINT B$\nPRINT \"abc\"\n")
            .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["abc", "ABC", "abc"]);
}

/// Radix conversions.
#[test]
fn test_hex_and_oct() {
    let output = compile_and_run("PRINT HEX$(255)\nPRINT OCT$(15)\nPRINT HEX$(16)\n").unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["FF", "17", "10"]);
}

/// String assignment copies, so mutating one variable is not visible through
/// another, and a string constant's shared .data literal can never be written
/// through.
#[test]
fn test_string_assignment_copies() {
    let output = compile_and_run(
        "A$ = \"HELLO\"\nB$ = A$\nMID$(A$,1,1) = \"J\"\nPRINT A$\nPRINT B$\nPRINT \"HELLO\"\nC$ = \"HELLO\"\nPRINT C$\n",
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(
        lines,
        vec!["JELLO", "HELLO", "HELLO", "HELLO"],
        "only A$ changed; the literal is intact"
    );
}

/// MID$ as an assignment target overwrites in place and never changes the
/// target's length.
#[test]
fn test_mid_assignment() {
    let output = compile_and_run(
        "A$ = \"hello\"\nMID$(A$,1,1) = \"J\"\nPRINT A$\nB$ = \"hello\"\nMID$(B$,2) = \"XY\"\nPRINT B$\nC$ = \"abc\"\nMID$(C$,2) = \"ZZZZZ\"\nPRINT C$\nPRINT LEN(C$)\n",
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, vec!["Jello", "hXYlo", "aZZ", "3"]);
}

/// MID$ assignment works on an array element too.
#[test]
fn test_mid_assignment_into_array() {
    let output =
        compile_and_run("DIM S$(2)\nS$(0) = \"hello\"\nMID$(S$(0),1,1) = \"J\"\nPRINT S$(0)\n")
            .unwrap();
    assert_eq!(output.trim(), "Jello");
}

/// Two calls to the same string builtin in one expression must not alias.
///
/// STR$, CHR$, HEX$ and OCT$ each formatted into a buffer they owned outright,
/// and nothing copied the result until it was assigned. Two of them in one
/// expression therefore returned the same pointer, and the first value was
/// gone by the time the expression finished: `STR$(2) + STR$(1)` produced
/// "11", and `STR$(A) = STR$(B)` was true for every A and B.
#[test]
fn test_string_builtins_do_not_share_a_buffer() {
    let source = r#"
A = 2
B = 1
PRINT STR$(A) + STR$(B)
PRINT CHR$(65) + CHR$(66)
PRINT HEX$(10) + HEX$(11)
PRINT OCT$(8) + OCT$(9)
PRINT STR$(A) = STR$(B)
PRINT STR$(A) = STR$(A)
"#;
    let output = compile_and_run(source).unwrap();
    assert_eq!(
        output.lines().collect::<Vec<_>>(),
        vec![" 2 1", "AB", "AB", "1011", " 0 ", "-1 "]
    );
}

/// STR$ renders what PRINT renders, so it neither loses digits nor stops
/// round-tripping through VAL. It used to be its own sprintf("%g"), which cuts
/// off at six significant digits.
#[test]
fn test_str_matches_print_and_round_trips() {
    let source = r#"
PRINT STR$(123456789.125)
PRINT STR$(1 / 3)
PRINT VAL(STR$(1 / 3)) = 1 / 3
PRINT STR$(0.1 + 0.2)
X! = 3.14159
PRINT STR$(X!)
"#;
    let output = compile_and_run(source).unwrap();
    assert_eq!(
        output.lines().collect::<Vec<_>>(),
        vec![
            " 123456789.125",
            " .3333333333333333",
            "-1 ",
            " .30000000000000004",
            // A SINGLE carries ~7 digits, and STR$ respects that as PRINT does.
            " 3.14159",
        ]
    );
}

/// A start position past the end of the string finds nothing.
///
/// The runtime subtracted `start - 1` from the remaining length without
/// checking, so a start beyond the string made that length go negative --
/// which, unsigned, is enormous. The "is there room for the needle" test then
/// passed and memcmp read past the end of the buffer, returning whatever
/// position the garbage happened to match at: 253 and 261 on two runs of the
/// same program.
#[test]
fn test_instr_start_beyond_the_string() {
    let output = compile_and_run(
        r#"
PRINT INSTR(10, "abc", "b")
PRINT INSTR(4, "abc", "b")
PRINT INSTR(3, "abc", "c")
PRINT INSTR(1, "abc", "a")
S = 99
PRINT INSTR(S, "abc", "b")
PRINT INSTR(2, "", "x")
"#,
    )
    .unwrap();
    let lines = crate::common::lines(&output);
    assert_eq!(lines, &["0", "0", "3", "1", "0", "0"]);
}

/// A start position below 1 is an illegal argument, as it is in GW-BASIC.
///
/// It used to move the search pointer *backwards* out of the buffer.
#[test]
fn test_instr_start_below_one_is_refused() {
    for source in [
        "PRINT INSTR(0, \"abc\", \"b\")\n",
        "PRINT INSTR(-1, \"abc\", \"b\")\n",
    ] {
        let run = crate::common::compile_and_run_raw(source, "").expect("should compile");
        assert_eq!(run.exit_code, Some(1), "stderr: {}", run.stderr);
        assert!(
            run.stderr.contains("Illegal function call"),
            "stderr: {}",
            run.stderr
        );
    }
}
