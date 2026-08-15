//! Array tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::{compile_and_run, normalize_output};

#[test]
fn test_arrays_1d_2d() {
    // Test 1D and 2D arrays with various access patterns
    let output = compile_and_run(
        r#"
DIM A(5)
A(1) = 10
A(3) = 30
PRINT A(1)
PRINT A(3)
DIM B(2, 3)
B(0, 0) = 1
B(1, 1) = 5
B(2, 2) = 9
PRINT B(0, 0) + B(1, 1) + B(2, 2)
DIM Grid(1, 2)
FOR I = 0 TO 1
    FOR J = 0 TO 2
        Grid(I, J) = I * 10 + J
    NEXT J
NEXT I
PRINT Grid(0, 0), Grid(0, 1), Grid(0, 2), Grid(1, 0), Grid(1, 1), Grid(1, 2)
"#,
    )
    .unwrap();
    let normalized = normalize_output(&output);
    let lines: Vec<&str> = normalized.lines().collect();
    assert_eq!(lines[0], "10", "1d a(1)");
    assert_eq!(lines[1], "30", "1d a(3)");
    assert_eq!(lines[2], "15", "2d diagonal sum");
    let values: Vec<&str> = lines[3].split_whitespace().collect();
    assert_eq!(values, vec!["0", "1", "2", "10", "11", "12"], "2d loop");
}

#[test]
fn test_3d_array() {
    let output = compile_and_run(
        r#"
DIM Cube(1, 1, 1)
Cube(0, 0, 0) = 1
Cube(0, 0, 1) = 2
Cube(0, 1, 0) = 3
Cube(0, 1, 1) = 4
Cube(1, 0, 0) = 5
Cube(1, 0, 1) = 6
Cube(1, 1, 0) = 7
Cube(1, 1, 1) = 8
PRINT Cube(0, 0, 0) + Cube(1, 1, 1)
"#,
    )
    .unwrap();
    // 1 + 8 = 9
    assert_eq!(output.trim(), "9");
}

/// LANGREF documents `DIM Values%(50)`, but typed numeric arrays returned
/// garbage: every element was stored as an f64 while an array access is typed
/// by its name's suffix, so the bit pattern was reinterpreted.
#[test]
fn test_typed_numeric_arrays() {
    let output = compile_and_run(
        r#"
DIM A%(3)
DIM B&(3)
DIM C!(3)
DIM D#(3)
A%(0) = 3
B&(0) = 100000
C!(0) = 3.5
D#(0) = 3.5
PRINT A%(0)
PRINT B&(0)
PRINT C!(0)
PRINT D#(0)
A%(1) = 3.7
PRINT A%(1)
A%(2) = 2
PRINT A%(0) + A%(2)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(
        lines,
        vec!["3", "100000", "3.5", "3.5", "3", "5"],
        "typed elements keep their declared type; INTEGER truncates"
    );
}

/// Multi-dimensional typed arrays index correctly at the narrower element size.
#[test]
fn test_typed_multidim_array() {
    let output = compile_and_run(
        r#"
DIM M%(2,2)
M%(0,1) = 5
M%(1,1) = 7
M%(2,0) = 9
PRINT M%(0,1); M%(1,1); M%(2,0)
DIM V%(4)
FOR I = 0 TO 4
V%(I) = I * I
NEXT I
FOR I = 0 TO 4
PRINT V%(I);
NEXT I
PRINT ""
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "579");
    assert_eq!(lines[1], "014916");
}

/// A fresh array reads as 0 / "": allocation is zeroed, which plain malloc
/// does not guarantee.
#[test]
fn test_arrays_start_zeroed() {
    let output =
        compile_and_run("DIM A(5)\nDIM S$(2)\nPRINT A(0); A(3); A(5)\nPRINT LEN(S$(1))\n").unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["000", "0"]);
}

/// REDIM resizes an existing array, reusing its descriptor, and clears it.
#[test]
fn test_redim() {
    let output = compile_and_run(
        "DIM A(2)\nA(0) = 7\nREDIM A(5)\nPRINT A(0)\nA(5) = 9\nPRINT A(5); UBOUND(A)\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["0", "95"], "contents cleared, bound updated");
}

/// REDIM PRESERVE keeps the existing elements and zeroes the new tail.
#[test]
fn test_redim_preserve() {
    let output = compile_and_run(
        "DIM A(2)\nA(0) = 7\nA(1) = 8\nREDIM PRESERVE A(5)\nPRINT A(0); A(1); A(5)\nPRINT UBOUND(A)\n",
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["780", "5"]);
}

/// Growing an array a step at a time, which is what PRESERVE is for.
#[test]
fn test_redim_preserve_in_loop() {
    let output = compile_and_run(
        "DIM A(1)\nFOR I = 1 TO 3\nREDIM PRESERVE A(I)\nA(I) = I * 10\nNEXT I\nPRINT A(1); A(2); A(3)\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "102030");
}

/// String arrays survive PRESERVE too.
#[test]
fn test_redim_preserve_strings() {
    let output = compile_and_run(
        "DIM S$(1)\nS$(0) = \"keep\"\nREDIM PRESERVE S$(3)\nPRINT S$(0); LEN(S$(0))\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "keep4");
}

/// LBOUND and UBOUND take an array *name*, so a string array is fine.
#[test]
fn test_bounds_of_string_array() {
    let output = compile_and_run("DIM N$(5)\nPRINT LBOUND(N$); UBOUND(N$)\n").unwrap();
    assert_eq!(output.trim(), "05");
}

/// The dimension may be computed, not only written as a literal.
///
/// A non-literal dimension used to be silently treated as dimension 1.
#[test]
fn test_bounds_with_computed_dimension() {
    let output = compile_and_run(
        "DIM A(2,5)\nCONST D = 2\nK = 2\nPRINT UBOUND(A, K); UBOUND(A, D); UBOUND(A, 1)\n",
    )
    .unwrap();
    assert_eq!(output.trim(), "552");
}
