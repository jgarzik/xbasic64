//! BASIC-to-x86_64 Compiler
//!
//! Compiles 1980s-era BASIC programs to x86-64 executables.
//! Supports Linux, macOS, and Windows (MinGW).

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

mod abi;
mod codegen;
mod lexer;
mod parser;
mod runtime;

use clap::Parser;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

/// BASIC-to-x86_64 compiler
#[derive(Parser)]
#[command(name = "xbasic64")]
#[command(about = "Compiles 1980s-era BASIC programs to x86-64 executables")]
struct Args {
    /// Input BASIC source file
    input: String,

    /// Output file name
    #[arg(short, long)]
    output: Option<String>,

    /// Emit assembly only (don't assemble or link)
    #[arg(short = 'S')]
    asm_only: bool,
}

fn main() {
    let args = Args::parse();

    let input_file = &args.input;

    // Read source file
    let source = match fs::read_to_string(input_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", input_file, e);
            std::process::exit(1);
        }
    };

    // Tokenize
    let mut lexer = lexer::Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Lexer error: {}", e);
            std::process::exit(1);
        }
    };

    // Parse
    let mut parser = parser::Parser::new(tokens);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Parse error: {}", e);
            std::process::exit(1);
        }
    };

    // Generate code
    let mut codegen = codegen::CodeGen::default();
    let asm = codegen.generate(&program);

    // Add runtime
    let runtime_asm = runtime::generate_runtime();

    let full_asm = format!("{}\n{}", asm, runtime_asm);

    // Determine output file names - put temp files next to output
    let input_path = Path::new(&input_file);
    let stem = input_path.file_stem().unwrap().to_str().unwrap();
    let input_dir = input_path.parent().unwrap_or(Path::new("."));

    let exe_file = args.output.unwrap_or_else(|| {
        if cfg!(windows) {
            input_dir
                .join(format!("{}.exe", stem))
                .to_string_lossy()
                .to_string()
        } else {
            input_dir.join(stem).to_string_lossy().to_string()
        }
    });

    // Put temp files next to the executable
    let exe_path = Path::new(&exe_file);
    let exe_dir = exe_path.parent().unwrap_or(Path::new("."));
    let exe_stem = exe_path.file_stem().unwrap().to_str().unwrap();
    let asm_file = exe_dir
        .join(format!("{}.s", exe_stem))
        .to_string_lossy()
        .to_string();
    let obj_file = exe_dir
        .join(format!("{}.o", exe_stem))
        .to_string_lossy()
        .to_string();

    // Write assembly
    match fs::File::create(&asm_file) {
        Ok(mut f) => {
            if let Err(e) = f.write_all(full_asm.as_bytes()) {
                eprintln!("Error writing assembly: {}", e);
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error creating assembly file: {}", e);
            std::process::exit(1);
        }
    }

    if args.asm_only {
        println!("Assembly written to {}", asm_file);
        return;
    }

    // Assemble - use clang on Windows, GNU as elsewhere
    #[cfg(windows)]
    let as_status = Command::new("clang")
        .args(["-c", "-o", &obj_file, &asm_file])
        .status();

    #[cfg(not(windows))]
    let as_status = Command::new("as")
        .args(["-o", &obj_file, &asm_file])
        .status();

    match as_status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            eprintln!("Assembler failed with status: {}", status);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to run assembler: {}", e);
            std::process::exit(1);
        }
    }

    // Link - Windows uses link.exe with UCRT, others use cc
    // msvcrt.lib provides CRT startup (mainCRTStartup) and imports CRT DLL
    #[cfg(windows)]
    let cc_status = Command::new("link.exe")
        .args([
            &format!("/OUT:{}", exe_file),
            &obj_file,
            "/SUBSYSTEM:CONSOLE",
            "/DEFAULTLIB:msvcrt.lib",
            "/DEFAULTLIB:ucrt.lib",
            "/DEFAULTLIB:kernel32.lib",
            "/DEFAULTLIB:legacy_stdio_definitions.lib",
        ])
        .status();

    #[cfg(not(windows))]
    let cc_status = {
        #[allow(unused_mut)]
        let mut cc_args = vec!["-o", &exe_file, &obj_file, "-lm"];

        #[cfg(target_os = "linux")]
        cc_args.push("-no-pie");

        Command::new("cc").args(&cc_args).status()
    };

    match cc_status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            eprintln!("Linker failed with status: {}", status);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to run linker: {}", e);
            std::process::exit(1);
        }
    }

    // Clean up temporary files
    let _ = fs::remove_file(&asm_file);
    let _ = fs::remove_file(&obj_file);

    println!("Compiled {} -> {}", input_file, exe_file);
}
