//! DATA/READ/RESTORE tests (consolidated)

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_and_run;

#[test]
fn test_data_read_restore() {
    // Test DATA/READ and RESTORE
    let output = compile_and_run(
        r#"
DATA 10, 20, 30
READ A
READ B
READ C
PRINT A + B + C
DATA 5, 10
RESTORE
READ D
PRINT D
"#,
    )
    .unwrap();
    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines[0], "60", "data read sum");
    assert_eq!(lines[1], "10", "restore reads first data");
}
