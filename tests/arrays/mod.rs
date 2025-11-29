//! Array tests

use crate::common::compile_and_run;

#[test]
fn test_dim_single_array() {
    let output = compile_and_run(
        r#"
DIM A(5)
A(1) = 10
A(3) = 30
PRINT A(1)
PRINT A(3)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "10\n30");
}

#[test]
fn test_2d_array() {
    let output = compile_and_run(
        r#"
DIM A(2, 3)
A(0, 0) = 1
A(0, 1) = 2
A(0, 2) = 3
A(1, 0) = 4
A(1, 1) = 5
A(1, 2) = 6
A(2, 0) = 7
A(2, 1) = 8
A(2, 2) = 9
PRINT A(0, 0) + A(1, 1) + A(2, 2)
"#,
    )
    .unwrap();
    // Diagonal sum: 1 + 5 + 9 = 15
    assert_eq!(output.trim(), "15");
}

#[test]
fn test_2d_array_loop() {
    let output = compile_and_run(
        r#"
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
    // Comma-separated prints with tabs
    let values: Vec<&str> = output.split_whitespace().collect();
    assert_eq!(values, vec!["0", "1", "2", "10", "11", "12"]);
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
