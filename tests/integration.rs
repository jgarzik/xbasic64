//! Integration tests for the BASIC compiler

use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn compile_and_run(source: &str) -> Result<String, String> {
    let tmp = TempDir::new().map_err(|e| e.to_string())?;
    let bas_file = tmp.path().join("test.bas");
    let exe_file = tmp.path().join("test");

    fs::write(&bas_file, source).map_err(|e| e.to_string())?;

    // Compile
    let compile_output = Command::new(env!("CARGO_BIN_EXE_basic64"))
        .arg(&bas_file)
        .arg("-o")
        .arg(&exe_file)
        .output()
        .map_err(|e| format!("Failed to run compiler: {}", e))?;

    if !compile_output.status.success() {
        return Err(format!(
            "Compilation failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&compile_output.stdout),
            String::from_utf8_lossy(&compile_output.stderr)
        ));
    }

    // Run
    let run_output = Command::new(&exe_file)
        .output()
        .map_err(|e| format!("Failed to run executable: {}", e))?;

    if !run_output.status.success() {
        return Err(format!(
            "Execution failed with status {}:\nstderr: {}",
            run_output.status,
            String::from_utf8_lossy(&run_output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&run_output.stdout).to_string())
}

#[test]
fn test_hello_world() {
    let output = compile_and_run(r#"PRINT "Hello, World!""#).unwrap();
    assert_eq!(output.trim(), "Hello, World!");
}

#[test]
fn test_print_number() {
    let output = compile_and_run("PRINT 42").unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_arithmetic_add() {
    let output = compile_and_run("PRINT 10 + 5").unwrap();
    assert_eq!(output.trim(), "15");
}

#[test]
fn test_arithmetic_mul() {
    let output = compile_and_run("PRINT 6 * 7").unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_variable_assignment() {
    let output = compile_and_run(
        r#"
X = 100
Y = 23
PRINT X + Y
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "123");
}

#[test]
fn test_for_loop() {
    let output = compile_and_run(
        r#"
FOR I = 1 TO 5
    PRINT I
NEXT I
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1", "2", "3", "4", "5"]);
}

#[test]
fn test_while_loop() {
    let output = compile_and_run(
        r#"
X = 1
WHILE X <= 3
    PRINT X
    X = X + 1
WEND
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1", "2", "3"]);
}

#[test]
fn test_if_then_else() {
    let output = compile_and_run(
        r#"
X = 10
IF X > 5 THEN
    PRINT "big"
ELSE
    PRINT "small"
END IF
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "big");
}

#[test]
fn test_if_then_else_false() {
    let output = compile_and_run(
        r#"
X = 3
IF X > 5 THEN
    PRINT "big"
ELSE
    PRINT "small"
END IF
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "small");
}

#[test]
fn test_comparison_operators() {
    let output = compile_and_run(
        r#"
IF 5 < 10 THEN PRINT "ok1"
IF 10 > 5 THEN PRINT "ok2"
IF 5 <= 5 THEN PRINT "ok3"
IF 5 >= 5 THEN PRINT "ok4"
IF 5 = 5 THEN PRINT "ok5"
IF 5 <> 6 THEN PRINT "ok6"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines.len(), 6);
}

#[test]
fn test_function_definition() {
    let output = compile_and_run(
        r#"
FUNCTION Double(X)
    Double = X * 2
END FUNCTION

PRINT Double(21)
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_expression_precedence() {
    let output = compile_and_run("PRINT 2 + 3 * 4").unwrap();
    assert_eq!(output.trim(), "14");
}

#[test]
fn test_parentheses() {
    let output = compile_and_run("PRINT (2 + 3) * 4").unwrap();
    assert_eq!(output.trim(), "20");
}

#[test]
fn test_negative_numbers() {
    let output = compile_and_run("PRINT -5 + 10").unwrap();
    assert_eq!(output.trim(), "5");
}

#[test]
fn test_multiple_prints() {
    let output = compile_and_run(
        r#"
PRINT "A"
PRINT "B"
PRINT "C"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["A", "B", "C"]);
}

#[test]
fn test_sqr_function() {
    let output = compile_and_run("PRINT SQR(16)").unwrap();
    assert_eq!(output.trim(), "4");
}

#[test]
fn test_abs_function() {
    let output = compile_and_run("PRINT ABS(-42)").unwrap();
    assert_eq!(output.trim(), "42");
}

// Helper to compile and run with access to temp directory for file I/O tests
fn compile_and_run_with_files<F>(source: &str, setup: F) -> Result<(String, TempDir), String>
where
    F: FnOnce(&std::path::Path) -> Result<(), String>,
{
    let tmp = TempDir::new().map_err(|e| e.to_string())?;
    let bas_file = tmp.path().join("test.bas");
    let exe_file = tmp.path().join("test");

    // Run setup (create input files, etc.)
    setup(tmp.path())?;

    fs::write(&bas_file, source).map_err(|e| e.to_string())?;

    // Compile
    let compile_output = Command::new(env!("CARGO_BIN_EXE_basic64"))
        .arg(&bas_file)
        .arg("-o")
        .arg(&exe_file)
        .output()
        .map_err(|e| format!("Failed to run compiler: {}", e))?;

    if !compile_output.status.success() {
        return Err(format!(
            "Compilation failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&compile_output.stdout),
            String::from_utf8_lossy(&compile_output.stderr)
        ));
    }

    // Run from the temp directory so relative file paths work
    let run_output = Command::new(&exe_file)
        .current_dir(tmp.path())
        .output()
        .map_err(|e| format!("Failed to run executable: {}", e))?;

    if !run_output.status.success() {
        return Err(format!(
            "Execution failed with status {}:\nstderr: {}",
            run_output.status,
            String::from_utf8_lossy(&run_output.stderr)
        ));
    }

    Ok((String::from_utf8_lossy(&run_output.stdout).to_string(), tmp))
}

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
    assert_eq!(output.trim(), "done");

    // Verify file contents
    let file_contents = fs::read_to_string(tmp.path().join("output.txt")).unwrap();
    let lines: Vec<&str> = file_contents.lines().collect();
    assert_eq!(lines, vec!["Hello, File!", "42"]);
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
    assert_eq!(output.trim(), "30");
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
    assert_eq!(output.trim(), "appended");

    // Verify file contents
    let file_contents = fs::read_to_string(tmp.path().join("data.txt")).unwrap();
    let lines: Vec<&str> = file_contents.lines().collect();
    assert_eq!(lines, vec!["Line 1", "Line 2", "Line 3"]);
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
