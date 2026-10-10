//! Assemble the runtime library once, when the compiler is built
//!
//! It used to be appended to every program's assembly and assembled again on
//! every compile. Now it is assembled here for the target, into
//! `$OUT_DIR/runtime.o`, which the compiler embeds and links beside each
//! program's own object.
//!
//! The assembler is the one the compiler has always used for the target:
//! GNU `as` for ELF, `clang -c` for COFF. So building the compiler needs it,
//! as running the compiler always did.
//!
//! The Rust runtime in `rt/` is built here too, with rustc, as a `no_std`
//! staticlib for the target, emitted as the one object fat LTO makes of it
//! and the parts of `core` it uses (`$OUT_DIR/rt.o`). It is not linked into
//! programs yet: it is being ported from the assembly, which it will replace
//! in one switch.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

#[path = "src/rtbuild.rs"]
mod rtbuild;

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/rtbuild.rs");
    println!("cargo:rerun-if-changed=src/runtime");
    println!("cargo:rerun-if-changed=rt/src");

    let os = env::var("CARGO_CFG_TARGET_OS").expect("cargo sets CARGO_CFG_TARGET_OS");
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let asm = out.join("runtime.s");
    let obj = out.join("runtime.o");

    std::fs::write(&asm, rtbuild::emit(rtbuild::for_target(&os))).expect("OUT_DIR is writable");

    let mut cmd = if os == "windows" {
        let mut c = Command::new("clang");
        c.arg("-c");
        c
    } else {
        Command::new("as")
    };
    let output = cmd
        .arg("-o")
        .arg(&obj)
        .arg(&asm)
        .output()
        .unwrap_or_else(|e| panic!("cannot run the assembler for the runtime: {e}"));
    assert!(
        output.status.success(),
        "the runtime does not assemble:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    build_rt(&out);
}

/// Build `rt/` for the target as one object: a staticlib, with fat LTO
/// merging it and what it uses of `core` into a single unit, emitted as an
/// object. The staticlib archive would also carry all of `compiler_builtins`
/// -- 4 MB the runtime does not reference.
fn build_rt(out: &std::path::Path) {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let target = env::var("TARGET").expect("cargo sets TARGET");
    let obj = out.join("rt.o");
    let output = Command::new(rustc)
        .args(["--edition", "2024", "--crate-type", "staticlib"])
        .args(["--crate-name", "xbasic64_rt", "--target", &target])
        .args(["-C", "opt-level=3", "-C", "panic=abort", "-C", "lto=fat"])
        .args(["-C", "codegen-units=1", "-C", "debug-assertions=off"])
        .args(["-C", "overflow-checks=off", "--emit", "obj"])
        .arg("-o")
        .arg(&obj)
        .arg("rt/src/lib.rs")
        .output()
        .unwrap_or_else(|e| panic!("cannot run rustc for the runtime: {e}"));
    assert!(
        output.status.success(),
        "the runtime does not build:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
