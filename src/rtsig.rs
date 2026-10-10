//! Signatures of the runtime helpers that generated code calls
//!
//! The runtime is hand-written assembly, so nothing but its comments states what
//! a helper takes and returns. This table states it once, in a form a code
//! generator can use: a backend that builds calls from signatures (rather than
//! by placing values in registers itself) needs exactly this, and the `fallible`
//! flag tells any backend which calls an `ON ERROR` handler can interrupt.
//!
//! Every helper follows its platform's standard C ABI for the types listed, so
//! one signature serves both System V and Win64. A string result is a (pointer,
//! length) pair, which both ABIs return in `rax:rdx`.
//!
//! Integer arguments are all listed as `I64`, because codegen always fills the
//! whole register and some helpers read all of it; a helper that only reads
//! the low half still sees the right value. Integer *results* are listed at
//! the width the helper actually writes: a helper that sets only `eax` leaves
//! the top of `rax` undefined, and a caller must not read it as 64 bits.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

/// A machine-level argument or result type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    /// 32-bit integer in the low half of a general register.
    I32,
    /// 64-bit integer or pointer.
    I64,
    /// Double in an XMM register.
    F64,
}

use Ty::{F64, I32, I64};

/// One runtime helper as generated code sees it.
#[derive(Debug)]
pub struct RtHelper {
    pub name: &'static str,
    #[expect(
        dead_code,
        reason = "read by a backend that builds calls from signatures"
    )]
    pub params: &'static [Ty],
    #[expect(
        dead_code,
        reason = "read by a backend that builds calls from signatures"
    )]
    pub rets: &'static [Ty],
    /// Whether the helper can raise a trappable runtime error.
    ///
    /// Conservative: true when some path through the helper, in either
    /// runtime tree, reaches `_rt_error`. A test recomputes this from the
    /// assembly, so it cannot drift from the code.
    pub fallible: bool,
    /// Whether the helper never returns. Set for helpers that end the
    /// program; `_rt_error` is not one of them, since it returns when a
    /// handler traps the error.
    pub noreturn: bool,
}

const STR: &[Ty] = &[I64, I64];

const fn h(name: &'static str, params: &'static [Ty], rets: &'static [Ty]) -> RtHelper {
    RtHelper {
        name,
        params,
        rets,
        fallible: false,
        noreturn: false,
    }
}

const fn fallible(mut helper: RtHelper) -> RtHelper {
    helper.fallible = true;
    helper
}

const fn noreturn(mut helper: RtHelper) -> RtHelper {
    helper.noreturn = true;
    helper
}

/// Every helper generated code may call, by name.
pub static HELPERS: &[RtHelper] = &[
    // Program lifetime and errors
    h("_rt_platform_init", &[], &[]),
    noreturn(h("_rt_end", &[], &[])),
    noreturn(h("_rt_stop", &[I64], &[])),
    // (message, line). Returns only when a handler traps the error.
    h("_rt_error", &[I64, I64], &[]),
    noreturn(h("_rt_fatal", &[I64, I64], &[])),
    // (error number, line). Raises, so it returns only when a handler traps.
    fallible(h("_rt_error_num", &[I64, I64], &[])),
    h("_rt_err", &[], &[I32]),
    h("_rt_erl", &[], &[I32]),
    // Console
    h("_rt_input_string", &[], STR),
    fallible(h("_rt_input_line", &[I64, I64, I64, I64], &[])),
    h("_rt_input_field_num", &[I64], &[F64]),
    h("_rt_input_field_str", &[I64], STR),
    h("_rt_cls", &[], &[]),
    h("_rt_beep", &[], &[]),
    h("_rt_pos", &[], &[I32]),
    // (row, column)
    h("_rt_locate", &[I64, I64], &[]),
    // (foreground, background)
    h("_rt_color", &[I64, I64], &[]),
    // Output to a file number, 0 being the console
    // (file, pointer, length)
    fallible(h("_rt_file_print_string", &[I64, I64, I64], &[])),
    fallible(h("_rt_file_print_float", &[I64, F64], &[])),
    fallible(h("_rt_file_print_single", &[I64, F64], &[])),
    fallible(h("_rt_print_number", &[I64, F64], &[])),
    fallible(h("_rt_print_number_single", &[I64, F64], &[])),
    fallible(h("_rt_print_zone", &[I64], &[])),
    // (file, count) and (file, column)
    fallible(h("_rt_file_print_spc", &[I64, I64], &[])),
    fallible(h("_rt_file_print_tab", &[I64, I64], &[])),
    // (file, character code)
    fallible(h("_rt_file_print_char", &[I64, I64], &[])),
    fallible(h("_rt_file_print_newline", &[I64], &[])),
    // PRINT USING: (file, format pointer, format length), then each value
    h("_rt_using_start", &[I64, I64, I64], &[]),
    fallible(h("_rt_using_num", &[F64], &[])),
    fallible(h("_rt_using_str", &[I64, I64], &[])),
    fallible(h("_rt_using_end", &[], &[])),
    // Files
    // (name pointer, name length, mode, file)
    fallible(h("_rt_file_open", &[I64, I64, I64, I64], &[])),
    // (name pointer, name length, file, record length)
    fallible(h("_rt_file_open_random", &[I64, I64, I64, I64], &[])),
    h("_rt_file_close", &[I64], &[]),
    h("_rt_file_close_all", &[], &[]),
    fallible(h("_rt_width", &[I64, I64], &[])),
    fallible(h("_rt_file_input_string", &[I64], STR)),
    fallible(h("_rt_file_input_number", &[I64], &[F64])),
    fallible(h("_rt_file_line_input", &[I64], STR)),
    h("_rt_file_eof", &[I64], &[I32]),
    h("_rt_file_loc", &[I64], &[F64]),
    h("_rt_file_lof", &[I64], &[F64]),
    h("_rt_field_reset", &[I64], &[]),
    // (file, width) -> the field's place in the record buffer
    fallible(h("_rt_field_next", &[I64, I64], STR)),
    // (destination pointer, destination length, source pointer, source length)
    h("_rt_lset", &[I64, I64, I64, I64], &[]),
    h("_rt_rset", &[I64, I64, I64, I64], &[]),
    // (file, record, line)
    fallible(h("_rt_file_get", &[I64, I64, I64], &[])),
    fallible(h("_rt_file_put", &[I64, I64, I64], &[])),
    // (file, first record, last record, line)
    fallible(h("_rt_lock", &[I64, I64, I64, I64], &[])),
    fallible(h("_rt_unlock", &[I64, I64, I64, I64], &[])),
    // (value bits, width)
    h("_rt_mk", &[I64, I64], STR),
    // (pointer, length, line)
    fallible(h("_rt_cvi", &[I64, I64, I64], &[F64])),
    fallible(h("_rt_cvl", &[I64, I64, I64], &[F64])),
    fallible(h("_rt_cvs", &[I64, I64, I64], &[F64])),
    fallible(h("_rt_cvd", &[I64, I64, I64], &[F64])),
    // DATA
    fallible(h("_rt_read_number", &[I64], &[F64])),
    fallible(h("_rt_read_string", &[I64], STR)),
    h("_rt_restore", &[I64], &[]),
    fallible(h("_rt_array_autodim", &[I64, I64, I64, I64], &[])),
    // Strings
    fallible(h("_rt_val", STR, &[F64])),
    h("_rt_str", &[F64], STR),
    h("_rt_str_single", &[F64], STR),
    h("_rt_chr", &[I64], STR),
    h("_rt_space", &[I64], STR),
    h("_rt_hex", &[I64], STR),
    h("_rt_oct", &[I64], STR),
    // (count, character code)
    h("_rt_string_n", &[I64, I64], STR),
    h("_rt_ltrim", STR, STR),
    h("_rt_rtrim", STR, STR),
    h("_rt_ucase", STR, STR),
    h("_rt_lcase", STR, STR),
    h("_rt_strdup", STR, STR),
    // (pointer, length, width)
    h("_rt_fixed", &[I64, I64, I64], STR),
    // (pointer, length, count)
    h("_rt_left", &[I64, I64, I64], STR),
    h("_rt_right", &[I64, I64, I64], STR),
    // (pointer, length, start, count)
    h("_rt_mid", &[I64, I64, I64, I64], STR),
    // (left pointer, left length, right pointer, right length)
    h("_rt_strcat", &[I64, I64, I64, I64], STR),
    h("_rt_strcmp", &[I64, I64, I64, I64], &[I32]),
    // (haystack pointer, haystack length, needle pointer, needle length, start)
    fallible(h("_rt_instr", &[I64, I64, I64, I64, I64], &[I64])),
    // (target pointer, target length, start, count, source pointer, source length)
    h("_rt_mid_assign", &[I64, I64, I64, I64, I64, I64], &[]),
    // Numbers
    h("_rt_rnd", &[F64], &[F64]),
    h("_rt_randomize", &[F64], &[]),
    h("_rt_timer", &[], &[F64]),
    h("_rt_date", &[], STR),
    h("_rt_time", &[], STR),
];

/// The helper called `name`.
///
/// Panics for a name the table does not know: codegen calling a helper with
/// no signature is a compiler bug, and it is caught the first time that call
/// is compiled rather than when some later backend needs the signature.
pub fn lookup(name: &str) -> &'static RtHelper {
    HELPERS
        .iter()
        .find(|h| h.name == name)
        .unwrap_or_else(|| panic!("runtime helper {name} has no signature in rtsig.rs"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    /// Labels and the labels each can transfer control to, for one tree.
    ///
    /// Deliberately crude, and conservative in the right direction: every
    /// jump or call to a label is an edge, and so is falling off the end of
    /// one label into the next unless the last instruction was a `jmp` or
    /// `ret`. It can only over-report what reaches a label.
    fn control_flow(source: &str) -> (BTreeMap<String, BTreeSet<String>>, BTreeSet<String>) {
        let is_label = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        };
        let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut exported = BTreeSet::new();
        let mut current: Option<String> = None;
        let mut ended = false;
        for raw in source.lines() {
            let line = raw.split('#').next().unwrap().trim();
            if line.is_empty() {
                continue;
            }
            if let Some(name) = line.strip_prefix(".globl ") {
                exported.insert(name.trim().to_string());
                continue;
            }
            // A label, alone or followed by data on the same line
            // (`_mode: .asciz "r+b"`). Data is never executed, so nothing
            // falls through a data label.
            if let Some((label, rest)) = line.split_once(':')
                && is_label(label)
            {
                if let Some(prev) = &current
                    && !ended
                {
                    edges.get_mut(prev).unwrap().insert(label.to_string());
                }
                current = Some(label.to_string());
                edges.entry(label.to_string()).or_default();
                ended = !rest.trim().is_empty();
                continue;
            }
            let Some(cur) = &current else { continue };
            if line.starts_with('.') {
                continue; // a directive, not an instruction
            }
            let mut words = line.split_whitespace();
            let op = words.next().unwrap();
            let arg = words.next().unwrap_or("");
            if (op.starts_with('j') || op == "call") && is_label(arg) {
                edges.get_mut(cur).unwrap().insert(arg.to_string());
            }
            ended = op == "jmp" || op == "ret";
        }
        (edges, exported)
    }

    fn reaches_error(edges: &BTreeMap<String, BTreeSet<String>>, from: &str) -> bool {
        let mut seen = BTreeSet::new();
        let mut work = vec![from.to_string()];
        while let Some(label) = work.pop() {
            if label == "_rt_error" || label == "_rt_error_num" {
                return true;
            }
            if seen.insert(label.clone())
                && let Some(next) = edges.get(&label)
            {
                work.extend(next.iter().cloned());
            }
        }
        false
    }

    /// Every helper in the table is exported by both runtime trees.
    #[test]
    fn test_every_helper_exists_in_both_trees() {
        for (tree, source) in crate::runtime::tree_sources() {
            let (_, exported) = control_flow(&source);
            for helper in HELPERS {
                assert!(
                    exported.contains(helper.name),
                    "{} is in rtsig.rs but not exported by the {tree} runtime",
                    helper.name
                );
            }
        }
    }

    /// `fallible` says exactly whether a helper can reach `_rt_error` in
    /// either tree.
    #[test]
    fn test_fallible_matches_the_assembly() {
        let trees: Vec<_> = crate::runtime::tree_sources()
            .into_iter()
            .map(|(tree, source)| (tree, control_flow(&source).0))
            .collect();
        for helper in HELPERS {
            if helper.name == "_rt_error" || helper.name == "_rt_fatal" {
                continue;
            }
            let reaching: Vec<&str> = trees
                .iter()
                .filter(|(_, edges)| reaches_error(edges, helper.name))
                .map(|(tree, _)| *tree)
                .collect();
            assert_eq!(
                helper.fallible,
                !reaching.is_empty(),
                "{}: fallible is {} but _rt_error is reachable in {:?}",
                helper.name,
                helper.fallible,
                reaching
            );
        }
    }

    /// Every helper codegen names has a signature.
    ///
    /// `lookup` panics on an unknown name, but only when that call is
    /// compiled; this catches one that no test happens to compile.
    #[test]
    fn test_every_helper_codegen_names_is_listed() {
        let source = include_str!("codegen.rs");
        let mut rest = source;
        while let Some(at) = rest.find("\"_rt_") {
            let name: String = rest[at + 1..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            rest = &rest[at + 1 + name.len()..];
            // `"_rt_"` itself is a prefix test, not a helper.
            if !name.ends_with('_') {
                lookup(&name);
            }
        }
    }

    #[test]
    fn test_names_are_unique() {
        let mut seen = BTreeSet::new();
        for helper in HELPERS {
            assert!(seen.insert(helper.name), "{} listed twice", helper.name);
        }
    }
}
