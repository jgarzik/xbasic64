//! Integration tests for the BASIC compiler

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn compile_and_run(source: &str) -> Result<String, String> {
    compile_and_run_with_stdin(source, "")
}

fn compile_and_run_with_stdin(source: &str, stdin_input: &str) -> Result<String, String> {
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

    // Run with optional stdin
    let mut child = Command::new(&exe_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run executable: {}", e))?;

    if !stdin_input.is_empty() {
        let child_stdin = child.stdin.as_mut().unwrap();
        child_stdin
            .write_all(stdin_input.as_bytes())
            .map_err(|e| format!("Failed to write to stdin: {}", e))?;
    }

    let run_output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for executable: {}", e))?;

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

#[test]
fn test_select_case() {
    let output = compile_and_run(
        r#"
X = 2
SELECT CASE X
    CASE 1
        PRINT "one"
    CASE 2
        PRINT "two"
    CASE 3
        PRINT "three"
    CASE ELSE
        PRINT "other"
END SELECT
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "two");
}

#[test]
fn test_select_case_else() {
    let output = compile_and_run(
        r#"
X = 99
SELECT CASE X
    CASE 1
        PRINT "one"
    CASE 2
        PRINT "two"
    CASE ELSE
        PRINT "other"
END SELECT
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "other");
}

// === Control Flow Tests ===

#[test]
fn test_do_loop_while() {
    let output = compile_and_run(
        r#"
X = 1
DO WHILE X <= 3
    PRINT X
    X = X + 1
LOOP
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1", "2", "3"]);
}

#[test]
fn test_do_loop_until() {
    let output = compile_and_run(
        r#"
X = 1
DO UNTIL X > 3
    PRINT X
    X = X + 1
LOOP
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1", "2", "3"]);
}

#[test]
fn test_do_loop_while_post() {
    let output = compile_and_run(
        r#"
X = 1
DO
    PRINT X
    X = X + 1
LOOP WHILE X <= 3
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["1", "2", "3"]);
}

#[test]
fn test_goto() {
    let output = compile_and_run(
        r#"
10 PRINT "A"
20 GOTO 40
30 PRINT "B"
40 PRINT "C"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["A", "C"]);
}

#[test]
fn test_gosub_return() {
    let output = compile_and_run(
        r#"
10 PRINT "start"
20 GOSUB 100
30 PRINT "end"
40 END
100 PRINT "in sub"
110 RETURN
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["start", "in sub", "end"]);
}

#[test]
fn test_on_goto() {
    let output = compile_and_run(
        r#"
10 X = 2
20 ON X GOTO 100, 200, 300
30 PRINT "none"
40 END
100 PRINT "first"
110 END
200 PRINT "second"
210 END
300 PRINT "third"
310 END
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "second");
}

#[test]
fn test_sub_definition() {
    let output = compile_and_run(
        r#"
PrintHello
PRINT "done"
END

SUB PrintHello
    PRINT "Hello from sub"
END SUB
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["Hello from sub", "done"]);
}

#[test]
fn test_sub_with_params() {
    let output = compile_and_run(
        r#"
AddPrint(10, 20)
END

SUB AddPrint(A, B)
    PRINT A + B
END SUB
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "30");
}

// === DATA/READ/RESTORE Tests ===

#[test]
fn test_data_read() {
    let output = compile_and_run(
        r#"
DATA 10, 20, 30
READ A
READ B
READ C
PRINT A + B + C
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "60");
}

#[test]
fn test_data_restore() {
    let output = compile_and_run(
        r#"
DATA 5, 10
READ A
READ B
RESTORE
READ C
PRINT A + B + C
"#,
    )
    .unwrap();
    // A=5, B=10, C=5 (restored)
    assert_eq!(output.trim(), "20");
}

// === FOR STEP and ELSEIF Tests ===

#[test]
fn test_for_step_positive() {
    let output = compile_and_run(
        r#"
FOR I = 0 TO 10 STEP 2
    PRINT I
NEXT I
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["0", "2", "4", "6", "8", "10"]);
}

#[test]
fn test_for_step_negative() {
    let output = compile_and_run(
        r#"
FOR I = 5 TO 1 STEP -1
    PRINT I
NEXT I
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["5", "4", "3", "2", "1"]);
}

#[test]
fn test_elseif() {
    let output = compile_and_run(
        r#"
X = 2
IF X = 1 THEN
    PRINT "one"
ELSEIF X = 2 THEN
    PRINT "two"
ELSEIF X = 3 THEN
    PRINT "three"
ELSE
    PRINT "other"
END IF
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "two");
}

#[test]
fn test_end_statement() {
    let output = compile_and_run(
        r#"
PRINT "before"
END
PRINT "after"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "before");
}

// === Arithmetic and Logical Operator Tests ===

#[test]
fn test_arithmetic_division() {
    let output = compile_and_run("PRINT 10 / 4").unwrap();
    assert_eq!(output.trim(), "2.5");
}

#[test]
fn test_arithmetic_integer_division() {
    let output = compile_and_run("PRINT 10 \\ 4").unwrap();
    assert_eq!(output.trim(), "2");
}

#[test]
fn test_arithmetic_mod() {
    let output = compile_and_run("PRINT 10 MOD 3").unwrap();
    assert_eq!(output.trim(), "1");
}

#[test]
fn test_arithmetic_power() {
    let output = compile_and_run("PRINT 2 ^ 10").unwrap();
    assert_eq!(output.trim(), "1024");
}

#[test]
fn test_logical_and() {
    let output = compile_and_run(
        r#"
IF 1 AND 1 THEN PRINT "yes"
IF 1 AND 0 THEN PRINT "no"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "yes");
}

#[test]
fn test_logical_or() {
    let output = compile_and_run(
        r#"
IF 0 OR 1 THEN PRINT "yes"
IF 0 OR 0 THEN PRINT "no"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "yes");
}

#[test]
fn test_logical_not() {
    let output = compile_and_run(
        r#"
IF NOT 0 THEN PRINT "yes"
IF NOT 1 THEN PRINT "no"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "yes");
}

#[test]
fn test_logical_xor() {
    let output = compile_and_run(
        r#"
IF 1 XOR 0 THEN PRINT "a"
IF 0 XOR 1 THEN PRINT "b"
IF 1 XOR 1 THEN PRINT "c"
IF 0 XOR 0 THEN PRINT "d"
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["a", "b"]);
}

// === Math Function Tests ===

#[test]
fn test_int_function() {
    let output = compile_and_run("PRINT INT(3.7)").unwrap();
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_fix_function() {
    // FIX truncates toward zero, INT floors
    let output = compile_and_run("PRINT FIX(-3.7)").unwrap();
    assert_eq!(output.trim(), "-3");
}

#[test]
fn test_sgn_function() {
    let output = compile_and_run(
        r#"
PRINT SGN(-5)
PRINT SGN(0)
PRINT SGN(5)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["-1", "0", "1"]);
}

#[test]
fn test_sin_cos() {
    let output = compile_and_run("PRINT INT(SIN(0) * 100), INT(COS(0) * 100)").unwrap();
    // sin(0) = 0, cos(0) = 1
    let values: Vec<&str> = output.split_whitespace().collect();
    assert_eq!(values, vec!["0", "100"]);
}

#[test]
fn test_tan_atn() {
    let output = compile_and_run("PRINT INT(TAN(0) * 100), INT(ATN(0) * 100)").unwrap();
    // tan(0) = 0, atn(0) = 0
    let values: Vec<&str> = output.split_whitespace().collect();
    assert_eq!(values, vec!["0", "0"]);
}

#[test]
fn test_exp_log() {
    let output = compile_and_run("PRINT INT(EXP(0)), INT(LOG(1))").unwrap();
    // exp(0) = 1, log(1) = 0
    let values: Vec<&str> = output.split_whitespace().collect();
    assert_eq!(values, vec!["1", "0"]);
}

#[test]
fn test_rnd_function() {
    // RND returns a value between 0 and 1
    let output = compile_and_run(
        r#"
X = RND(1)
IF X >= 0 AND X < 1 THEN PRINT "ok"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "ok");
}

// === String Function Tests ===

#[test]
fn test_len_function() {
    let output = compile_and_run(r#"PRINT LEN("Hello")"#).unwrap();
    assert_eq!(output.trim(), "5");
}

#[test]
fn test_left_right() {
    let output = compile_and_run(
        r#"
PRINT LEFT$("Hello", 2)
PRINT RIGHT$("Hello", 2)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["He", "lo"]);
}

#[test]
fn test_mid_function() {
    let output = compile_and_run(r#"PRINT MID$("Hello", 2, 3)"#).unwrap();
    assert_eq!(output.trim(), "ell");
}

#[test]
fn test_chr_asc() {
    let output = compile_and_run(
        r#"
PRINT CHR$(65)
PRINT ASC("A")
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["A", "65"]);
}

#[test]
fn test_val_str() {
    let output = compile_and_run(
        r#"
X = VAL("42")
PRINT X + 8
PRINT STR$(100)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["50", "100"]);
}

#[test]
fn test_instr_function() {
    let output = compile_and_run(r#"PRINT INSTR("Hello World", "World")"#).unwrap();
    assert_eq!(output.trim(), "7");
}

// === Conversion Function Tests ===

#[test]
fn test_cint_clng() {
    let output = compile_and_run(
        r#"
PRINT CINT(3.7)
PRINT CLNG(3.7)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines, vec!["4", "4"]);
}

#[test]
fn test_csng_cdbl() {
    // These convert to float types; test that they work
    let output = compile_and_run(
        r#"
X! = CSNG(3)
Y# = CDBL(3)
PRINT X! + Y#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "6");
}

#[test]
fn test_timer_function() {
    // TIMER returns seconds since midnight; just verify it returns a number
    let output = compile_and_run(
        r#"
T = TIMER
IF T >= 0 THEN PRINT "ok"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "ok");
}

// === Type Suffix Tests ===

#[test]
fn test_integer_suffix() {
    let output = compile_and_run(
        r#"
X% = 32000
PRINT X%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "32000");
}

#[test]
fn test_long_suffix() {
    let output = compile_and_run(
        r#"
X& = 100000
PRINT X&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "100000");
}

#[test]
fn test_string_variable() {
    let output = compile_and_run(
        r#"
X$ = "Hello"
Y$ = " World"
PRINT X$ + Y$
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "Hello World");
}

// === Comment Tests ===

#[test]
fn test_rem_comment() {
    let output = compile_and_run(
        r#"
REM This is a comment
PRINT "before"
REM Another comment
PRINT "after"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "before\nafter");
}

// === STOP Statement Tests ===

#[test]
fn test_stop_statement() {
    let output = compile_and_run(
        r#"
PRINT "before"
STOP
PRINT "after"
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "before");
}

// === DIM Array Declaration Tests ===

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

// === INPUT Statement Tests ===

#[test]
fn test_input_number() {
    let output = compile_and_run_with_stdin(
        r#"
INPUT X
PRINT X * 2
"#,
        "21\n",
    )
    .unwrap();
    assert!(output.contains("42"));
}

#[test]
fn test_input_string() {
    let output = compile_and_run_with_stdin(
        r#"
INPUT A$
PRINT "Hello, "; A$
"#,
        "World\n",
    )
    .unwrap();
    assert!(output.contains("Hello, World"));
}

#[test]
fn test_line_input() {
    let output = compile_and_run_with_stdin(
        r#"
LINE INPUT A$
PRINT A$
"#,
        "Hello, World!\n",
    )
    .unwrap();
    assert!(output.contains("Hello, World!"));
}

// === Type Conversion Tests ===

#[test]
fn test_single_suffix() {
    // Test Single (!) type suffix - 32-bit float
    let output = compile_and_run(
        r#"
X! = 3.14159
PRINT X!
"#,
    )
    .unwrap();
    // Single has ~7 significant digits
    assert!(output.contains("3.14159"));
}

#[test]
fn test_single_arithmetic() {
    // Test arithmetic with Single type
    let output = compile_and_run(
        r#"
A! = 2.5
B! = 3.5
PRINT A! + B!
PRINT A! * B!
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "6");
    assert_eq!(lines[1], "8.75");
}

#[test]
fn test_type_promotion_integer_long() {
    // Integer + Long should promote to Long
    let output = compile_and_run(
        r#"
A% = 100
B& = 200
PRINT A% + B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "300");
}

#[test]
fn test_type_promotion_integer_single() {
    // Integer + Single should promote to Single
    let output = compile_and_run(
        r#"
A% = 10
B! = 2.5
PRINT A% + B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "12.5");
}

#[test]
fn test_type_promotion_integer_double() {
    // Integer + Double should promote to Double
    let output = compile_and_run(
        r#"
A% = 10
B# = 2.5
PRINT A% + B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "12.5");
}

#[test]
fn test_type_promotion_long_single() {
    // Long + Single should promote to Single
    let output = compile_and_run(
        r#"
A& = 100
B! = 0.5
PRINT A& + B!
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "100.5");
}

#[test]
fn test_type_promotion_long_double() {
    // Long + Double should promote to Double
    let output = compile_and_run(
        r#"
A& = 100
B# = 0.25
PRINT A& + B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "100.25");
}

#[test]
fn test_type_promotion_single_double() {
    // Single + Double should promote to Double
    let output = compile_and_run(
        r#"
A! = 1.5
B# = 2.5
PRINT A! + B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "4");
}

#[test]
fn test_cross_type_assignment_int_to_long() {
    // Assign Integer to Long variable
    let output = compile_and_run(
        r#"
A% = 42
B& = A%
PRINT B&
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_cross_type_assignment_int_to_double() {
    // Assign Integer to Double variable
    let output = compile_and_run(
        r#"
A% = 42
B# = A%
PRINT B#
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_cross_type_assignment_double_to_int() {
    // Assign Double to Integer variable (truncation)
    let output = compile_and_run(
        r#"
A# = 3.7
B% = A#
PRINT B%
"#,
    )
    .unwrap();
    // Truncates toward zero
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_truncation_positive() {
    // Positive float truncation
    let output = compile_and_run(
        r#"
PRINT CINT(3.1)
PRINT CINT(3.5)
PRINT CINT(3.9)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    // CINT uses banker's rounding (round half to even)
    assert_eq!(lines[0], "3");
    assert_eq!(lines[1], "4");
    assert_eq!(lines[2], "4");
}

#[test]
fn test_truncation_negative() {
    // Negative float truncation
    let output = compile_and_run(
        r#"
PRINT CINT(-3.1)
PRINT CINT(-3.5)
PRINT CINT(-3.9)
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    // CINT rounds toward nearest
    assert_eq!(lines[0], "-3");
    assert_eq!(lines[1], "-4");
    assert_eq!(lines[2], "-4");
}

#[test]
fn test_division_produces_double() {
    // Division (/) always produces Double
    let output = compile_and_run(
        r#"
A% = 7
B% = 2
PRINT A% / B%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3.5");
}

#[test]
fn test_integer_division() {
    // Integer division (\) produces Long
    let output = compile_and_run(
        r#"
A% = 7
B% = 2
PRINT A% \ B%
"#,
    )
    .unwrap();
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_mixed_expression_complex() {
    // Complex expression with multiple type promotions
    let output = compile_and_run(
        r#"
A% = 10
B& = 20
C! = 0.5
D# = 100.0
PRINT A% + B& * C! + D#
"#,
    )
    .unwrap();
    // 10 + 20*0.5 + 100 = 10 + 10 + 100 = 120
    assert_eq!(output.trim(), "120");
}
