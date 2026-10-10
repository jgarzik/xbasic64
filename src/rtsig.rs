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
    pub params: &'static [Ty],
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
    /// Whether a string result is a fresh allocation the caller owns and
    /// must free -- rather than a slice of its argument, a static buffer, or
    /// a pointer into DATA or a record buffer.
    pub fresh: bool,
}

const STR: &[Ty] = &[I64, I64];

const fn h(name: &'static str, params: &'static [Ty], rets: &'static [Ty]) -> RtHelper {
    RtHelper {
        name,
        params,
        rets,
        fallible: false,
        noreturn: false,
        fresh: false,
    }
}

const fn fallible(mut helper: RtHelper) -> RtHelper {
    helper.fallible = true;
    helper
}

const fn fresh(mut helper: RtHelper) -> RtHelper {
    helper.fresh = true;
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
    fallible(fresh(h("_rt_input_field_str", &[I64], STR))),
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
    fallible(fresh(h("_rt_mk", &[I64, I64], STR))),
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
    fallible(fresh(h("_rt_str", &[F64], STR))),
    fallible(fresh(h("_rt_str_single", &[F64], STR))),
    fallible(fresh(h("_rt_chr", &[I64], STR))),
    fallible(fresh(h("_rt_space", &[I64], STR))),
    fallible(fresh(h("_rt_hex", &[I64], STR))),
    fallible(fresh(h("_rt_oct", &[I64], STR))),
    // (count, character code)
    fallible(fresh(h("_rt_string_n", &[I64, I64], STR))),
    h("_rt_ltrim", STR, STR),
    h("_rt_rtrim", STR, STR),
    fallible(fresh(h("_rt_ucase", STR, STR))),
    fallible(fresh(h("_rt_lcase", STR, STR))),
    fallible(fresh(h("_rt_strdup", STR, STR))),
    // (pointer, length, width)
    fallible(fresh(h("_rt_fixed", &[I64, I64, I64], STR))),
    // (pointer, length, count)
    h("_rt_left", &[I64, I64, I64], STR),
    h("_rt_right", &[I64, I64, I64], STR),
    // (pointer, length, start, count)
    h("_rt_mid", &[I64, I64, I64, I64], STR),
    // (left pointer, left length, right pointer, right length)
    fallible(fresh(h("_rt_strcat", &[I64, I64, I64, I64], STR))),
    h("_rt_strcmp", &[I64, I64, I64, I64], &[I32]),
    // (haystack pointer, haystack length, needle pointer, needle length, start)
    fallible(h("_rt_instr", &[I64, I64, I64, I64, I64], &[I64])),
    // (target pointer, target length, start, count, source pointer, source length)
    h("_rt_mid_assign", &[I64, I64, I64, I64, I64, I64], &[]),
    // Numbers
    h("_rt_rnd", &[F64], &[F64]),
    h("_rt_randomize", &[F64], &[]),
    h("_rt_timer", &[], &[F64]),
    fallible(fresh(h("_rt_date", &[], STR))),
    fallible(fresh(h("_rt_time", &[], STR))),
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

    /// The runtime's sources, `rt/src/*.rs`, by module name.
    const RT_SOURCES: [(&str, &str); 11] = [
        ("data", include_str!("../rt/src/data.rs")),
        ("error", include_str!("../rt/src/error.rs")),
        ("file", include_str!("../rt/src/file.rs")),
        ("input", include_str!("../rt/src/input.rs")),
        ("crate", include_str!("../rt/src/lib.rs")),
        ("math", include_str!("../rt/src/math.rs")),
        ("num", include_str!("../rt/src/num.rs")),
        ("print", include_str!("../rt/src/print.rs")),
        ("string", include_str!("../rt/src/string.rs")),
        ("sys", include_str!("../rt/src/sys.rs")),
        ("using", include_str!("../rt/src/using.rs")),
    ];

    /// A function: its file's module, and its name.
    type Func = (&'static str, String);

    /// Each function in the runtime, and the functions its body may call --
    /// a call graph read from the text.
    ///
    /// Deliberately crude, and conservative in the right direction: a name
    /// mentioned in a body is a call. `m::f` is `f` in `m.rs` when there is
    /// such a file; a bare `f` (or one under some other path) is `f` in the
    /// same file if it has one, else every `f`; and functions of one name in
    /// one file are merged. It can only over-report what a function reaches.
    fn call_graph() -> BTreeMap<Func, BTreeSet<Func>> {
        let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
        // (module, name) -> the (qualifier, name) pairs its body mentions.
        let mut mentions: BTreeMap<Func, Vec<(String, String)>> = BTreeMap::new();
        for (module, source) in RT_SOURCES {
            // Comments and character literals hold braces that are not code.
            let code: String = source
                .lines()
                .map(|l| l.split("//").next().unwrap())
                .collect::<Vec<_>>()
                .join("\n")
                .replace("'{'", "' '")
                .replace("'}'", "' '");
            let mut rest = code.as_str();
            while let Some(at) = rest.find("fn ") {
                let after = &rest[at + 3..];
                let name: String = after.chars().take_while(|c| ident(*c)).collect();
                let word_start = !rest[..at].chars().last().is_some_and(ident);
                let Some(open) = after.find('{') else { break };
                // A declaration (`fn f();` in an extern block) has no body.
                if name.is_empty() || !word_start || after[..open].contains(';') {
                    rest = &rest[at + 3..];
                    continue;
                }
                let mut depth = 0;
                let mut close = open;
                for (i, c) in after[open..].char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                close = open + i;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let body = &after[open..close];
                let entry = mentions.entry((module, name)).or_default();
                let mut prev = "";
                let mut words = body.split_inclusive(|c: char| !ident(c)).peekable();
                while let Some(w) = words.next() {
                    let word = w.trim_end_matches(|c: char| !ident(c));
                    if !word.is_empty() {
                        entry.push((prev.to_string(), word.to_string()));
                    }
                    // `a::b`: remember `a` as `b`'s qualifier.
                    prev = if w.ends_with(':') && words.peek().is_some_and(|n| n == &":") {
                        word
                    } else {
                        ""
                    };
                }
                rest = &after[open + 1..];
            }
        }
        let defined: BTreeSet<Func> = mentions.keys().cloned().collect();
        let modules: BTreeSet<&str> = RT_SOURCES.iter().map(|(m, _)| *m).collect();
        let mut graph = BTreeMap::new();
        for (func, words) in &mentions {
            let mut callees = BTreeSet::new();
            for (qualifier, name) in words {
                let in_module = |m: &'static str| defined.get(&(m, name.clone())).cloned();
                if let Some(m) = modules.get(qualifier.as_str())
                    && let Some(f) = in_module(m)
                {
                    callees.insert(f);
                } else if let Some(f) = in_module(func.0) {
                    callees.insert(f);
                } else {
                    callees.extend(defined.iter().filter(|(_, n)| n == name).cloned());
                }
            }
            graph.insert(func.clone(), callees);
        }
        graph
    }

    /// Whether the helper `from` reaches a function named in `targets`.
    fn reaches(graph: &BTreeMap<Func, BTreeSet<Func>>, from: &str, targets: &[&str]) -> bool {
        let mut seen = BTreeSet::new();
        let mut work: Vec<Func> = graph.keys().filter(|(_, n)| n == from).cloned().collect();
        while let Some(func) = work.pop() {
            if targets.contains(&func.1.as_str()) {
                return true;
            }
            if seen.insert(func.clone())
                && let Some(next) = graph.get(&func)
            {
                work.extend(next.iter().cloned());
            }
        }
        false
    }

    /// Every helper that says its string result is fresh allocates it, and
    /// every one that does not, does not: generated code frees a fresh
    /// result, and freeing a slice or a static buffer would corrupt the heap,
    /// while not freeing a fresh one leaks it. `alloc_str` is the only
    /// allocator of returned strings.
    #[test]
    fn test_fresh_matches_the_runtime() {
        let graph = call_graph();
        for helper in HELPERS.iter().filter(|h| h.rets == STR) {
            assert_eq!(
                helper.fresh,
                reaches(&graph, helper.name, &["alloc_str"]),
                "{}: fresh is {}",
                helper.name,
                helper.fresh
            );
        }
        for helper in HELPERS.iter().filter(|h| h.fresh) {
            assert_eq!(
                helper.rets, STR,
                "{} is fresh, so returns a string",
                helper.name
            );
        }
    }

    /// Every helper in the table is a function of the runtime.
    #[test]
    fn test_every_helper_is_in_the_runtime() {
        let graph = call_graph();
        for helper in HELPERS {
            assert!(
                graph.keys().any(|(_, n)| n == helper.name),
                "{} is in rtsig.rs but not defined in rt/src",
                helper.name
            );
        }
    }

    /// `fallible` says exactly whether a helper can raise an error a
    /// handler may trap.
    #[test]
    fn test_fallible_matches_the_runtime() {
        let graph = call_graph();
        for helper in HELPERS {
            if helper.name == "_rt_error" || helper.name == "_rt_fatal" {
                continue;
            }
            assert_eq!(
                helper.fallible,
                reaches(&graph, helper.name, &["raise_with_code"]),
                "{}: fallible is {}",
                helper.name,
                helper.fallible
            );
        }
    }

    /// Every helper codegen names has a signature.
    ///
    /// `lookup` panics on an unknown name, but only when that call is
    /// compiled; this catches one that no test happens to compile.
    #[test]
    fn test_every_helper_codegen_names_is_listed() {
        for source in [
            include_str!("codegen/mod.rs"),
            include_str!("codegen/builtins.rs"),
            include_str!("codegen/calls.rs"),
            include_str!("codegen/expr.rs"),
            include_str!("codegen/io.rs"),
            include_str!("codegen/stmt.rs"),
            include_str!("codegen/storage.rs"),
            include_str!("codegen/strings.rs"),
            include_str!("codegen/trap.rs"),
        ] {
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
    }

    #[test]
    fn test_names_are_unique() {
        let mut seen = BTreeSet::new();
        for helper in HELPERS {
            assert!(seen.insert(helper.name), "{} listed twice", helper.name);
        }
    }
}
