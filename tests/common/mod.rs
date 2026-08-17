//! Common test utilities for integration tests
//!
//! On line endings: the Win64 runtime writes CRLF and the System V one writes
//! LF, but `str::lines()` splits on `\n` and drops a trailing `\r`, so an
//! assertion that goes through `lines()` is already platform-independent and
//! needs no normalizing helper. Comparing a whole multi-line block against a
//! literal would not be -- write those as per-line assertions.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::TempDir;

/// Result of running a compiled program, regardless of how it exited.
///
/// Unlike [`compile_and_run`], this preserves output produced *before* an abort
/// and exposes the numeric exit code, so tests can assert on programs that are
/// expected to fail at runtime (bounds violations, division by zero, ...).
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub stdout: String,
    pub stderr: String,
    /// `None` if the process was terminated by a signal.
    pub exit_code: Option<i32>,
}

impl RunOutput {
    /// Lines of stdout, each trimmed, for the common line-by-line style.
    ///
    /// Per line rather than once over the whole string, because a number
    /// carries GW-BASIC's spacing: a blank where the sign would go and a blank
    /// after, so `PRINT 42` writes " 42 ". A test asserting what a program
    /// *computed* should not have to spell those out; the ones that assert on
    /// the spacing itself read `stdout` directly, and say so.
    pub fn lines(&self) -> Vec<&str> {
        lines(&self.stdout)
    }

    /// Panic unless the program ran to completion.
    ///
    /// [`compile_and_run_raw`] deliberately returns `Ok` whatever the program
    /// did, which makes a crash look like truncated output -- and an assertion
    /// phrased as "this must not appear" then passes *because* the program
    /// died. The CLS test was written that way and stayed green on Windows
    /// while the program it ran was aborting with an access violation.
    ///
    /// Any test that reads stdout for what a statement produced wants this
    /// too. The exit code is printed in hex: Windows says what went wrong in
    /// it, and 0xC0000005 is not a number the compiler ever chooses.
    pub fn assert_ran_to_completion(&self, what: &str) {
        assert_eq!(
            self.exit_code,
            Some(0),
            "{what} did not run to completion: exit {:?} (0x{:08X}), stdout {:?}, stderr {:?}",
            self.exit_code,
            self.exit_code.unwrap_or(-1),
            self.stdout,
            self.stderr
        );
    }
}

/// Lines of `text`, each trimmed, ignoring blank ones at the ends.
///
/// The counterpart of [`RunOutput::lines`] for the helpers that return a bare
/// String. See there for why the trim is per line.
pub fn lines(text: &str) -> Vec<&str> {
    text.trim().lines().map(str::trim).collect()
}

/// A failure of the compiler itself (lexer, parser, sema, codegen, assembler, linker).
#[derive(Debug, Clone)]
pub struct CompileError {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

impl CompileError {
    /// Compiler diagnostics go to stderr; this is what message assertions use.
    pub fn contains(&self, needle: &str) -> bool {
        self.stderr.contains(needle) || self.stdout.contains(needle)
    }

    /// True if the compiler rejected the program cleanly rather than panicking.
    ///
    /// A Rust panic exits with 101, so this distinguishes "diagnosed and
    /// refused" from "crashed". Tests that assert a program is rejected should
    /// also assert *how*, since turning panics into diagnostics is a goal.
    pub fn is_clean_rejection(&self) -> bool {
        self.exit_code == Some(1)
    }
}

/// Compile only; returns `Err` with the diagnostics if the compiler rejects the program.
///
/// Use this for "this program must be rejected, with this message" tests.
pub fn compile_only(source: &str) -> Result<(), CompileError> {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let bas_file = tmp.path().join("test.bas");
    let exe_file = tmp.path().join("test");
    fs::write(&bas_file, source).expect("failed to write source");

    let out = Command::new(env!("CARGO_BIN_EXE_xbasic64"))
        .arg(&bas_file)
        .arg("-o")
        .arg(&exe_file)
        .output()
        .expect("failed to run compiler");

    if out.status.success() {
        Ok(())
    } else {
        Err(CompileError {
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            exit_code: out.status.code(),
        })
    }
}

/// Compile only, with extra compiler flags.
///
/// For the combinations a flag makes illegal -- `--unsafe` removes the checks
/// `ON ERROR` exists to trap, so the two together are refused.
pub fn compile_only_flags(source: &str, flags: &[&str]) -> Result<(), CompileError> {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let bas_file = tmp.path().join("test.bas");
    let exe_file = tmp.path().join("test");
    fs::write(&bas_file, source).expect("failed to write source");

    let out = Command::new(env!("CARGO_BIN_EXE_xbasic64"))
        .arg(&bas_file)
        .args(flags)
        .arg("-o")
        .arg(&exe_file)
        .output()
        .expect("failed to run compiler");

    if out.status.success() {
        Ok(())
    } else {
        Err(CompileError {
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            exit_code: out.status.code(),
        })
    }
}

/// Compile and run, returning `Ok` **regardless of the program's exit status**.
///
/// `Err` is reserved for compilation failure. This is the primitive the other
/// run helpers are built on.
pub fn compile_and_run_raw(source: &str, stdin_input: &str) -> Result<RunOutput, String> {
    compile_and_run_flags(source, stdin_input, &[])
}

/// The same, with extra compiler flags.
///
/// Used to exercise `--unsafe`, whose whole effect is on the code emitted, so
/// nothing else in the suite would notice if it stopped working.
pub fn compile_and_run_flags(
    source: &str,
    stdin_input: &str,
    flags: &[&str],
) -> Result<RunOutput, String> {
    let tmp = TempDir::new().map_err(|e| e.to_string())?;
    let bas_file = tmp.path().join("test.bas");
    let exe_file = tmp.path().join("test");

    fs::write(&bas_file, source).map_err(|e| e.to_string())?;

    let compile_output = Command::new(env!("CARGO_BIN_EXE_xbasic64"))
        .arg(&bas_file)
        .args(flags)
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

    let mut child = Command::new(&exe_file)
        // Run inside the temp dir, so a program that opens a file by a bare
        // name writes it there and it dies with the TempDir. Without this the
        // program inherited the test runner's directory -- the repository root
        // -- and `OPEN "a.txt" FOR OUTPUT` left a stray file behind on every
        // run, two of which were committed by accident.
        .current_dir(tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run executable: {}", e))?;

    // Always write (even an empty string) so that programs reading stdin see a
    // clean EOF rather than an inherited terminal.
    child
        .stdin
        .as_mut()
        .expect("stdin was piped")
        .write_all(stdin_input.as_bytes())
        .map_err(|e| format!("Failed to write to stdin: {}", e))?;

    let run_output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for executable: {}", e))?;

    Ok(RunOutput {
        stdout: String::from_utf8_lossy(&run_output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&run_output.stderr).to_string(),
        exit_code: run_output.status.code(),
    })
}

pub fn compile_and_run(source: &str) -> Result<String, String> {
    compile_and_run_with_stdin(source, "")
}

pub fn compile_and_run_with_stdin(source: &str, stdin_input: &str) -> Result<String, String> {
    let run = compile_and_run_raw(source, stdin_input)?;

    if run.exit_code != Some(0) {
        return Err(format!(
            "Execution failed with exit code {:?}:\nstderr: {}",
            run.exit_code, run.stderr
        ));
    }

    Ok(run.stdout)
}

/// Compile to assembly and return the *generated* portion of it.
///
/// The runtime is concatenated onto every program, and it is thousands of
/// hand-written instructions containing an example of nearly everything. A
/// shape assertion that searched the whole file would be satisfied, or broken,
/// by code the compiler did not emit -- so the text is cut at the runtime's
/// banner and only what precedes it is returned.
///
/// `-S` writes the assembly beside the output path and stops before the
/// assembler, so nothing here depends on `as` or `cc` being able to run.
pub fn compile_to_asm(source: &str) -> Result<String, String> {
    const RUNTIME_BANNER: &str = "# BASIC Runtime Library";

    let tmp = TempDir::new().map_err(|e| e.to_string())?;
    let bas_file = tmp.path().join("test.bas");
    let out_file = tmp.path().join("test");

    fs::write(&bas_file, source).map_err(|e| e.to_string())?;

    let compile_output = Command::new(env!("CARGO_BIN_EXE_xbasic64"))
        .arg("-S")
        .arg(&bas_file)
        .arg("-o")
        .arg(&out_file)
        .output()
        .map_err(|e| format!("Failed to run compiler: {}", e))?;

    if !compile_output.status.success() {
        return Err(format!(
            "Compilation failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&compile_output.stdout),
            String::from_utf8_lossy(&compile_output.stderr)
        ));
    }

    let asm = fs::read_to_string(tmp.path().join("test.s")).map_err(|e| e.to_string())?;

    Ok(match asm.find(RUNTIME_BANNER) {
        Some(cut) => asm[..cut].to_string(),
        None => return Err("the runtime banner is missing from the emitted assembly".to_string()),
    })
}

/// Helper to compile and run with access to temp directory for file I/O tests
pub fn compile_and_run_with_files<F>(source: &str, setup: F) -> Result<(String, TempDir), String>
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
    let compile_output = Command::new(env!("CARGO_BIN_EXE_xbasic64"))
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
