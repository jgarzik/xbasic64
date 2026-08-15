//! Common test utilities for integration tests

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
    /// Lines of stdout, trimmed, for the common line-by-line assertion style.
    pub fn lines(&self) -> Vec<&str> {
        self.stdout.trim().lines().collect()
    }
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

/// Compile and run, returning `Ok` **regardless of the program's exit status**.
///
/// `Err` is reserved for compilation failure. This is the primitive the other
/// run helpers are built on.
pub fn compile_and_run_raw(source: &str, stdin_input: &str) -> Result<RunOutput, String> {
    let tmp = TempDir::new().map_err(|e| e.to_string())?;
    let bas_file = tmp.path().join("test.bas");
    let exe_file = tmp.path().join("test");

    fs::write(&bas_file, source).map_err(|e| e.to_string())?;

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

    let mut child = Command::new(&exe_file)
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

/// Normalize line endings for cross-platform test assertions (CRLF -> LF)
pub fn normalize_output(s: &str) -> String {
    s.trim().replace("\r\n", "\n")
}
