//! `xbasic64 run`: compile a program into memory and run it, here
//!
//! The same lowering as an executable's (`codegen::lower`), into a
//! `JITModule` instead of an `ObjectModule`, and the same runtime -- the
//! `rt` crate, linked into the compiler as well as built for programs. The
//! whole program is compiled before it starts; nothing is interpreted.
//!
//! Two things differ from an executable, both in where things are:
//!
//! - The runtime and the C library are this process's, anywhere in its
//!   address space, so generated code reaches them by absolute address
//!   (`colocate` off), and the JIT is told each one's address here:
//!   `symbols` lists every helper, message and trapping word, and every C
//!   function generated code calls. An executable does not export its
//!   functions, so nothing is found by name otherwise.
//! - The program's own code and data are PC-relative, as in an executable,
//!   which needs them within 2 GB of each other: they are allocated from one
//!   arena, reserved up front. It starts small and is doubled, and the
//!   program lowered again, when it runs out.
//!
//! The program ends the process, as an executable does: END, STOP and a
//! fatal error exit from the runtime, and the end of main exits with what
//! main returns.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::codegen::{self, Options};
use crate::parser::Program;
use crate::sema::Symbols;
use cranelift_jit::{ArenaMemoryProvider, JITBuilder, JITModule};
use std::ffi::c_void;
use std::io::Write;
use xbasic64_rt as rt;

/// The arena's first size, and the largest it may grow to: below 2 GB, the
/// reach of a PC-relative reference.
const ARENA_START: usize = 64 << 20;
const ARENA_MAX: usize = 1 << 30;

/// Compile `program` and run it; returns only if it cannot be compiled.
pub fn run(program: &Program, symbols: &Symbols, opts: Options) -> String {
    let mut arena = ARENA_START;
    let lowered = loop {
        let module = match new_module(arena) {
            Ok(m) => m,
            Err(e) => return e,
        };
        match codegen::lower(program, symbols.clone(), opts, module, false, false) {
            Ok(l) => break l,
            // Lowering defines each function as it goes, so a full arena
            // shows as an error part-way; lower again into a larger one.
            Err(e) if e.contains("exhausted") && arena < ARENA_MAX => arena *= 2,
            Err(e) => return e,
        }
    };
    let mut module = lowered.module;
    if let Err(e) = module.finalize_definitions() {
        return e.to_string();
    }
    let main = module.get_finalized_function(lowered.main);
    // `main` has the platform's C convention: `codegen::lower` declares it
    // with the target's default, as an executable's C runtime calls it.
    let main: extern "C" fn() -> i32 = unsafe { std::mem::transmute(main) };
    // The program writes through the C library's stdout; nothing of the
    // compiler's own may be left waiting in front of it.
    let _ = std::io::stdout().flush();
    let status = main();
    // What an executable's C runtime does on return from main. Windows'
    // ExitProcess, under std's exit, would not flush the C library.
    rt::sys::flush_all();
    drop(module); // after main: its code must outlive the call
    std::process::exit(status)
}

/// A JIT module for this host, the program's memory from an arena of
/// `arena` bytes, every outside symbol it may need known.
fn new_module(arena: usize) -> Result<JITModule, String> {
    let isa = codegen::make_isa(false)?;
    let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
    builder.symbols(symbols());
    let memory = ArenaMemoryProvider::new_with_size(arena)
        .map_err(|e| format!("cannot reserve {} MB for the JIT: {e}", arena >> 20))?;
    builder.memory_provider(Box::new(memory));
    Ok(JITModule::new(builder))
}

// The C library functions generated code calls (`codegen::LIBC_FUNCTIONS`),
// for their addresses. Declared with the C library's own types, which is
// all an address needs.
unsafe extern "C" {
    fn pow(x: f64, y: f64) -> f64;
    fn sin(x: f64) -> f64;
    fn cos(x: f64) -> f64;
    fn tan(x: f64) -> f64;
    fn atan(x: f64) -> f64;
    fn exp(x: f64) -> f64;
    fn log(x: f64) -> f64;
    fn calloc(n: usize, size: usize) -> *mut c_void;
    fn realloc(p: *mut c_void, size: usize) -> *mut c_void;
    fn free(p: *mut c_void);
    fn memset(p: *mut c_void, c: i32, n: usize) -> *mut c_void;
    fn memcpy(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void;
}

/// Every symbol generated code may name that is not its own: the runtime's
/// helpers (`rtsig`), its messages and trapping words, and the C library
/// functions codegen calls. A test holds this to `rtsig` and codegen.
fn symbols() -> Vec<(&'static str, *const u8)> {
    let word = |w: &'static rt::Word| (w as *const rt::Word).cast::<u8>();
    vec![
        (
            "_rt_array_autodim",
            rt::data::_rt_array_autodim as *const u8,
        ),
        ("_rt_beep", rt::math::_rt_beep as *const u8),
        ("_rt_chr", rt::string::_rt_chr as *const u8),
        ("_rt_cls", rt::math::_rt_cls as *const u8),
        ("_rt_color", rt::math::_rt_color as *const u8),
        ("_rt_cvd", rt::file::_rt_cvd as *const u8),
        ("_rt_cvi", rt::file::_rt_cvi as *const u8),
        ("_rt_cvl", rt::file::_rt_cvl as *const u8),
        ("_rt_cvs", rt::file::_rt_cvs as *const u8),
        ("_rt_data_init", rt::data::_rt_data_init as *const u8),
        ("_rt_date", rt::math::_rt_date as *const u8),
        ("_rt_end", rt::error::_rt_end as *const u8),
        ("_rt_erl", rt::error::_rt_erl as *const u8),
        ("_rt_err", rt::error::_rt_err as *const u8),
        ("_rt_error", rt::error::_rt_error as *const u8),
        ("_rt_error_num", rt::error::_rt_error_num as *const u8),
        ("_rt_fatal", rt::error::_rt_fatal as *const u8),
        ("_rt_field_next", rt::file::_rt_field_next as *const u8),
        ("_rt_field_reset", rt::file::_rt_field_reset as *const u8),
        ("_rt_file_close", rt::file::_rt_file_close as *const u8),
        (
            "_rt_file_close_all",
            rt::file::_rt_file_close_all as *const u8,
        ),
        ("_rt_file_eof", rt::file::_rt_file_eof as *const u8),
        ("_rt_file_get", rt::file::_rt_file_get as *const u8),
        (
            "_rt_file_input_number",
            rt::file::_rt_file_input_number as *const u8,
        ),
        (
            "_rt_file_input_string",
            rt::file::_rt_file_input_string as *const u8,
        ),
        (
            "_rt_file_line_input",
            rt::file::_rt_file_line_input as *const u8,
        ),
        ("_rt_file_loc", rt::file::_rt_file_loc as *const u8),
        ("_rt_file_lof", rt::file::_rt_file_lof as *const u8),
        ("_rt_file_open", rt::file::_rt_file_open as *const u8),
        (
            "_rt_file_open_random",
            rt::file::_rt_file_open_random as *const u8,
        ),
        (
            "_rt_file_print_char",
            rt::file::_rt_file_print_char as *const u8,
        ),
        (
            "_rt_file_print_float",
            rt::file::_rt_file_print_float as *const u8,
        ),
        (
            "_rt_file_print_newline",
            rt::file::_rt_file_print_newline as *const u8,
        ),
        (
            "_rt_file_print_single",
            rt::file::_rt_file_print_single as *const u8,
        ),
        (
            "_rt_file_print_spc",
            rt::file::_rt_file_print_spc as *const u8,
        ),
        (
            "_rt_file_print_string",
            rt::file::_rt_file_print_string as *const u8,
        ),
        (
            "_rt_file_print_tab",
            rt::file::_rt_file_print_tab as *const u8,
        ),
        ("_rt_file_put", rt::file::_rt_file_put as *const u8),
        ("_rt_fixed", rt::string::_rt_fixed as *const u8),
        ("_rt_hex", rt::string::_rt_hex as *const u8),
        (
            "_rt_input_field_num",
            rt::input::_rt_input_field_num as *const u8,
        ),
        (
            "_rt_input_field_str",
            rt::input::_rt_input_field_str as *const u8,
        ),
        ("_rt_input_line", rt::input::_rt_input_line as *const u8),
        ("_rt_input_string", rt::input::_rt_input_string as *const u8),
        ("_rt_instr", rt::string::_rt_instr as *const u8),
        ("_rt_lcase", rt::string::_rt_lcase as *const u8),
        ("_rt_left", rt::string::_rt_left as *const u8),
        ("_rt_locate", rt::math::_rt_locate as *const u8),
        ("_rt_lock", rt::file::_rt_lock as *const u8),
        ("_rt_lset", rt::file::_rt_lset as *const u8),
        ("_rt_ltrim", rt::string::_rt_ltrim as *const u8),
        ("_rt_mid", rt::string::_rt_mid as *const u8),
        ("_rt_mid_assign", rt::string::_rt_mid_assign as *const u8),
        ("_rt_mk", rt::file::_rt_mk as *const u8),
        ("_rt_oct", rt::string::_rt_oct as *const u8),
        (
            "_rt_platform_init",
            rt::print::_rt_platform_init as *const u8,
        ),
        ("_rt_pos", rt::math::_rt_pos as *const u8),
        ("_rt_print_number", rt::file::_rt_print_number as *const u8),
        (
            "_rt_print_number_single",
            rt::file::_rt_print_number_single as *const u8,
        ),
        ("_rt_print_zone", rt::file::_rt_print_zone as *const u8),
        ("_rt_randomize", rt::math::_rt_randomize as *const u8),
        ("_rt_read_number", rt::data::_rt_read_number as *const u8),
        ("_rt_read_string", rt::data::_rt_read_string as *const u8),
        ("_rt_restore", rt::data::_rt_restore as *const u8),
        ("_rt_right", rt::string::_rt_right as *const u8),
        ("_rt_rnd", rt::math::_rt_rnd as *const u8),
        ("_rt_rset", rt::file::_rt_rset as *const u8),
        ("_rt_rtrim", rt::string::_rt_rtrim as *const u8),
        ("_rt_space", rt::string::_rt_space as *const u8),
        ("_rt_stop", rt::error::_rt_stop as *const u8),
        ("_rt_str", rt::string::_rt_str as *const u8),
        ("_rt_strcat", rt::string::_rt_strcat as *const u8),
        ("_rt_strcmp", rt::string::_rt_strcmp as *const u8),
        ("_rt_strdup", rt::string::_rt_strdup as *const u8),
        ("_rt_string_n", rt::string::_rt_string_n as *const u8),
        ("_rt_str_single", rt::string::_rt_str_single as *const u8),
        ("_rt_time", rt::math::_rt_time as *const u8),
        ("_rt_timer", rt::math::_rt_timer as *const u8),
        ("_rt_ucase", rt::string::_rt_ucase as *const u8),
        ("_rt_unlock", rt::file::_rt_unlock as *const u8),
        ("_rt_using_end", rt::using::_rt_using_end as *const u8),
        ("_rt_using_num", rt::using::_rt_using_num as *const u8),
        ("_rt_using_start", rt::using::_rt_using_start as *const u8),
        ("_rt_using_str", rt::using::_rt_using_str as *const u8),
        ("_rt_val", rt::num::_rt_val as *const u8),
        ("_rt_width", rt::file::_rt_width as *const u8),
        // The runtime's error-trapping words and messages.
        ("_err_handler", word(&rt::error::_err_handler)),
        ("_err_pending", word(&rt::error::_err_pending)),
        ("_err_active", word(&rt::error::_err_active)),
        ("_err_line", word(&rt::error::_err_line)),
        ("_err_syntax", rt::error::_err_syntax.as_ptr()),
        ("_err_subscript", rt::error::_err_subscript.as_ptr()),
        ("_err_typemismatch", rt::error::_err_typemismatch.as_ptr()),
        ("_err_div0", rt::error::_err_div0.as_ptr()),
        ("_err_domain", rt::error::_err_domain.as_ptr()),
        ("_err_overflow", rt::error::_err_overflow.as_ptr()),
        ("_err_dupdef", rt::error::_err_dupdef.as_ptr()),
        ("_err_memory", rt::error::_err_memory.as_ptr()),
        ("_err_gosub", rt::error::_err_gosub.as_ptr()),
        ("_err_noreturn", rt::error::_err_noreturn.as_ptr()),
        ("_err_outofdata", rt::error::_err_outofdata.as_ptr()),
        ("_err_badfile", rt::error::_err_badfile.as_ptr()),
        ("_err_badmode", rt::error::_err_badmode.as_ptr()),
        ("_err_fieldovf", rt::error::_err_fieldovf.as_ptr()),
        ("_err_permission", rt::error::_err_permission.as_ptr()),
        ("_err_notfound", rt::error::_err_notfound.as_ptr()),
        ("_err_alreadyopen", rt::error::_err_alreadyopen.as_ptr()),
        ("_err_pastend", rt::error::_err_pastend.as_ptr()),
        ("_err_badname", rt::error::_err_badname.as_ptr()),
        ("_err_access", rt::error::_err_access.as_ptr()),
        ("_err_pathnotfound", rt::error::_err_pathnotfound.as_ptr()),
        ("_err_unprintable", rt::error::_err_unprintable.as_ptr()),
        ("_err_resnoerr", rt::error::_err_resnoerr.as_ptr()),
        // The C library.
        ("pow", pow as *const u8),
        ("sin", sin as *const u8),
        ("cos", cos as *const u8),
        ("tan", tan as *const u8),
        ("atan", atan as *const u8),
        ("exp", exp as *const u8),
        ("log", log as *const u8),
        ("calloc", calloc as *const u8),
        ("realloc", realloc as *const u8),
        ("free", free as *const u8),
        ("memset", memset as *const u8),
        ("memcpy", memcpy as *const u8),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The JIT knows every outside symbol generated code may name -- each
    /// runtime helper in `rtsig`, each message codegen raises, the trapping
    /// words and each C function codegen calls -- once, and nothing else:
    /// one it did not know would stop `run` with a panic at load time.
    #[test]
    fn test_the_jit_knows_every_outside_symbol() {
        let table: Vec<&str> = symbols().iter().map(|(n, _)| *n).collect();
        let known: BTreeSet<&str> = table.iter().copied().collect();
        assert_eq!(known.len(), table.len(), "a symbol is listed twice");
        let mut wanted: BTreeSet<&str> = crate::rtsig::HELPERS.iter().map(|h| h.name).collect();
        wanted.extend(codegen::LIBC_FUNCTIONS);
        wanted.extend(["_err_handler", "_err_pending", "_err_active", "_err_line"]);
        for name in &wanted {
            assert!(known.contains(name), "the JIT does not know {name}");
        }
        for name in &known {
            assert!(
                wanted.contains(name) || name.starts_with("_err_"),
                "{name} is not something generated code names"
            );
        }
        assert!(symbols().iter().all(|(_, p)| !p.is_null()));
    }
}
