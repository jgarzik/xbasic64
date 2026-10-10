//! Built-in functions

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use super::*;
use crate::lexer::normalized;

impl FnGen<'_> {
    /// A string argument's (pointer, length).
    fn str_arg(&mut self, e: &Expr) -> (Value, Value) {
        self.gen_expr(e).str()
    }

    /// Raise Illegal function call when the i64 `v` is below `min`.
    fn min_check(&mut self, v: Value, min: i64) {
        self.check_range(v, min, None, RtError::Domain);
    }

    /// A call to a built-in function or user FUNCTION, or an array element
    /// written like one.
    pub(super) fn gen_fn_call(&mut self, name: &str, args: &[Expr]) -> Val {
        let upper = name.to_uppercase();
        // A user procedure wins over a builtin of the same name, and an
        // array element is spelled like a call.
        if self.cg.symbols.procs.contains_key(&upper) {
            return self
                .gen_call(&upper, args)
                .expect("a FUNCTION returns a value");
        }

        match upper.as_str() {
            "SIN" | "COS" | "TAN" | "ATN" | "EXP" | "LOG" => {
                let x = self.gen_double(&args[0]);
                // LOG is undefined at and below zero.
                if upper == "LOG" && self.cg.opts.checks {
                    let zero = self.f64const(0.0);
                    let bad = self
                        .b
                        .ins()
                        .fcmp(FloatCC::UnorderedOrLessThanOrEqual, x, zero);
                    self.raise_if(bad, RtError::Domain);
                }
                let f = match upper.as_str() {
                    "SIN" => "sin",
                    "COS" => "cos",
                    "TAN" => "tan",
                    "ATN" => "atan",
                    "EXP" => "exp",
                    _ => "log",
                };
                let r = self.call(f, &[x])[0];
                // EXP is the one that can exceed DOUBLE's range.
                if upper == "EXP" {
                    self.check_float(r, DataType::Double);
                }
                double(r)
            }
            "SQR" => {
                let x = self.gen_double(&args[0]);
                if self.cg.opts.checks {
                    let zero = self.f64const(0.0);
                    let bad = self.b.ins().fcmp(FloatCC::UnorderedOrLessThan, x, zero);
                    self.raise_if(bad, RtError::Domain);
                }
                let r = self.b.ins().sqrt(x);
                double(r)
            }
            "INT" => {
                let x = self.gen_double(&args[0]);
                let r = self.b.ins().floor(x);
                double(r)
            }
            "FIX" => {
                let x = self.gen_double(&args[0]);
                let r = self.b.ins().trunc(x);
                double(r)
            }
            "TIMER" => double(self.call_rt("_rt_timer", &[])[0]),
            "ERR" | "ERL" => {
                let f = if upper == "ERR" { "_rt_err" } else { "_rt_erl" };
                let r = self.call_rt(f, &[])[0];
                Val::Num(DataType::Long, r)
            }
            "DATE$" | "TIME$" => {
                let f = if upper == "DATE$" {
                    "_rt_date"
                } else {
                    "_rt_time"
                };
                let r = self.call_rt(f, &[]);
                Val::Str(r[0], r[1])
            }
            "VAL" => {
                let (p, l) = self.str_arg(&args[0]);
                let r = self.call_rt("_rt_val", &[p, l])[0];
                double(r)
            }
            "LTRIM$" | "RTRIM$" | "UCASE$" | "LCASE$" => {
                let f = match upper.as_str() {
                    "LTRIM$" => "_rt_ltrim",
                    "RTRIM$" => "_rt_rtrim",
                    "UCASE$" => "_rt_ucase",
                    _ => "_rt_lcase",
                };
                let (p, l) = self.str_arg(&args[0]);
                let r = self.call_rt(f, &[p, l]);
                Val::Str(r[0], r[1])
            }
            "CHR$" | "SPACE$" => {
                let n = self.gen_i64(&args[0]);
                if upper == "CHR$" {
                    // A character code, not a byte.
                    self.check_range(n, 0, Some(255), RtError::Domain);
                } else {
                    self.min_check(n, 0);
                }
                let f = if upper == "CHR$" {
                    "_rt_chr"
                } else {
                    "_rt_space"
                };
                let r = self.call_rt(f, &[n]);
                Val::Str(r[0], r[1])
            }
            "HEX$" | "OCT$" => {
                // Two's complement in the argument's own width: HEX$(-1) of
                // an INTEGER is FFFF, of a LONG FFFFFFFF.
                let v = self.gen_expr(&args[0]);
                let integer = v.ty() == DataType::Integer;
                let n = self.long_of(v);
                let n = if integer {
                    let h = self.b.ins().ireduce(types::I16, n);
                    self.b.ins().uextend(types::I64, h)
                } else {
                    self.b.ins().uextend(types::I64, n)
                };
                let f = if upper == "HEX$" {
                    "_rt_hex"
                } else {
                    "_rt_oct"
                };
                let r = self.call_rt(f, &[n]);
                Val::Str(r[0], r[1])
            }
            "CSNG" => {
                let v = self.gen_expr(&args[0]);
                self.coerce(v, DataType::Single)
            }
            "CDBL" => {
                let v = self.gen_expr(&args[0]);
                self.coerce(v, DataType::Double)
            }
            "CINT" | "CLNG" => {
                let v = self.gen_expr(&args[0]);
                let to = if upper == "CINT" {
                    DataType::Integer
                } else {
                    DataType::Long
                };
                self.coerce(v, to)
            }
            "ABS" => {
                let v = self.gen_expr(&args[0]);
                let t = v.ty();
                let x = self.double_of(v);
                let r = self.b.ins().fabs(x);
                self.coerce(Val::Num(DataType::Double, r), expr::abs_result_type(t))
            }
            "SGN" => {
                let x = self.gen_double(&args[0]);
                let zero = self.f64const(0.0);
                let gt = self.b.ins().fcmp(FloatCC::GreaterThan, x, zero);
                let lt = self.b.ins().fcmp(FloatCC::UnorderedOrLessThan, x, zero);
                let gt = self.b.ins().uextend(types::I32, gt);
                let lt = self.b.ins().uextend(types::I32, lt);
                let s = self.b.ins().isub(gt, lt);
                let r = self.b.ins().fcvt_from_sint(types::F64, s);
                double(r)
            }
            "RND" => {
                // A bare RND means "next value", which _rt_rnd spells as a
                // positive argument.
                let x = match args.first() {
                    Some(a) => self.gen_double(a),
                    None => self.f64const(1.0),
                };
                let r = self.call_rt("_rt_rnd", &[x])[0];
                double(r)
            }
            "LEN" => {
                let (_, l) = self.str_arg(&args[0]);
                let r = self.b.ins().ireduce(types::I32, l);
                Val::Num(DataType::Long, r)
            }
            "LEFT$" | "RIGHT$" => {
                let (p, l) = self.str_arg(&args[0]);
                let n = self.gen_i64(&args[1]);
                self.min_check(n, 0);
                let f = if upper == "LEFT$" {
                    "_rt_left"
                } else {
                    "_rt_right"
                };
                let r = self.call_rt(f, &[p, l, n]);
                Val::Str(r[0], r[1])
            }
            "MID$" => {
                let (p, l) = self.str_arg(&args[0]);
                let start = self.gen_i64(&args[1]);
                // String indexing is 1-based.
                self.min_check(start, 1);
                let count = match args.get(2) {
                    Some(e) => {
                        let n = self.gen_i64(e);
                        self.min_check(n, 0);
                        n
                    }
                    // -1 is "the rest", which _rt_mid reads as such.
                    None => self.iconst(types::I64, -1),
                };
                let r = self.call_rt("_rt_mid", &[p, l, start, count]);
                Val::Str(r[0], r[1])
            }
            "INSTR" => {
                let (start, hay, needle) = if args.len() == 3 {
                    (Some(&args[0]), &args[1], &args[2])
                } else {
                    (None, &args[0], &args[1])
                };
                let start = match start {
                    Some(s) => self.gen_i64(s),
                    None => self.iconst(types::I64, 1),
                };
                let (hp, hl) = self.str_arg(hay);
                let (np, nl) = self.str_arg(needle);
                let r = self.call_rt("_rt_instr", &[hp, hl, np, nl, start])[0];
                let r = self.b.ins().ireduce(types::I32, r);
                Val::Num(DataType::Long, r)
            }
            "FRE" => {
                // No BASIC string heap to run out of; a plausible figure.
                self.gen_expr(&args[0]);
                Val::Num(DataType::Long, self.iconst(types::I32, 65535))
            }
            "POS" => {
                // The argument is ignored, as in GW-BASIC.
                self.gen_expr(&args[0]);
                let r = self.call_rt("_rt_pos", &[])[0];
                Val::Num(DataType::Long, r)
            }
            "ASC" => {
                let (p, l) = self.str_arg(&args[0]);
                // The empty string has no first character.
                self.min_check(l, 1);
                let c = self
                    .b
                    .ins()
                    .uload8(types::I32, MemFlagsData::trusted(), p, 0);
                Val::Num(DataType::Long, c)
            }
            "STRING$" => {
                // STRING$(n, ch): a character code, or a string whose first
                // character is used.
                let n = self.gen_i64(&args[0]);
                self.min_check(n, 0);
                let ch = self.gen_expr(&args[1]);
                let c = if ch.ty() == DataType::String {
                    let (p, l) = ch.str();
                    self.min_check(l, 1);
                    self.b
                        .ins()
                        .uload8(types::I64, MemFlagsData::trusted(), p, 0)
                } else {
                    let c = self.i64_of(ch);
                    self.check_range(c, 0, Some(255), RtError::Domain);
                    c
                };
                let r = self.call_rt("_rt_string_n", &[n, c]);
                Val::Str(r[0], r[1])
            }
            "LBOUND" | "UBOUND" => self.gen_bound(&upper, args),
            "EOF" | "LOF" | "LOC" => {
                let n = self.gen_i64(&args[0]);
                self.file_num_check(n);
                let f = match upper.as_str() {
                    "EOF" => "_rt_file_eof",
                    "LOC" => "_rt_file_loc",
                    _ => "_rt_file_lof",
                };
                let r = self.call_rt(f, &[n])[0];
                let ty = self.fn_return_type(&upper);
                self.rt_number(r, ty)
            }
            // STR$ renders what PRINT would, picking the same digits PRINT
            // would by the argument's static type.
            "STR$" => {
                let single = self.expr_type(&args[0]) == DataType::Single;
                let x = self.gen_double(&args[0]);
                let f = if single { "_rt_str_single" } else { "_rt_str" };
                let r = self.call_rt(f, &[x]);
                Val::Str(r[0], r[1])
            }
            // MKI$/MKL$/MKS$/MKD$: a number's bytes, at its type's width.
            "MKI$" | "MKL$" | "MKS$" | "MKD$" => {
                let (want, width) = match upper.as_str() {
                    "MKI$" => (DataType::Integer, 2),
                    "MKL$" => (DataType::Long, 4),
                    "MKS$" => (DataType::Single, 4),
                    _ => (DataType::Double, 8),
                };
                let v = self.gen_expr(&args[0]);
                let v = self.coerce(v, want).num();
                let bits = match want {
                    DataType::Integer | DataType::Long => self.b.ins().sextend(types::I64, v),
                    DataType::Single => {
                        let b = self.b.ins().bitcast(types::I32, MemFlagsData::new(), v);
                        self.b.ins().uextend(types::I64, b)
                    }
                    _ => self.b.ins().bitcast(types::I64, MemFlagsData::new(), v),
                };
                let w = self.iconst(types::I64, width);
                let r = self.call_rt("_rt_mk", &[bits, w]);
                Val::Str(r[0], r[1])
            }
            // CVI/CVL/CVS/CVD: those bytes read back as a number.
            "CVI" | "CVL" | "CVS" | "CVD" => {
                let (p, l) = self.str_arg(&args[0]);
                let line = self.iconst(types::I64, i64::from(self.report_line()));
                let f = match upper.as_str() {
                    "CVI" => "_rt_cvi",
                    "CVL" => "_rt_cvl",
                    "CVS" => "_rt_cvs",
                    _ => "_rt_cvd",
                };
                let r = self.call_rt(f, &[p, l, line])[0];
                let ty = self.fn_return_type(&upper);
                self.rt_number(r, ty)
            }
            // TAB/SPC outside PRINT still move the console's position.
            "TAB" | "SPC" => {
                let console = self.iconst(types::I64, CONSOLE);
                self.gen_print_position(&upper, &args[0], console);
                double(self.f64const(0.0))
            }
            _ => self.array_load(&upper, args),
        }
    }

    /// A runtime result as a number of type `ty`: an integer helper's i32 or
    /// i64, or a Double.
    pub(super) fn rt_number(&mut self, r: Value, ty: DataType) -> Val {
        let have = self.b.func.dfg.value_type(r);
        let v = match (have, ty) {
            (types::F64, DataType::Double) => r,
            (types::F64, DataType::Single) => self.b.ins().fdemote(types::F32, r),
            (types::F64, t) => return self.coerce(Val::Num(DataType::Double, r), t),
            (types::I64, _) => self.b.ins().ireduce(types::I32, r),
            _ => r,
        };
        Val::Num(ty, v)
    }

    /// LBOUND/UBOUND: the stored lower bound, or that plus the count, less
    /// one. An array with no storage yet is created first, as any use of it
    /// would be.
    fn gen_bound(&mut self, upper: &str, args: &[Expr]) -> Val {
        let Expr::Variable(arr) = &args[0] else {
            unreachable!("sema requires an array name here")
        };
        let arr = arr.to_uppercase();
        let rank = self
            .cg
            .symbols
            .lookup_array(&self.sema_scope(), &arr)
            .expect("sema checked the array exists")
            .rank as i64;
        let info = self
            .lookup_array(&arr)
            .expect("sema checked the array exists");
        // Touching an element is what creates it; the bound read below is
        // then of the array that exists.
        let ptr = self.load(types::I64, info.loc);
        let create = self.b.create_block();
        let have = self.b.create_block();
        self.brif(ptr, have, create);
        self.switch_sealed(create);
        let desc = self.addr(info.loc);
        let nd = self.iconst(types::I64, info.ndims as i64);
        let es = self.iconst(types::I64, i64::from(self.elem_size_for(&arr)));
        let base = self.iconst(types::I64, self.cg.symbols.option_base);
        self.call_rt("_rt_array_autodim", &[desc, nd, es, base]);
        self.begin_sealed(have);

        let is_lower = upper == "LBOUND";
        let constant = match args.get(1) {
            None => Some(1),
            Some(Expr::Literal(Literal::Integer(n))) => Some(*n),
            Some(Expr::Variable(n)) => match self.cg.symbols.consts.get(normalized(n)) {
                Some(Literal::Integer(n)) => Some(*n),
                _ => None,
            },
            Some(_) => None,
        };
        let r = match constant {
            Some(dim) => {
                let dim = dim as i32;
                let lo = self.load(types::I64, info.loc.offset(8 * (rank as i32 + dim)));
                if is_lower {
                    lo
                } else {
                    let n = self.load(types::I64, info.loc.offset(8 * dim));
                    let s = self.b.ins().iadd(lo, n);
                    self.b.ins().iadd_imm_s(s, -1)
                }
            }
            None => {
                // A computed dimension, 1..=rank, indexes the descriptor.
                let d = self.gen_i64(&args[1]);
                let d0 = self.b.ins().iadd_imm_s(d, -1);
                if self.cg.opts.checks {
                    let bad = self
                        .b
                        .ins()
                        .icmp_imm_s(IntCC::UnsignedGreaterThanOrEqual, d0, rank);
                    self.raise_if(bad, RtError::Subscript);
                }
                let desc = self.addr(info.loc);
                let off = self.b.ins().ishl_imm_s(d, 3);
                let at = self.b.ins().iadd(desc, off);
                let lo =
                    self.b
                        .ins()
                        .load(types::I64, MemFlagsData::trusted(), at, (8 * rank) as i32);
                if is_lower {
                    lo
                } else {
                    let n = self
                        .b
                        .ins()
                        .load(types::I64, MemFlagsData::trusted(), at, 0);
                    let s = self.b.ins().iadd(lo, n);
                    self.b.ins().iadd_imm_s(s, -1)
                }
            }
        };
        let r = self.b.ins().ireduce(types::I32, r);
        Val::Num(DataType::Long, r)
    }

    /// Check a file number against the handle table's slots.
    pub(super) fn file_num_check(&mut self, n: Value) {
        self.check_range(n, 1, Some(crate::sema::MAX_FILE_NUM), RtError::BadFileNum);
    }
}

/// A Double result.
fn double(v: Value) -> Val {
    Val::Num(DataType::Double, v)
}
