//! Type system tests (conversion, promotion, truncation)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_type_conversions() {
    // CINT, CLNG, CSNG, CDBL conversion functions
    let output = compile_and_run(
        r#"
PRINT CINT(3.7)
PRINT CLNG(3.7)
X! = CSNG(3): PRINT X!
Y# = CDBL(3): PRINT Y#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "4", "cint rounds");
    assert_eq!(lines[1], "4", "clng rounds");
    assert_eq!(lines[2], "3", "csng");
    assert_eq!(lines[3], "3", "cdbl");
}

#[test]
fn test_truncation_and_assignment() {
    // Truncation and cross-type assignments
    let output = compile_and_run(
        r#"
PRINT CINT(3.1)
PRINT CINT(3.5)
PRINT CINT(3.9)
PRINT CINT(-3.1)
PRINT CINT(-3.5)
PRINT CINT(-3.9)
A% = 42: B& = A%: PRINT B&
A% = 42: B# = A%: PRINT B#
A# = 3.7: B% = A#: PRINT B%
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "3", "cint 3.1");
    assert_eq!(lines[1], "4", "cint 3.5");
    assert_eq!(lines[2], "4", "cint 3.9");
    assert_eq!(lines[3], "-3", "cint -3.1");
    assert_eq!(lines[4], "-4", "cint -3.5");
    assert_eq!(lines[5], "-4", "cint -3.9");
    assert_eq!(lines[6], "42", "int to long");
    assert_eq!(lines[7], "42", "int to double");
    assert_eq!(lines[8], "3", "double to int truncates");
}

#[test]
fn test_division_types() {
    // Division (/) always produces Double, integer division (\) produces Long
    let output = compile_and_run(
        r#"
A% = 7: B% = 2: PRINT A% / B%
A% = 7: B% = 2: PRINT A% \ B%
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "3.5", "division produces double");
    assert_eq!(lines[1], "3", "integer division");
}

#[test]
fn test_promotion_add() {
    // Type promotion for addition: int+long, int+single, int+double, long+single, long+double, single+double
    let output = compile_and_run(
        r#"
A% = 100: B& = 200: PRINT A% + B&
A% = 10: B! = 2.5: PRINT A% + B!
A% = 10: B# = 2.5: PRINT A% + B#
A& = 100: B! = 0.5: PRINT A& + B!
A& = 100: B# = 0.25: PRINT A& + B#
A! = 1.5: B# = 2.5: PRINT A! + B#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "300", "int+long");
    assert_eq!(lines[1], "12.5", "int+single");
    assert_eq!(lines[2], "12.5", "int+double");
    assert_eq!(lines[3], "100.5", "long+single");
    assert_eq!(lines[4], "100.25", "long+double");
    assert_eq!(lines[5], "4", "single+double");
}

#[test]
fn test_promotion_sub() {
    // Type promotion for subtraction
    let output = compile_and_run(
        r#"
A% = 50: B& = 20: PRINT A% - B&
A% = 10: B! = 2.5: PRINT A% - B!
A% = 10: B# = 3.25: PRINT A% - B#
A& = 100: B! = 0.5: PRINT A& - B!
A& = 100: B# = 0.25: PRINT A& - B#
A! = 5.5: B# = 2.25: PRINT A! - B#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "30", "int-long");
    assert_eq!(lines[1], "7.5", "int-single");
    assert_eq!(lines[2], "6.75", "int-double");
    assert_eq!(lines[3], "99.5", "long-single");
    assert_eq!(lines[4], "99.75", "long-double");
    assert_eq!(lines[5], "3.25", "single-double");
}

#[test]
fn test_promotion_mul() {
    // Type promotion for multiplication
    let output = compile_and_run(
        r#"
A% = 10: B& = 20: PRINT A% * B&
A% = 4: B! = 2.5: PRINT A% * B!
A% = 3: B# = 2.5: PRINT A% * B#
A& = 100: B! = 0.5: PRINT A& * B!
A& = 100: B# = 0.25: PRINT A& * B#
A! = 2.5: B# = 4.0: PRINT A! * B#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "200", "int*long");
    assert_eq!(lines[1], "10", "int*single");
    assert_eq!(lines[2], "7.5", "int*double");
    assert_eq!(lines[3], "50", "long*single");
    assert_eq!(lines[4], "25", "long*double");
    assert_eq!(lines[5], "10", "single*double");
}

#[test]
fn test_promotion_div_intdiv_mod() {
    // Type promotion for division, integer division, and mod
    let output = compile_and_run(
        r#"
A% = 7: B& = 2: PRINT A% / B&
A% = 5: B! = 2.0: PRINT A% / B!
A& = 9: B! = 2.0: PRINT A& / B!
A& = 11: B# = 4.0: PRINT A& / B#
A! = 7.0: B# = 2.0: PRINT A! / B#
A% = 17: B& = 5: PRINT A% \ B&
A% = 17: B! = 5.0: PRINT A% \ B!
A& = 25: B# = 7.0: PRINT A& \ B#
A! = 100.0: B# = 30.0: PRINT A! \ B#
A% = 17: B& = 5: PRINT A% MOD B&
A% = 17: B! = 5.0: PRINT A% MOD B!
A& = 25: B# = 7.0: PRINT A& MOD B#
A! = 100.0: B# = 30.0: PRINT A! MOD B#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "3.5", "int/long");
    assert_eq!(lines[1], "2.5", "int/single");
    assert_eq!(lines[2], "4.5", "long/single");
    assert_eq!(lines[3], "2.75", "long/double");
    assert_eq!(lines[4], "3.5", "single/double");
    assert_eq!(lines[5], "3", "int\\long");
    assert_eq!(lines[6], "3", "int\\single");
    assert_eq!(lines[7], "3", "long\\double");
    assert_eq!(lines[8], "3", "single\\double");
    assert_eq!(lines[9], "2", "int mod long");
    assert_eq!(lines[10], "2", "int mod single");
    assert_eq!(lines[11], "4", "long mod double");
    assert_eq!(lines[12], "10", "single mod double");
}

#[test]
fn test_promotion_pow() {
    // Type promotion for power operation
    let output = compile_and_run(
        r#"
A% = 2: B& = 8: PRINT A% ^ B&
A% = 4: B! = 0.5: PRINT A% ^ B!
A% = 2: B# = 3.0: PRINT A% ^ B#
A& = 9: B! = 0.5: PRINT A& ^ B!
A& = 3: B# = 4.0: PRINT A& ^ B#
A! = 2.0: B# = 10.0: PRINT A! ^ B#
A% = 10: B& = 20: C! = 0.5: D# = 100.0: PRINT A% + B& * C! + D#
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "256", "int^long");
    assert_eq!(lines[1], "2", "int^single");
    assert_eq!(lines[2], "8", "int^double");
    assert_eq!(lines[3], "3", "long^single");
    assert_eq!(lines[4], "81", "long^double");
    assert_eq!(lines[5], "1024", "single^double");
    assert_eq!(lines[6], "120", "mixed expression");
}

/// User-defined TYPE records: declaration, field access, and mixed field types.
#[test]
fn test_type_records() {
    let output = compile_and_run(
        "TYPE Rec\nI AS INTEGER\nL AS LONG\nS AS SINGLE\nD AS DOUBLE\nN AS STRING * 20\nEND TYPE\nDIM R AS Rec\nPRINT R.I\nR.I = 7\nR.L = 100000\nR.S = 2.5\nR.D = 1.25\nR.N = \"hello\"\nPRINT R.I; R.L; R.S; R.D\nPRINT R.N; LEN(R.N)\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "0", "a record starts zeroed");
    assert_eq!(lines[1], "71000002.51.25", "each field keeps its own type");
    // A `STRING * 20` field holds twenty characters whatever it is given, so
    // "hello" is space-padded and LEN is 20. This asserted "hello5" while
    // assignment stored the source verbatim and the declared width did nothing.
    assert_eq!(lines[2], "hello               20");
}

/// A TYPE may contain another TYPE, to any depth.
#[test]
fn test_nested_records() {
    let output = compile_and_run(
        "TYPE Point\nX AS INTEGER\nY AS INTEGER\nEND TYPE\nTYPE Rect\nTL AS Point\nBR AS Point\nEND TYPE\nDIM B AS Rect\nB.TL.X = 1\nB.TL.Y = 2\nB.BR.X = 9\nB.BR.Y = 8\nPRINT B.TL.X; B.TL.Y; B.BR.X; B.BR.Y\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "1298");
}

/// Assigning one record to another copies it, rather than aliasing.
#[test]
fn test_whole_record_assignment() {
    let output = compile_and_run(
        "TYPE P\nX AS INTEGER\nY AS INTEGER\nEND TYPE\nDIM A AS P\nDIM B AS P\nB.X = 3\nB.Y = 4\nA = B\nPRINT A.X; A.Y\nB.X = 99\nPRINT A.X\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["34", "3"], "A kept its own copy");
}

/// Arrays of records, including a nested field and a string field.
#[test]
fn test_arrays_of_records() {
    let output = compile_and_run(
        "TYPE Person\nNM AS STRING * 20\nAGE AS INTEGER\nEND TYPE\nDIM P(2) AS Person\nP(0).NM = \"Alice\"\nP(0).AGE = 30\nP(1).NM = \"Bob\"\nP(1).AGE = 25\nPRINT P(0).NM; P(0).AGE\nPRINT P(1).NM; P(1).AGE\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["Alice30", "Bob25"]);
}

/// A record array indexed by a loop variable.
#[test]
fn test_record_array_in_loop() {
    let output = compile_and_run(
        "TYPE P\nN AS INTEGER\nEND TYPE\nDIM A(4) AS P\nFOR I = 0 TO 4\nA(I).N = I * I\nNEXT I\nFOR I = 0 TO 4\nPRINT A(I).N;\nNEXT I\nPRINT \"\"\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "014916");
}

/// A module-level record is shared with procedures, like any other global.
#[test]
fn test_record_visible_in_procedure() {
    let output = compile_and_run(
        "TYPE P\nX AS INTEGER\nEND TYPE\nDIM G AS P\nSUB Bump\nG.X = G.X + 1\nEND SUB\nG.X = 5\nBump\nBump\nPRINT G.X\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "7");
}

/// A record declared inside a procedure is local to it and zeroed each call.
#[test]
fn test_local_record_is_fresh_each_call() {
    let output = compile_and_run(
        "TYPE P\nX AS INTEGER\nEND TYPE\nSUB T\nDIM L AS P\nPRINT L.X\nL.X = 9\nEND SUB\nT\nT\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["0", "0"]);
}

/// A record parameter's frame slot must not outlive its procedure.
///
/// Storage for typed variables used to live in one map that `gen_procedure`
/// never cleared, so a later procedure -- or module-level code -- resolved the
/// name to the earlier procedure's frame offset and read stack garbage.
#[test]
fn test_record_storage_does_not_leak_between_procedures() {
    let output = compile_and_run(
        "TYPE P\nX AS INTEGER\nEND TYPE\nDIM G AS P\nSUB First(G AS P)\nPRINT G.X\nEND SUB\nSUB Second\nPRINT G.X\nEND SUB\nG.X = 7\nSecond\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "7");
}

/// Two procedures each declaring a local record must get distinct slots, and
/// neither may inherit the other's.
#[test]
fn test_local_records_in_sibling_procedures() {
    let output = compile_and_run(
        "TYPE P\nX AS INTEGER\nY AS INTEGER\nEND TYPE\nSUB A(Z AS P)\nPRINT Z.X\nEND SUB\nSUB B\nDIM Z AS P\nZ.X = 5\nZ.Y = 6\nPRINT Z.X; Z.Y\nEND SUB\nB\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "56");
}

/// A record is passed to a procedure by value: the callee gets the address of
/// the caller's copy and copies it into a local slot, so changes do not escape.
#[test]
fn test_record_parameters() {
    let output = compile_and_run(
        "TYPE P\nX AS INTEGER\nY AS INTEGER\nEND TYPE\nSUB Show(V AS P)\nPRINT V.X; V.Y\nV.X = 99\nEND SUB\nDIM A AS P\nA.X = 7\nA.Y = 8\nShow(A)\nPRINT A.X\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["78", "7"], "the caller's record is unchanged");
}

/// A record parameter mixed with ordinary ones, and a nested record.
#[test]
fn test_record_parameter_mixed_and_nested() {
    let output = compile_and_run(
        "TYPE Pt\nX AS INTEGER\nEND TYPE\nTYPE Bx\nTL AS Pt\nEND TYPE\nSUB T(A, V AS Bx, B)\nPRINT A; V.TL.X; B\nEND SUB\nDIM Q AS Bx\nQ.TL.X = 5\nT(1, Q, 2)\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "152");
}

/// `AS` also gives a plain variable or parameter a declared type.
#[test]
fn test_as_typed_variables() {
    let output = compile_and_run(
        "DIM N AS INTEGER\nDIM S AS STRING * 10\nN = 42\nS = \"hi\"\nPRINT N; S\nPRINT N * 2\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    // `S` is `STRING * 10`, so "hi" is padded to ten characters.
    assert_eq!(lines, vec!["42hi        ", "84"]);
}

/// A typed parameter, and a FUNCTION with a declared result type.
#[test]
fn test_as_typed_parameters_and_result() {
    let output = compile_and_run(
        "SUB T(N AS INTEGER, S AS STRING * 10)\nPRINT N; S\nEND SUB\nFUNCTION F AS INTEGER\nF = 42\nEND FUNCTION\nT(7, \"hi\")\nPRINT F\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["7hi", "42"]);
}

/// A record variable may be assigned from an array element, whose address is
/// only known at run time.
#[test]
fn test_record_assignment_from_array_element() {
    let output = compile_and_run(
        "TYPE P\nX AS INTEGER\nY AS INTEGER\nEND TYPE\nDIM A(2) AS P\nDIM One AS P\nA(1).X = 3\nA(1).Y = 4\nOne = A(1)\nPRINT One.X; One.Y\nA(1).X = 99\nPRINT One.X\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["34", "3"], "the copy is independent");
}

/// A record argument may be any record lvalue, not only a plain variable.
///
/// The callee is handed the address of the caller's copy; `A(1)` used to miss
/// that path entirely and pass a float, which the prologue then dereferenced.
#[test]
fn test_record_argument_from_array_element() {
    let output = compile_and_run(
        "TYPE P\nX AS INTEGER\nEND TYPE\nSUB Show(V AS P)\nPRINT V.X\nEND SUB\nDIM A(3) AS P\nA(1).X = 9\nA(2).X = 4\nShow A(1)\nShow A(2)\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["9", "4"]);
}

/// The same for a nested record reached through a field path.
#[test]
fn test_record_argument_from_nested_field() {
    let output = compile_and_run(
        "TYPE P\nX AS INTEGER\nEND TYPE\nTYPE Q\nI AS P\nEND TYPE\nSUB Show(V AS P)\nPRINT V.X\nEND SUB\nDIM W AS Q\nW.I.X = 4\nShow W.I\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "4");
}

/// A `STRING * n` field always holds exactly n characters.
///
/// Assignment stored the source verbatim, so the declared width did nothing at
/// all: a longer value was kept whole and a shorter one stayed short. That is
/// the entire purpose of a fixed-length string, and it is what makes a record
/// laid out over a random-access file line up.
#[test]
fn test_fixed_length_string_pads_and_truncates() {
    let output = compile_and_run(
        r#"
TYPE R
  N AS STRING * 5
END TYPE
DIM P AS R
P.N = "abcdefgh"
PRINT "["; P.N; "]"
PRINT LEN(P.N)
P.N = "ab"
PRINT "["; P.N; "]"
PRINT LEN(P.N)
P.N = ""
PRINT "["; P.N; "]"
PRINT LEN(P.N)
P.N = "exact"
PRINT "["; P.N; "]"
PRINT LEN(P.N)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        &[
            "[abcde]", "5", "[ab   ]", "5", "[     ]", "5", "[exact]", "5"
        ]
    );
}

/// The same for a standalone `DIM ... AS STRING * n`, and it compares equal to
/// the padded text.
#[test]
fn test_fixed_length_string_variable() {
    let output = compile_and_run(
        r#"
DIM S AS STRING * 4
S = "xy"
PRINT "["; S; "]"
PRINT S = "xy  "
PRINT LEN(S)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, &["[xy  ]", "-1", "4"]);
}
