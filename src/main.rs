//! BASIC-to-x86_64 Compiler
//!
//! Compiles 1980s-era BASIC programs to Linux x86-64 executables.

mod codegen;
mod lexer;
mod parser;
mod runtime;

use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <source.bas> [-o output]", args[0]);
        eprintln!("       {} -S <source.bas>  # emit assembly only", args[0]);
        std::process::exit(1);
    }

    let mut input_file = None;
    let mut output_file = None;
    let mut asm_only = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                if i < args.len() {
                    output_file = Some(args[i].clone());
                }
            }
            "-S" => {
                asm_only = true;
            }
            arg if arg.starts_with('-') => {
                eprintln!("Unknown option: {}", arg);
                std::process::exit(1);
            }
            _ => {
                input_file = Some(args[i].clone());
            }
        }
        i += 1;
    }

    let input_file = match input_file {
        Some(f) => f,
        None => {
            eprintln!("Error: No input file specified");
            std::process::exit(1);
        }
    };

    // Read source file
    let source = match fs::read_to_string(&input_file) {
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
    let mut codegen = codegen::CodeGen::new();
    let asm = codegen.generate(&program);

    // Add runtime
    let runtime_asm = runtime::generate_runtime();

    let full_asm = format!("{}\n{}", asm, runtime_asm);

    // Determine output file names - put temp files next to output
    let input_path = Path::new(&input_file);
    let stem = input_path.file_stem().unwrap().to_str().unwrap();
    let input_dir = input_path.parent().unwrap_or(Path::new("."));

    let exe_file = output_file.unwrap_or_else(|| {
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

    if asm_only {
        println!("Assembly written to {}", asm_file);
        return;
    }

    // Assemble
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

    // Link - use appropriate flags for the platform
    #[allow(unused_mut)] // mut needed on Linux for -no-pie
    let mut cc_args = vec!["-o", &exe_file, &obj_file, "-lm"];

    // Add -no-pie on Linux to avoid PIE issues
    #[cfg(target_os = "linux")]
    cc_args.push("-no-pie");

    let cc_status = Command::new("cc").args(&cc_args).status();

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
