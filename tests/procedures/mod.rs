//! Function and subroutine tests

use crate::common::compile_and_run;

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
