//! Print statement tests

use crate::common::compile_and_run;

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
