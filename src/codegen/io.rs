//! Console and file statements: PRINT, WRITE, INPUT, OPEN and the rest

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use super::*;

impl FnGen<'_> {
    /// A file number as an i64. A literal is taken as written -- sema has
    /// checked it -- and anything else is checked against the handle
    /// table's slots.
    fn file_num(&mut self, e: &Expr) -> Value {
        if let Expr::Literal(Literal::Integer(n)) = e {
            return self.iconst(types::I64, *n);
        }
        let n = self.gen_i64(e);
        self.file_num_check(n);
        n
    }

    /// A record number, length or width: a LONG, not checked as a file
    /// number.
    fn long_arg(&mut self, e: &Expr) -> Value {
        if let Expr::Literal(Literal::Integer(n)) = e {
            return self.iconst(types::I64, *n);
        }
        self.gen_i64(e)
    }

    /// Where a PRINT writes: a file number, or the console.
    fn sink(&mut self, file_num: Option<&Expr>) -> Value {
        match file_num {
            Some(e) => self.file_num(e),
            None => self.iconst(types::I64, CONSOLE),
        }
    }

    /// An optional LOCATE/COLOR argument, range checked, or -1 when omitted:
    /// the runtime then leaves that setting as it was.
    fn optional_arg(&mut self, arg: Option<&Expr>, min: i64, max: Option<i64>) -> Value {
        match arg {
            Some(e) => {
                let n = self.gen_i64(e);
                self.check_range(n, min, max, RtError::Domain);
                n
            }
            None => self.iconst(types::I64, -1),
        }
    }

    pub(super) fn gen_io_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Print {
                file_num,
                items,
                newline,
                using: Some(fmt),
                ..
            } => {
                let sink = self.sink(file_num.as_ref());
                // The format is read at run time, as the statement runs.
                let (fp, fl) = self.gen_expr(fmt).str();
                self.call_rt("_rt_using_start", &[sink, fp, fl]);
                for item in items {
                    let PrintItem::Expr(value) = item else {
                        continue;
                    };
                    if self.expr_type(value) == DataType::String {
                        let (p, l) = self.gen_expr(value).str();
                        self.call_rt("_rt_using_str", &[p, l]);
                    } else {
                        let x = self.gen_double(value);
                        self.call_rt("_rt_using_num", &[x]);
                    }
                }
                self.call_rt("_rt_using_end", &[]);
                if *newline {
                    self.call_rt("_rt_file_print_newline", &[sink]);
                }
            }

            StmtKind::Print {
                file_num,
                items,
                newline,
                write,
                ..
            } => {
                let sink = self.sink(file_num.as_ref());
                let mut first = true;
                for item in items {
                    match item {
                        PrintItem::Expr(expr) => {
                            // WRITE separates values with commas and quotes
                            // strings; PRINT emits them bare.
                            if *write {
                                if !first {
                                    let comma = self.iconst(types::I64, i64::from(b','));
                                    self.call_rt("_rt_file_print_char", &[sink, comma]);
                                }
                                self.gen_write_expr(expr, sink);
                            } else {
                                self.gen_print_expr(expr, sink, true);
                            }
                            first = false;
                        }
                        PrintItem::Tab => {
                            // For WRITE the comma is only a separator.
                            if !*write {
                                self.call_rt("_rt_print_zone", &[sink]);
                            }
                        }
                        PrintItem::Empty => {}
                    }
                }
                if *newline {
                    self.call_rt("_rt_file_print_newline", &[sink]);
                }
            }

            StmtKind::Input {
                prompt,
                query,
                vars,
                file_num,
            } => {
                if let Some(e) = file_num {
                    let f = self.file_num(e);
                    for var in vars {
                        let v = if self.expr_type(&Self::lvalue_expr(var)) == DataType::String {
                            let r = self.call_rt("_rt_file_input_string", &[f]);
                            Val::Str(r[0], r[1])
                        } else {
                            let r = self.call_rt("_rt_file_input_number", &[f]);
                            Val::Num(DataType::Double, r[0])
                        };
                        self.store_lvalue(var, v);
                    }
                    return;
                }

                // The question mark is part of the prompt text.
                let text = match (prompt.as_deref(), query) {
                    (Some(p), true) => Some(format!("{}? ", p)),
                    (Some(p), false) => Some(p.to_string()),
                    (None, true) => Some("? ".to_string()),
                    (None, false) => None,
                };
                // The console form reads and checks one whole line for the
                // statement, then hands the values out one by one. It needs
                // each variable's type for that.
                let types_str: String = vars
                    .iter()
                    .map(|v| match self.expr_type(&Self::lvalue_expr(v)) {
                        DataType::String => '$',
                        DataType::Integer => 'I',
                        DataType::Long => 'L',
                        DataType::Single => 'S',
                        DataType::Double => 'D',
                    })
                    .collect();
                let (pp, pl) = match &text {
                    Some(t) => self.str_literal(t),
                    None => {
                        let z = self.iconst(types::I64, 0);
                        (z, z)
                    }
                };
                let (tp, _) = self.str_literal(&types_str);
                let n = self.iconst(types::I64, vars.len() as i64);
                self.call_rt("_rt_input_line", &[pp, pl, tp, n]);
                for (i, var) in vars.iter().enumerate() {
                    let idx = self.iconst(types::I64, i as i64);
                    let v = if self.expr_type(&Self::lvalue_expr(var)) == DataType::String {
                        let r = self.call_rt("_rt_input_field_str", &[idx]);
                        Val::Str(r[0], r[1])
                    } else {
                        let r = self.call_rt("_rt_input_field_num", &[idx]);
                        Val::Num(DataType::Double, r[0])
                    };
                    self.store_lvalue(var, v);
                }
            }

            StmtKind::LineInput {
                prompt,
                var,
                file_num,
            } => {
                if let Some(p) = prompt {
                    let console = self.iconst(types::I64, CONSOLE);
                    let (pp, pl) = self.str_literal(p);
                    self.call_rt("_rt_file_print_string", &[console, pp, pl]);
                }
                let r = match file_num {
                    Some(e) => {
                        let f = self.file_num(e);
                        self.call_rt("_rt_file_line_input", &[f])
                    }
                    None => self.call_rt("_rt_input_string", &[]),
                };
                self.store_lvalue(var, Val::Str(r[0], r[1]));
            }

            StmtKind::Open {
                filename,
                mode,
                file_num,
                reclen,
            } => {
                let f = self.file_num(file_num);
                let rec = match (mode, reclen) {
                    (FileMode::Random, None) => Some(self.iconst(types::I64, DEFAULT_RECLEN)),
                    (FileMode::Random, Some(e)) => Some(self.long_arg(e)),
                    _ => None,
                };
                let (np, nl) = self.gen_expr(filename).str();
                match rec {
                    Some(r) => {
                        self.call_rt("_rt_file_open_random", &[np, nl, f, r]);
                    }
                    None => {
                        let m = match mode {
                            FileMode::Input => 0,
                            FileMode::Output => 1,
                            FileMode::Append => 2,
                            FileMode::Random => unreachable!("taken above"),
                        };
                        let m = self.iconst(types::I64, m);
                        self.call_rt("_rt_file_open", &[np, nl, m, f]);
                    }
                }
            }

            // Bare CLOSE closes every open file.
            StmtKind::Close(nums) if nums.is_empty() => {
                self.call_rt("_rt_file_close_all", &[]);
            }
            StmtKind::Close(nums) => {
                for e in nums {
                    let f = self.file_num(e);
                    self.call_rt("_rt_file_close", &[f]);
                }
            }

            // WIDTH: 1 to 255 columns, 255 meaning no limit.
            StmtKind::Width { file_num, width } => {
                let f = self.sink(file_num.as_ref());
                let w = self.gen_i64(width);
                self.check_range(w, 1, Some(255), RtError::Domain);
                self.call_rt("_rt_width", &[f, w]);
            }

            StmtKind::Field { file_num, fields } => {
                // Each clause binds its variable to (buffer + offset, width)
                // without copying, so it aliases the record buffer.
                let f = self.file_num(file_num);
                self.call_rt("_rt_field_reset", &[f]);
                for fs in fields {
                    let w = self.long_arg(&fs.width);
                    let r = self.call_rt("_rt_field_next", &[f, w]);
                    self.bind_alias(&fs.target, r[0], r[1]);
                }
            }

            StmtKind::SetField {
                target,
                value,
                right,
            } => {
                // LSET writes *through* the field's existing pointer.
                let (sp, sl) = self.gen_expr(value).str();
                let (dp, dl) = self.gen_expr(&Self::lvalue_expr(target)).str();
                let f = if *right { "_rt_rset" } else { "_rt_lset" };
                self.call_rt(f, &[dp, dl, sp, sl]);
            }

            StmtKind::GetPut {
                file_num,
                record,
                is_put,
            } => {
                let f = self.file_num(file_num);
                // Record 0 means "the one after the last".
                let r = match record {
                    Some(e) => self.long_arg(e),
                    None => self.iconst(types::I64, 0),
                };
                let line = self.iconst(types::I64, i64::from(self.report_line()));
                let h = if *is_put {
                    "_rt_file_put"
                } else {
                    "_rt_file_get"
                };
                self.call_rt(h, &[f, r, line]);
            }

            StmtKind::Lock {
                file_num,
                range,
                is_unlock,
            } => {
                let f = self.file_num(file_num);
                // A missing range is the whole file, first record 0.
                let (s, e) = match range {
                    Some((s, e)) => {
                        let s = self.long_arg(s);
                        let e = match e {
                            Some(e) => self.long_arg(e),
                            // `LOCK #1, 5` locks exactly record 5.
                            None => s,
                        };
                        (s, e)
                    }
                    None => {
                        let z = self.iconst(types::I64, 0);
                        (z, z)
                    }
                };
                let line = self.iconst(types::I64, i64::from(self.report_line()));
                let h = if *is_unlock { "_rt_unlock" } else { "_rt_lock" };
                self.call_rt(h, &[f, s, e, line]);
            }

            StmtKind::Locate { row, col } => {
                let r = self.optional_arg(row.as_ref(), 1, None);
                let c = self.optional_arg(col.as_ref(), 1, None);
                self.call_rt("_rt_locate", &[r, c]);
            }

            StmtKind::Color { fg, bg } => {
                let f = self.optional_arg(fg.as_ref(), 0, Some(31));
                let b = self.optional_arg(bg.as_ref(), 0, Some(15));
                self.call_rt("_rt_color", &[f, b]);
            }

            StmtKind::Cls => {
                self.call_rt("_rt_cls", &[]);
            }
            StmtKind::Beep => {
                self.call_rt("_rt_beep", &[]);
            }
            _ => unreachable!("not an I/O statement"),
        }
    }

    /// One WRITE value: strings quoted, numbers bare.
    fn gen_write_expr(&mut self, expr: &Expr, sink: Value) {
        if self.expr_type(expr) == DataType::String {
            let q = self.iconst(types::I64, i64::from(b'"'));
            self.call_rt("_rt_file_print_char", &[sink, q]);
            self.gen_print_expr(expr, sink, false);
            let q = self.iconst(types::I64, i64::from(b'"'));
            self.call_rt("_rt_file_print_char", &[sink, q]);
        } else {
            self.gen_print_expr(expr, sink, false);
        }
    }

    /// Write one PRINT item to `sink`. `padded` gives a number the spacing
    /// PRINT gives it; WRITE passes false.
    fn gen_print_expr(&mut self, expr: &Expr, sink: Value, padded: bool) {
        // TAB() and SPC() position the cursor rather than producing a value.
        if let Expr::FnCall { name, args } = expr {
            let upper = name.to_uppercase();
            if (upper == "TAB" || upper == "SPC") && !self.cg.symbols.procs.contains_key(&upper) {
                self.gen_print_position(&upper, &args[0], sink);
                return;
            }
        }
        let ty = self.expr_type(expr);
        if ty == DataType::String {
            let (p, l) = self.gen_expr(expr).str();
            self.call_rt("_rt_file_print_string", &[sink, p, l]);
        } else {
            // A SINGLE prints with its own digits.
            let x = self.gen_double(expr);
            let h = match (ty == DataType::Single, padded) {
                (true, true) => "_rt_print_number_single",
                (true, false) => "_rt_file_print_single",
                (false, true) => "_rt_print_number",
                (false, false) => "_rt_file_print_float",
            };
            self.call_rt(h, &[sink, x]);
        }
    }

    /// `TAB(n)` / `SPC(n)`: move the write position within `sink`.
    pub(super) fn gen_print_position(&mut self, name: &str, arg: &Expr, sink: Value) {
        let n = self.gen_i64(arg);
        let h = if name == "TAB" {
            "_rt_file_print_tab"
        } else {
            "_rt_file_print_spc"
        };
        self.call_rt(h, &[sink, n]);
    }
}
