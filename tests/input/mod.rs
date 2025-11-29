//! INPUT statement tests

use crate::common::compile_and_run_with_stdin;

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
