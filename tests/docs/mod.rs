//! The documentation's own examples, compiled.
//!
//! Every ```basic block in LANGREF.md and README.md is extracted and compiled.
//! This is the check that would have caught the original defects: the reference
//! documented string DATA, string parameters, `LINE INPUT #`, bare `CLOSE`,
//! typed arrays and named labels, and every one of those examples failed --
//! because nothing ever ran them.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::common::compile_only;

/// Extract the contents of every ```basic fenced block in `markdown`.
fn basic_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;

    for line in markdown.lines() {
        match &mut current {
            None => {
                if line.trim_start().starts_with("```basic") {
                    current = Some(String::new());
                }
            }
            Some(body) => {
                if line.trim_start().starts_with("```") {
                    blocks.push(std::mem::take(body));
                    current = None;
                } else {
                    body.push_str(line);
                    body.push('\n');
                }
            }
        }
    }
    blocks
}

/// Compile every example in one documentation file.
fn check_doc(path: &str) {
    let markdown =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("could not read {path}: {e}"));
    let blocks = basic_blocks(&markdown);
    assert!(
        !blocks.is_empty(),
        "{path} has no ```basic examples; the extractor is probably broken"
    );

    let mut failures = Vec::new();
    for (i, source) in blocks.iter().enumerate() {
        if let Err(e) = compile_only(source) {
            failures.push(format!(
                "--- {path} example {i} ---\n{source}--- rejected with ---\n{}",
                e.stderr
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} examples in {path} do not compile:\n\n{}",
        failures.len(),
        blocks.len(),
        failures.join("\n")
    );
}

#[test]
fn test_langref_examples_compile() {
    check_doc("LANGREF.md");
}

#[test]
fn test_readme_examples_compile() {
    check_doc("README.md");
}

/// The extractor itself, so a silent change in fence handling cannot make the
/// checks above vacuous.
#[test]
fn test_block_extraction() {
    let md = "text\n```basic\nPRINT 1\n```\nmore\n```\nnot basic\n```\n```basic\nPRINT 2\n```\n";
    assert_eq!(basic_blocks(md), vec!["PRINT 1\n", "PRINT 2\n"]);
}
