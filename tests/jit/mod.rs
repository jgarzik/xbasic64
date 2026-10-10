//! `xbasic64 run`: a program compiled into memory and run in the compiler's
//! own process (src/jit.rs)
//!
//! The rest of the suite runs in this mode too, with `XB_JIT=1`; these are
//! the things particular to it -- how the process ends, what reaches stdout
//! and stderr, and that it agrees with an executable.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::{compile_and_run_raw, compile_and_run_with_files, run_jit, run_jit_in};

/// Output, and nothing of the compiler's: no "Compiled" line, and the last
/// line's bytes even without a newline -- flushed as the program ends.
#[test]
fn test_run_prints_only_the_program() {
    let run = run_jit("PRINT \"hi\"; 1 / 3\nPRINT \"last\";\n", "", &[]).unwrap();
    run.assert_ran_to_completion("run");
    assert_eq!(
        run.stdout.replace("\r\n", "\n"),
        "hi .3333333333333333 \nlast"
    );
    assert_eq!(run.stderr, "");
}

/// The program's exit is the process's: END from inside a SUB, STOP, and
/// a runtime error, which reports itself and exits 1 after what was
/// printed before it.
#[test]
fn test_run_ends_as_an_executable_does() {
    let run = run_jit(
        "SUB S\n  PRINT \"in\"\n  END\nEND SUB\nS\nPRINT \"no\"\n",
        "",
        &[],
    )
    .unwrap();
    run.assert_ran_to_completion("END in a SUB");
    assert_eq!(run.lines(), vec!["in"]);

    let run = run_jit("10 PRINT \"a\"\n20 STOP\n30 PRINT \"no\"\n", "", &[]).unwrap();
    assert_eq!(run.exit_code, Some(0));
    assert_eq!(run.lines(), vec!["a"]);
    assert!(run.stderr.contains("Break in 20"), "{:?}", run.stderr);

    let run = run_jit("PRINT \"before\"\nX = 0\nPRINT 1 / X\n", "", &[]).unwrap();
    assert_eq!(run.exit_code, Some(1));
    assert_eq!(run.lines(), vec!["before"]);
    assert!(
        run.stderr.contains("?Division by zero in 3"),
        "{:?}",
        run.stderr
    );
}

/// A program that does not compile is reported as `xbasic64` reports it,
/// and nothing runs.
#[test]
fn test_run_of_a_bad_program_reports_it() {
    let run = run_jit("PRINT \"x\"\nGOTO 999\n", "", &[]).unwrap();
    assert_eq!(run.exit_code, Some(1));
    assert_eq!(run.stdout, "");
    assert!(run.stderr.contains("error"), "{:?}", run.stderr);
}

/// INPUT reads the process's stdin; DATA, READ and RESTORE; ON ERROR and
/// RESUME NEXT; GOSUB; `--unsafe`.
#[test]
fn test_run_of_the_runtime_parts() {
    let run = run_jit("INPUT \"n\"; N, S$\nPRINT N * 2; S$\n", "21, hi\n", &[]).unwrap();
    run.assert_ran_to_completion("INPUT");
    assert!(run.stdout.contains(" 42 hi"), "{:?}", run.stdout);

    let src = "DATA 1, \"two\", 3\nREAD A, B$, C\nRESTORE\nREAD D\nPRINT A; B$; C; D\n\
               ON ERROR GOTO 90\nX = 0: Y = 1 / X: PRINT \"next\"\nGOSUB 80\nEND\n\
               80 PRINT \"sub\": RETURN\n90 PRINT \"trap\"; ERR: RESUME NEXT\n";
    let run = run_jit(src, "", &[]).unwrap();
    run.assert_ran_to_completion("DATA, ON ERROR and GOSUB");
    assert_eq!(run.lines(), vec!["1 two 3  1", "trap 11", "next", "sub"]);

    let run = run_jit("DIM A(3)\nA(2) = 5\nPRINT A(2)\n", "", &["--unsafe"]).unwrap();
    run.assert_ran_to_completion("--unsafe");
    assert_eq!(run.lines(), vec!["5"]);
}

/// Files a program opens by name are in its working directory.
#[test]
fn test_run_opens_files_where_it_runs() {
    let dir = tempfile::TempDir::new().unwrap();
    let src = "OPEN \"t.txt\" FOR OUTPUT AS #1\nPRINT #1, 42\nCLOSE #1\n\
               OPEN \"t.txt\" FOR INPUT AS #1\nINPUT #1, N\nCLOSE #1\nPRINT N + 1\n";
    let run = run_jit_in(&dir, src, "", &[]).unwrap();
    run.assert_ran_to_completion("files");
    assert_eq!(run.lines(), vec!["43"]);
    assert!(dir.path().join("t.txt").exists());
}

/// A function too large for the optimizing register allocator goes through
/// the cheap ISA and `define_function_bytes`, which the JIT takes too.
#[test]
fn test_run_of_a_huge_function() {
    let mut src = String::new();
    for i in 0..5000 {
        let (a, b) = (i % 7, (i + 1) % 7);
        src.push_str(&format!(
            "A{a} = A{b} + {i}: IF A{a} > 1000 THEN A{a} = A{a} - 1000\n"
        ));
    }
    src.push_str("PRINT A0; A1; A2; A3; A4; A5; A6\n");
    let jit = run_jit(&src, "", &[]).unwrap();
    jit.assert_ran_to_completion("a huge function");
    let aot = compile_and_run_raw(&src, "").unwrap();
    assert_eq!(jit.stdout, aot.stdout);
}

/// The megatest, run both ways, prints the same.
#[test]
fn test_run_agrees_with_an_executable_on_the_megatest() {
    let mega = include_str!("../megatest/mega.bas");
    let (aot, _tmp) = compile_and_run_with_files(mega, |_| Ok(())).unwrap();
    let jit = run_jit(mega, "", &[]).unwrap();
    jit.assert_ran_to_completion("the megatest");
    assert_eq!(jit.stdout, aot);
    assert!(!aot.contains("FAIL "), "{aot}");
}
