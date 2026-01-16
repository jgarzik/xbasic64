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
