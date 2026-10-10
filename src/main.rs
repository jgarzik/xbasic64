//! BASIC-to-x86_64 Compiler
//!
//! Compiles 1980s-era BASIC programs to x86-64 executables.
//! Code generation is Cranelift, which writes the object file; the program is
//! linked with `cc` on Linux and MSVC `link.exe` on Windows.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

mod codegen;
mod lexer;
mod parser;
mod rtsig;
mod runtime;
mod sema;

use clap::Parser;
use std::fs;
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

    /// Write the program's Cranelift IR to `<output>.clif` instead of
    /// compiling it. The runtime is prebuilt, so it is not included.
    #[arg(short = 'S')]
    asm_only: bool,

    /// Disable runtime safety checks (array bounds, division by zero, overflow)
    ///
    /// Named `no_checks` because `unsafe` is a Rust keyword.
    #[arg(long = "unsafe")]
    no_checks: bool,
}

/// Format a diagnostic position as `file:line`, or just `file` when the line is
/// unknown. GCC-style, so editors and IDEs can parse it.
fn locate(file: &str, line: u32) -> String {
    if line == 0 {
        file.to_string()
    } else {
        format!("{}:{}", file, line)
    }
}

/// Print a batch of diagnostics and exit.
///
/// Shared by the parser and by sema so the two stages report identically: both
/// now produce a list rather than a single error, and a program with several
/// mistakes should not look different depending on which pass found them.
fn report(file: &str, diagnostics: &[(u32, String, Option<String>)]) -> ! {
    for (line, message, note) in diagnostics {
        eprintln!("{}: error: {}", locate(file, *line), message);
        if let Some(note) = note {
            eprintln!("{}: note: {}", locate(file, *line), note);
        }
    }
    let n = diagnostics.len();
    eprintln!("xbasic64: {} error{}", n, if n == 1 { "" } else { "s" });
    std::process::exit(1);
}

/// Whether a failed Windows link looks like GNU coreutils' `link` rather than
/// the MSVC linker.
///
/// `link.exe` is ambiguous on Windows: Git for Windows, MSYS and Cygwin all
/// ship coreutils' `link`, which shadows the MSVC linker whenever their
/// `usr/bin` leads the PATH. It rejects the very first MSVC flag it sees, and
/// says nothing about why -- so the message is matched here and explained.
fn looks_like_coreutils_link(stderr: &str) -> bool {
    stderr.contains("extra operand") || stderr.contains("Try 'link --help'")
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
            eprintln!("{}: error: {}", locate(input_file, lexer.current_line()), e);
            std::process::exit(1);
        }
    };
    let line_map = lexer.line_map().to_vec();

    // Parse
    let mut parser = parser::Parser::new(tokens, line_map);
    let mut program = match parser.parse() {
        Ok(p) => p,
        Err(errors) => report(
            input_file,
            &errors
                .iter()
                .map(|e| (e.line, e.to_string(), e.error.note().map(str::to_string)))
                .collect::<Vec<_>>(),
        ),
    };

    // Semantic analysis: reject bad programs here, with a source line, rather
    // than letting them reach codegen and become a panic or a linker error.
    let (symbols, diagnostics) = sema::analyze(&mut program, !args.no_checks);
    for d in diagnostics.iter().filter(|d| d.warning) {
        eprintln!("{}: warning: {}", locate(input_file, d.line), d.message);
        if let Some(note) = &d.note {
            eprintln!("{}: note: {}", locate(input_file, d.line), note);
        }
    }
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| !d.warning)
        .map(|d| (d.line, d.message.clone(), d.note.clone()))
        .collect();
    if !errors.is_empty() {
        report(input_file, &errors);
    }

    // Generate code
    let opts = codegen::Options {
        checks: !args.no_checks,
    };
    let output = match codegen::generate(&program, symbols, opts, args.asm_only) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{}: error: {}", input_file, e);
            std::process::exit(1);
        }
    };

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
    let obj_file = exe_dir
        .join(format!("{}.o", exe_stem))
        .to_string_lossy()
        .to_string();
    let rt_file = exe_dir
        .join(format!("{}.rt.o", exe_stem))
        .to_string_lossy()
        .to_string();

    if args.asm_only {
        let clif_file = exe_dir
            .join(format!("{}.clif", exe_stem))
            .to_string_lossy()
            .to_string();
        if let Err(e) = fs::write(&clif_file, output.clif.as_bytes()) {
            eprintln!("Error writing {}: {}", clif_file, e);
            std::process::exit(1);
        }
        println!("Cranelift IR written to {}", clif_file);
        return;
    }

    if let Err(e) = fs::write(&obj_file, &output.object) {
        eprintln!("Error writing the object file: {}", e);
        std::process::exit(1);
    }
    // The runtime, built when the compiler was built.
    if let Err(e) = fs::write(&rt_file, runtime::OBJECT) {
        eprintln!("Error writing the runtime object: {}", e);
        std::process::exit(1);
    }

    // Link - Windows uses link.exe with UCRT, others use cc
    // msvcrt.lib provides CRT startup (mainCRTStartup) and imports CRT DLL
    //
    // The output is captured rather than inherited so that a wrong `link.exe`
    // can be recognised; see below.
    #[cfg(windows)]
    let cc_output = Command::new("link.exe")
        .args([
            &format!("/OUT:{}", exe_file),
            &obj_file,
            &rt_file,
            "/SUBSYSTEM:CONSOLE",
            // Discard the runtime helpers this program never calls. rustc
            // emits each into a COMDAT section of its own;
            // /OPT:REF is the default for a non-/DEBUG link, but the whole
            // point is lost if that ever stops being true.
            "/OPT:REF",
            "/DEFAULTLIB:msvcrt.lib",
            "/DEFAULTLIB:ucrt.lib",
            "/DEFAULTLIB:kernel32.lib",
            "/DEFAULTLIB:legacy_stdio_definitions.lib",
        ])
        .output();

    // --gc-sections drops the runtime helpers this program never calls:
    // rustc emits each function into a section of its own, which is what the
    // flag works with, since a linker discards unreferenced sections, never
    // unreferenced labels. `PRINT "hi"` calls three of the 85 helpers.
    #[cfg(not(windows))]
    let cc_output = Command::new("cc")
        .args([
            "-o",
            &exe_file,
            &obj_file,
            &rt_file,
            "-lm",
            "-no-pie",
            "-Wl,--gc-sections",
        ])
        .output();

    match cc_output {
        Ok(out) if out.status.success() => {
            // A successful link can still have something to say.
            eprint!("{}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            eprint!("{}", stderr);
            eprint!("{}", String::from_utf8_lossy(&out.stdout));

            if cfg!(windows) && looks_like_coreutils_link(&stderr) {
                eprintln!(
                    "xbasic64: 'link.exe' appears to be GNU coreutils' link, \
                     not the MSVC linker"
                );
                eprintln!(
                    "xbasic64: note: run from a Developer Command Prompt, or put \
                     the MSVC tools ahead of Git/MSYS usr/bin on PATH"
                );
            }

            eprintln!("Linker failed with status: {}", out.status);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to run linker: {}", e);
            std::process::exit(1);
        }
    }

    // Clean up temporary files
    let _ = fs::remove_file(&obj_file);
    let _ = fs::remove_file(&rt_file);

    println!("Compiled {} -> {}", input_file, exe_file);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real message coreutils prints when handed MSVC's flags, captured
    /// from a Windows CI run.
    #[test]
    fn test_recognizes_coreutils_link() {
        let coreutils = "link: extra operand '/SUBSYSTEM:CONSOLE'\n\
                         Try 'link --help' for more information.\n";
        assert!(looks_like_coreutils_link(coreutils));
    }

    /// A genuine MSVC linker error must not be mistaken for it, or the hint
    /// would send people chasing their PATH over an unrelated failure.
    #[test]
    fn test_does_not_mistake_a_real_linker_error() {
        let msvc = "test.obj : error LNK2019: unresolved external symbol memcpy\n\
                    test.exe : fatal error LNK1120: 1 unresolved externals\n";
        assert!(!looks_like_coreutils_link(msvc));
    }
}
