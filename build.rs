//! Build the runtime library once, when the compiler is built
//!
//! The runtime is the Rust crate in `rt/`, `no_std`, one source for every
//! target. It is built here with rustc for the target, as a staticlib, and
//! the one object fat LTO makes of it and the parts of `core` it uses is
//! kept as `$OUT_DIR/rt.o`, which the compiler embeds and links beside each
//! program's own object.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=rt/src");
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    build_rt(&out);
}

/// Build `rt/` for the target as one object, `$OUT_DIR/rt.o`.
///
/// rustc builds a staticlib, with fat LTO merging the crate and every part
/// of `core` it uses -- its panic paths included -- into one object, the
/// archive's first member. The rest of the archive is `compiler_builtins`,
/// 4 MB the runtime does not reference, so only that member is kept.
/// (`--emit obj` would give the crate's object without `core` merged in,
/// leaving a bounds check's panic an undefined symbol.)
fn build_rt(out: &std::path::Path) {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let target = env::var("TARGET").expect("cargo sets TARGET");
    let lib = out.join("rt.a");
    // rustc is run directly, so rt/Cargo.toml is not read: the edition here
    // must be the one declared there, which clippy and the unit tests use.
    let output = Command::new(rustc)
        .args(["--edition", "2024", "--crate-type", "staticlib"])
        .args(["--crate-name", "xbasic64_rt", "--target", &target])
        .args(["-C", "opt-level=3", "-C", "panic=abort", "-C", "lto=fat"])
        .args(["-C", "codegen-units=1", "-C", "debug-assertions=off"])
        .args(["-C", "overflow-checks=off"])
        .arg("-o")
        .arg(&lib)
        .arg("rt/src/lib.rs")
        .output()
        .unwrap_or_else(|e| panic!("cannot run rustc for the runtime: {e}"));
    assert!(
        output.status.success(),
        "the runtime does not build:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let archive = std::fs::read(&lib).expect("rustc wrote the staticlib");
    let ours: Vec<(String, &[u8])> = archive_members(&archive)
        .into_iter()
        .filter(|(name, _)| name.contains("xbasic64_rt"))
        .collect();
    assert!(
        ours.len() == 1,
        "expected one LTO object for the runtime in its staticlib, found {:?}",
        ours.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
    std::fs::write(out.join("rt.o"), ours[0].1).expect("OUT_DIR is writable");
}

/// The members of a Unix `ar` archive -- the format rustc writes a
/// staticlib in on every target, `.lib` included -- by name: GNU and COFF
/// long names (`/N` into the `//` table) and BSD ones (`#1/N`) resolved, the
/// symbol tables skipped.
fn archive_members(a: &[u8]) -> Vec<(String, &[u8])> {
    assert!(a.starts_with(b"!<arch>\n"), "not an ar archive");
    let field = |h: &[u8], at: usize, len: usize| {
        String::from_utf8_lossy(&h[at..at + len])
            .trim_end()
            .to_string()
    };
    let mut members = Vec::new();
    let mut long_names: &[u8] = &[];
    let mut at = 8;
    while at + 60 <= a.len() {
        let header = &a[at..at + 60];
        let name = field(header, 0, 16);
        let size: usize = field(header, 48, 10).parse().expect("an ar member size");
        let mut data = &a[at + 60..at + 60 + size];
        at += 60 + size + (size & 1);
        let name = if name == "/" || name == "__.SYMDEF" || name.starts_with("__.SYMDEF ") {
            continue;
        } else if name == "//" {
            long_names = data;
            continue;
        } else if let Some(n) = name.strip_prefix("#1/") {
            let n: usize = n.parse().expect("a BSD long-name length");
            let full = String::from_utf8_lossy(&data[..n]);
            let full = full.trim_end_matches('\0').to_string();
            data = &data[n..];
            full
        } else if let Some(off) = name.strip_prefix('/') {
            // `/N` is a long name; anything else (`/<ECSYMBOLS>/`) is
            // another table.
            let Ok(off) = off.parse::<usize>() else {
                continue;
            };
            let rest = &long_names[off..];
            let end = rest
                .iter()
                .position(|&c| c == b'\n' || c == 0)
                .unwrap_or(rest.len());
            String::from_utf8_lossy(&rest[..end])
                .trim_end_matches('/')
                .to_string()
        } else {
            name.trim_end_matches('/').to_string()
        };
        members.push((name, data));
    }
    members
}
