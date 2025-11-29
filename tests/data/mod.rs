//! DATA/READ/RESTORE tests

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

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
