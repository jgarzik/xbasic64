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
}
