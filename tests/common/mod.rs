//! Common test utilities for integration tests

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::TempDir;

pub fn compile_and_run(source: &str) -> Result<String, String> {
    compile_and_run_with_stdin(source, "")
}

pub fn compile_and_run_with_stdin(source: &str, stdin_input: &str) -> Result<String, String> {
    let tmp = TempDir::new().map_err(|e| e.to_string())?;
    let bas_file = tmp.path().join("test.bas");
    let exe_file = tmp.path().join("test");

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
