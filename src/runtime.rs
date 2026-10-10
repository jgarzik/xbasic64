//! The runtime library, as an object file built with the compiler
//!
//! `build.rs` builds the Rust runtime in `rt/` for the target once, into one
//! object; the compiler writes it out beside each program's own and links
//! the two. Helpers a program never calls are dropped at link time: each is
//! in a section of its own.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

/// The runtime for the target this compiler was built for.
pub const OBJECT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rt.o"));

// nm is how the object is inspected, and Linux is where it is sure to be.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::process::Command;

    /// The runtime needs nothing but the C library: no part of `core` -- a
    /// panic path the build did not merge in -- since a program could not
    /// link, and nothing of a program, so that it can run one compiled into
    /// the compiler's own process.
    #[test]
    fn test_runtime_needs_only_the_c_library() {
        let dir = tempfile::tempdir().unwrap();
        let obj = dir.path().join("rt.o");
        std::fs::write(&obj, OBJECT).unwrap();
        let out = match Command::new("nm").arg("-u").arg(&obj).output() {
            Ok(out) => out,
            Err(_) => return, // no nm here: nothing to check with
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let foreign: Vec<&str> = text
            .lines()
            .filter_map(|l| l.split_whitespace().last())
            .filter(|s| s.starts_with("_R") || s.starts_with("_ZN") || s.contains("core"))
            .collect();
        assert!(foreign.is_empty(), "rt.o needs Rust symbols: {foreign:?}");
        // Nothing of a program: DATA is handed over by `_rt_data_init`.
        assert!(!text.contains("_data_"), "{text}");
    }
}
