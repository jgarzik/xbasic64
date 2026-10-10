//! Statements: main, control flow, assignment, DATA

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use super::*;
use crate::lexer::normalized;

impl FnGen<'_> {
    /// Lower `main`: the module-level statements.
    pub(super) fn gen_main(&mut self, program: &Program) {
        // The handler reads and writes main's variables in memory.
        self.ssa_enabled = !self.cg.traps;
        self.branch_targets = ssa::branch_targets(&program.statements);
        self.reserve_array_descriptors();
        if self.cg.gosub_used {
            // The GOSUB stack grows down from its end.
            let top = self.global_addr("_gosub_stack", GOSUB_STACK_SIZE as u32);
            let top = self.b.ins().iadd_imm_s(top, GOSUB_STACK_SIZE);
            let sp = self.global_addr("_gosub_sp", 8);
            self.b.ins().store(MemFlagsData::trusted(), top, sp, 0);
        }
        // The console is file handle 0, and each platform's runtime seeds
        // that slot its own way.
        self.call_rt("_rt_platform_init", &[]);

        self.begin_gosub_scope(&program.statements);
        self.cg.main_gosub_scope = (self.gosub_first, self.gosub_blocks.len());
        for stmt in &program.statements {
            if !matches!(stmt.kind, StmtKind::Sub { .. } | StmtKind::Function { .. }) {
                self.gen_stmt(stmt);
            }
        }
        let zero = self.iconst(types::I32, 0);
        self.b.ins().return_(&[zero]);
        self.after_terminator();
    }

    /// The block of label `name` in this function, made on first mention.
    pub(super) fn label_block(&mut self, name: &str) -> Block {
        if let Some(b) = self.labels.get(name) {
            return *b;
        }
        let b = self.b.create_block();
        self.labels.insert(name.to_string(), b);
        b
    }

    /// A label: a block of its own when something branches to it, and
    /// otherwise nothing at all.
    fn gen_label(&mut self, target: &GotoTarget) {
        let name = storage::main_label(target);
        if self.branch_targets.contains(&name) {
            let b = self.label_block(&name);
            self.begin_block(b);
        }
    }

    /// The block a GOTO or GOSUB to `target` goes to.
    pub(super) fn target_block(&mut self, target: &GotoTarget) -> Block {
        let name = storage::main_label(target);
        self.label_block(&name)
    }

    /// Compile one statement, bracketed by its resume points when trapping.
    ///
    /// In a trapping program every statement, at every level and in every
    /// function, has a start block -- where RESUME goes back to -- and a next
    /// block, where RESUME NEXT goes on from; an error raised while it runs
    /// goes to its fault block. A nested statement is its own: an error in a
    /// FOR body's statement resumes there, one in the FOR's limit at the FOR.
    pub(super) fn gen_stmt(&mut self, stmt: &Stmt) {
        // Each statement has temporaries of its own, freed when it is done;
        // an enclosing statement's were freed before it branched here.
        let outer_temps = std::mem::take(&mut self.temps);
        let outer_copy = self.copy_loads;
        self.copy_loads = self.calls_user_function(stmt);
        self.err_line_block = None;
        self.gen_stmt_framed(stmt);
        // The enclosing statement's line is no longer the one stored.
        self.err_line_block = None;
        self.free_temps();
        self.temps = outer_temps;
        self.copy_loads = outer_copy;
    }

    /// `gen_stmt`, bracketed by its resume points when trapping.
    fn gen_stmt_framed(&mut self, stmt: &Stmt) {
        if !self.cg.traps {
            self.gen_stmt_inner(stmt);
            return;
        }
        let id = self.cg.stmt_count;
        self.cg.stmt_count += 1;
        let st = self.b.create_block();
        let sn = self.b.create_block();
        self.begin_block(st);
        let outer = self.cur_stmt.replace((id, st, sn));
        self.gen_stmt_inner(stmt);
        self.cur_stmt = outer;
        self.begin_block(sn);
    }

    fn gen_body(&mut self, body: &[Stmt]) {
        for s in body {
            self.gen_stmt(s);
        }
    }

    fn gen_stmt_inner(&mut self, stmt: &Stmt) {
        if stmt.line != 0 {
            self.current_line = stmt.line;
        }
        // ERL, recorded per statement: many file helpers have no line to
        // pass, and those are the errors ON ERROR is used to catch.
        if self.cg.traps && !matches!(stmt.kind, StmtKind::Label(_) | StmtKind::LabelName(_)) {
            let line = i64::from(self.report_line());
            self.store_rt_word("_err_line", line);
        }
        match &stmt.kind {
            StmtKind::Label(n) => {
                self.basic_line = *n;
                self.gen_label(&GotoTarget::Line(*n));
            }
            StmtKind::LabelName(name) => self.gen_label(&GotoTarget::Label(name.clone())),

            StmtKind::Let {
                name,
                indices: None,
                value,
            } if self.typed_var(name).is_some() && !self.has_plain_slot(name) => {
                let (loc, ty) = self.typed_storage(name).expect("guarded above");
                if let TypeRef::Record(_) = &ty {
                    // Assigning one whole record to another copies its words.
                    let words = self.cg.symbols.type_words(&ty);
                    let src = match value {
                        Expr::Variable(_) | Expr::ArrayAccess { .. } | Expr::FnCall { .. } => {
                            self.record_addr(value)
                        }
                        _ => None,
                    };
                    let _ = words;
                    if let Some(src) = src {
                        self.copy_record(loc, src, &ty);
                    }
                    return;
                }
                let v = self.gen_expr(value);
                self.store_typed(loc, &ty, v);
            }

            StmtKind::Let {
                name,
                indices: Some(indices),
                value,
            } => {
                // The element's address first, then the value, as GW-BASIC
                // evaluates them.
                let a = self.array_addr(name, indices);
                // An element of an array of records takes a whole record.
                if let Some(ty @ TypeRef::Record(_)) = self.array_elem_type(name) {
                    if let Some(src) = self.record_addr(value) {
                        self.copy_record(Loc::Ptr(a, 0), src, &ty);
                    }
                    return;
                }
                let v = self.gen_expr(value);
                let ty = self.array_value_type(name);
                self.store_typed(Loc::Ptr(a, 0), &ty, v);
            }

            StmtKind::Let {
                name,
                indices: None,
                value,
            } => {
                let v = self.gen_expr(value);
                let place = self.var_place(name);
                self.store_place(&place, v);
            }

            StmtKind::FieldAssign { target, value } => {
                let v = self.gen_expr(value);
                let (loc, ty) = self.lvalue_loc(target);
                self.store_typed(loc, &ty, v);
            }

            StmtKind::Print { .. }
            | StmtKind::Input { .. }
            | StmtKind::LineInput { .. }
            | StmtKind::Open { .. }
            | StmtKind::Close(_)
            | StmtKind::Width { .. }
            | StmtKind::Field { .. }
            | StmtKind::SetField { .. }
            | StmtKind::GetPut { .. }
            | StmtKind::Lock { .. }
            | StmtKind::Locate { .. }
            | StmtKind::Color { .. }
            | StmtKind::Cls
            | StmtKind::Beep => self.gen_io_stmt(stmt),

            StmtKind::Resume(_) | StmtKind::OnError(_) | StmtKind::RaiseError(_) => {
                self.gen_trap_stmt(stmt)
            }

            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let then_b = self.b.create_block();
                let else_b = self.b.create_block();
                let end = self.b.create_block();
                let c = self.gen_condition(condition);
                self.free_temps();
                self.brif(c, then_b, else_b);
                self.switch_sealed(then_b);
                self.gen_body(then_branch);
                self.b.ins().jump(end, &[]);
                self.switch_sealed(else_b);
                if let Some(eb) = else_branch {
                    self.gen_body(eb);
                }
                self.begin_sealed(end);
            }

            StmtKind::For {
                var,
                start,
                end,
                step,
                body,
            } => self.gen_for(var, start, end, step.as_ref(), body),

            StmtKind::While { condition, body } => {
                let head = self.b.create_block();
                let body_b = self.b.create_block();
                let exit = self.b.create_block();
                self.begin_block(head);
                let c = self.gen_condition(condition);
                self.free_temps();
                self.brif(c, body_b, exit);
                self.switch_sealed(body_b);
                // WHILE is a DO-family loop for EXIT DO purposes.
                self.loop_stack.push((false, exit));
                self.gen_body(body);
                self.loop_stack.pop();
                self.b.ins().jump(head, &[]);
                self.b.seal_block(head);
                self.switch_sealed(exit);
            }

            StmtKind::DoLoop {
                condition,
                cond_at_start,
                is_until,
                body,
            } => {
                let head = self.b.create_block();
                let exit = self.b.create_block();
                self.begin_block(head);
                if *cond_at_start && let Some(cond) = condition {
                    // DO WHILE leaves when false; DO UNTIL when true.
                    let c = self.gen_condition(cond);
                    self.free_temps();
                    let body_b = self.b.create_block();
                    if *is_until {
                        self.brif(c, exit, body_b);
                    } else {
                        self.brif(c, body_b, exit);
                    }
                    self.switch_sealed(body_b);
                }
                self.loop_stack.push((false, exit));
                self.gen_body(body);
                self.loop_stack.pop();
                match condition {
                    Some(cond) if !*cond_at_start => {
                        // LOOP WHILE repeats while true; LOOP UNTIL while false.
                        let c = self.gen_condition(cond);
                        self.free_temps();
                        if *is_until {
                            self.brif(c, exit, head);
                        } else {
                            self.brif(c, head, exit);
                        }
                    }
                    _ => {
                        self.b.ins().jump(head, &[]);
                    }
                }
                self.b.seal_block(head);
                self.switch_sealed(exit);
            }

            StmtKind::Goto(target) => {
                let b = self.target_block(target);
                self.jump(b);
            }

            StmtKind::Gosub(target) => {
                let b = self.target_block(target);
                let ret = self.gosub_push();
                self.jump(b);
                self.b.switch_to_block(ret);
            }

            StmtKind::Return(target) => self.gen_return(target.as_ref()),

            StmtKind::OnGoto { expr, targets } => {
                // A selector outside 1..=n matches nothing and falls through.
                let sel = self.on_selector(expr);
                let after = self.b.create_block();
                let blocks: Vec<Block> = targets.iter().map(|t| self.target_block(t)).collect();
                let idx = self.b.ins().iadd_imm_s(sel, -1);
                self.br_table(idx, &blocks, after);
                self.switch_sealed(after);
            }

            StmtKind::OnGosub { expr, targets } => {
                // One return site serves the whole statement, and is pushed
                // only when the selector picks a subroutine: a GOSUB that
                // never happened must not leave a frame behind.
                let sel = self.on_selector(expr);
                let push = self.b.create_block();
                let none = self.b.create_block();
                let lo = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, sel, 1);
                let hi =
                    self.b
                        .ins()
                        .icmp_imm_s(IntCC::SignedGreaterThan, sel, targets.len() as i64);
                let out = self.b.ins().bor(lo, hi);
                self.brif(out, none, push);
                self.switch_sealed(push);
                let ret = self.gosub_push();
                let blocks: Vec<Block> = targets.iter().map(|t| self.target_block(t)).collect();
                let idx = self.b.ins().iadd_imm_s(sel, -1);
                self.br_table(idx, &blocks, ret);
                self.switch_sealed(none);
                self.begin_block(ret);
            }

            StmtKind::Dim { decls, .. } => {
                for decl in decls {
                    self.gen_declarator(decl, false, false);
                }
            }
            StmtKind::Redim { decls, preserve } => {
                for decl in decls {
                    self.gen_declarator(decl, true, *preserve);
                }
            }

            StmtKind::Call { name, args } => {
                self.gen_call(name, args);
            }

            StmtKind::Read(vars) => {
                for var in vars {
                    // The line goes along for Out of DATA.
                    let line = self.iconst(types::I64, i64::from(self.report_line()));
                    let v = if self.expr_type(&Self::lvalue_expr(var)) == DataType::String {
                        let r = self.call_rt("_rt_read_string", &[line]);
                        Val::Str(r[0], r[1])
                    } else {
                        let r = self.call_rt("_rt_read_number", &[line]);
                        Val::Num(DataType::Double, r[0])
                    };
                    self.store_lvalue(var, v);
                }
            }

            StmtKind::Restore(target) => {
                // RESTORE <line> resumes at the first DATA item at or after
                // that line.
                let idx = match target {
                    Some(GotoTarget::Line(n)) => {
                        self.cg.data_line_index.get(n).copied().unwrap_or(0)
                    }
                    Some(GotoTarget::Label(name)) => self
                        .cg
                        .data_label_index
                        .get(normalized(name))
                        .copied()
                        .unwrap_or(0),
                    None => 0,
                };
                let i = self.iconst(types::I64, idx as i64);
                self.call_rt("_rt_restore", &[i]);
            }

            StmtKind::MidAssign {
                target,
                start,
                len,
                value,
            } => {
                let cur = self.gen_expr(&Self::lvalue_expr(target));
                let (tp, tl) = cur.str();
                let s = self.gen_i64(start);
                let n = match len {
                    Some(e) => self.gen_i64(e),
                    // No length given: replace as much as the value provides.
                    None => self.iconst(types::I64, 0x7FFF_FFFF),
                };
                let (vp, vl) = self.gen_expr(value).str();
                self.call_rt("_rt_mid_assign", &[tp, tl, s, n, vp, vl]);
            }

            // Both are read before either is written, so `SWAP A(I), A(J)`
            // is right even when the subscripts alias.
            StmtKind::Swap(a, b) => {
                let (ea, eb) = (Self::lvalue_expr(a), Self::lvalue_expr(b));
                if let Some(ty) = self.record_type_of(&ea) {
                    // Two records exchange every word, their strings' with
                    // the rest: each string stays owned by exactly one.
                    let pa = self.record_addr(&ea).expect("a record lvalue");
                    let pb = self.record_addr(&eb).expect("sema matched the types");
                    for w in 0..self.cg.symbols.type_words(&ty) {
                        let flags = MemFlagsData::trusted();
                        let va = self.b.ins().load(types::I64, flags, pa, 8 * w);
                        let vb = self.b.ins().load(types::I64, flags, pb, 8 * w);
                        self.b.ins().store(flags, vb, pa, 8 * w);
                        self.b.ins().store(flags, va, pb, 8 * w);
                    }
                } else if self.expr_type(&ea) == DataType::String {
                    let (la, ta) = self.lvalue_loc(a);
                    let (lb, tb) = self.lvalue_loc(b);
                    let fixed = |t: &TypeRef| matches!(t, TypeRef::FixedString(1..));
                    if fixed(&ta) || fixed(&tb) {
                        // A fixed-length string keeps its width, so each side
                        // takes a copy of the other's value, stored -- and
                        // padded or cut -- as an assignment would.
                        let (pa, la_len) = self.load_str(la);
                        let (pb, lb_len) = self.load_str(lb);
                        let ca = self.call_rt("_rt_strdup", &[pa, la_len]);
                        let cb = self.call_rt("_rt_strdup", &[pb, lb_len]);
                        self.store_typed(la, &ta, Val::Str(cb[0], cb[1]));
                        self.store_typed(lb, &tb, Val::Str(ca[0], ca[1]));
                    } else {
                        // A string's two words move as they are, ownership
                        // and all: neither side is copied, and nothing is
                        // freed.
                        let wa = self.load_str_raw(la);
                        let wb = self.load_str_raw(lb);
                        self.store_str_raw(wb, la);
                        self.store_str_raw(wa, lb);
                    }
                } else {
                    let va = self.gen_expr(&Self::lvalue_expr(a));
                    let vb = self.gen_expr(&Self::lvalue_expr(b));
                    self.store_lvalue_raw(a, vb);
                    self.store_lvalue_raw(b, va);
                }
            }

            StmtKind::ExitLoop { is_for } => {
                let target = self
                    .loop_stack
                    .iter()
                    .rev()
                    .find(|(f, _)| f == is_for)
                    .map(|(_, b)| *b);
                if let Some(b) = target {
                    self.jump(b);
                }
            }

            StmtKind::ExitProc => {
                if let Some(b) = self.exit_block {
                    self.jump(b);
                }
            }

            StmtKind::Erase(names) => {
                for name in names {
                    let Some(info) = self.lookup_array(name) else {
                        continue; // sema has already complained
                    };
                    // ERASE releases the storage, and the strings in it, and
                    // nulls the descriptor, so the array reads as
                    // undimensioned again.
                    self.free_array_storage(name, info.loc, info.ndims);
                    let z = self.iconst(types::I64, 0);
                    self.store(z, info.loc);
                }
            }

            StmtKind::Randomize(seed) => {
                // With no seed, take the clock: there is nobody to ask.
                let s = match seed {
                    Some(e) => self.gen_double(e),
                    None => self.call_rt("_rt_timer", &[])[0],
                };
                self.call_rt("_rt_randomize", &[s]);
            }

            StmtKind::SelectCase { expr, cases } => self.gen_select(expr, cases),

            // STOP reports where it stopped, then ends as END does.
            StmtKind::Stop => {
                let line = self.iconst(types::I64, i64::from(self.report_line()));
                self.call_rt("_rt_stop", &[line]);
            }
            // END ends the program, from any frame.
            StmtKind::End => {
                self.call_rt("_rt_end", &[]);
            }

            // Decided before code generation, or consulted on lookup.
            StmtKind::Sub { .. }
            | StmtKind::Function { .. }
            | StmtKind::Data(_)
            | StmtKind::Shared(_)
            | StmtKind::Static(_)
            | StmtKind::Declare { .. }
            | StmtKind::TypeDef { .. }
            | StmtKind::Const { .. }
            | StmtKind::OptionBase(_)
            | StmtKind::DefType { .. } => {}
        }
    }

    /// The record type an lvalue expression denotes, if it denotes a whole
    /// record: a record variable, an element of an array of records, or a
    /// field that is itself a record.
    fn record_type_of(&self, e: &Expr) -> Option<TypeRef> {
        let ty = match e {
            Expr::Variable(n) => self.typed_var(n)?,
            Expr::ArrayAccess { name, .. } | Expr::FnCall { name, .. } => {
                self.array_elem_type(name)?
            }
            Expr::Field { .. } => {
                if let Some((name, _, fields)) = Self::flatten_indexed_field_path(e) {
                    let base = self.array_elem_type(&name)?;
                    self.field_byte_offset(&base, &fields)?.1
                } else {
                    let (name, fields) = Self::flatten_field_path(e)?;
                    let base = self.typed_var(&name)?;
                    self.field_byte_offset(&base, &fields)?.1
                }
            }
            _ => return None,
        };
        matches!(ty, TypeRef::Record(_)).then_some(ty)
    }

    /// Store a number into a target: SWAP's numeric half.
    fn store_lvalue_raw(&mut self, target: &LValue, v: Val) {
        let place = self.lvalue_place(target);
        self.store_place(&place, v);
    }

    // ---- GOSUB ----

    /// Push the number of this GOSUB's return site; its block, which the
    /// caller continues in after the jump.
    fn gosub_push(&mut self) -> Block {
        let i = self.gosub_next;
        self.gosub_next += 1;
        let site = (self.gosub_first + i) as i64;
        let block = self.gosub_blocks[i];
        let spa = self.global_addr("_gosub_sp", 8);
        let sp = self
            .b
            .ins()
            .load(types::I64, MemFlagsData::trusted(), spa, 0);
        let sp = self.b.ins().iadd_imm_s(sp, -8);
        let base = self.global_addr("_gosub_stack", GOSUB_STACK_SIZE as u32);
        let full = self.b.ins().icmp(IntCC::UnsignedLessThan, sp, base);
        self.check(full, RtError::GosubOverflow);
        let n = self.iconst(types::I64, site);
        self.b.ins().store(MemFlagsData::trusted(), n, sp, 0);
        let spa = self.global_addr("_gosub_sp", 8);
        self.b.ins().store(MemFlagsData::trusted(), sp, spa, 0);
        block
    }

    /// RETURN: pop a return site number and dispatch on it -- or, for
    /// `RETURN line`, drop it and go to the line.
    ///
    /// The number must be one of this scope's: an empty stack is a RETURN
    /// nobody GOSUBed to, and another scope's site would jump into code whose
    /// frame is not the live one. Both are "RETURN without GOSUB", and the
    /// stack is left alone so that a handler sees it as it was.
    fn gen_return(&mut self, target: Option<&GotoTarget>) {
        let spa = self.global_addr("_gosub_sp", 8);
        let sp = self
            .b
            .ins()
            .load(types::I64, MemFlagsData::trusted(), spa, 0);
        let base = self.global_addr("_gosub_stack", GOSUB_STACK_SIZE as u32);
        let top = self.b.ins().iadd_imm_s(base, GOSUB_STACK_SIZE);
        let empty = self
            .b
            .ins()
            .icmp(IntCC::UnsignedGreaterThanOrEqual, sp, top);
        self.check(empty, RtError::ReturnWithoutGosub);
        let site = self
            .b
            .ins()
            .load(types::I64, MemFlagsData::trusted(), sp, 0);
        let rel = self.b.ins().iadd_imm_s(site, -(self.gosub_first as i64));
        let count = self.gosub_blocks.len() as i64;
        let foreign = self
            .b
            .ins()
            .icmp_imm_s(IntCC::UnsignedGreaterThanOrEqual, rel, count);

        if self.kind == FnKind::Handler {
            // A handler that RETURNs from the GOSUB the error interrupted
            // leaves the handler for main: the site is main's, so main goes
            // there.
            let main_site = self.b.create_block();
            let own = self.b.create_block();
            self.brif(foreign, main_site, own);
            self.switch_sealed(main_site);
            let (mfirst, mcount) = self.cg.main_gosub_scope;
            let mrel = self.b.ins().iadd_imm_s(site, -(mfirst as i64));
            let bad =
                self.b
                    .ins()
                    .icmp_imm_s(IntCC::UnsignedGreaterThanOrEqual, mrel, mcount as i64);
            self.check(bad, RtError::ReturnWithoutGosub);
            let next = self.b.ins().iadd_imm_s(sp, 8);
            let spa = self.global_addr("_gosub_sp", 8);
            self.b.ins().store(MemFlagsData::trusted(), next, spa, 0);
            let rl = self.rt_label_addr();
            match target {
                None => {
                    self.b.ins().store(MemFlagsData::trusted(), site, rl, 0);
                    self.store_rt_word("_err_pending", 3);
                }
                Some(t) => {
                    let idx = self.resume_index(t);
                    let i = self.iconst(types::I64, idx as i64);
                    self.b.ins().store(MemFlagsData::trusted(), i, rl, 0);
                    self.store_rt_word("_err_pending", 2);
                }
            }
            self.store_rt_word("_err_active", 0);
            self.handler_return(3);
            self.switch_sealed(own);
        } else {
            self.check(foreign, RtError::ReturnWithoutGosub);
        }
        let next = self.b.ins().iadd_imm_s(sp, 8);
        let spa = self.global_addr("_gosub_sp", 8);
        self.b.ins().store(MemFlagsData::trusted(), next, spa, 0);
        match target {
            None => {
                let blocks = self.gosub_blocks.clone();
                let trap = self.b.create_block();
                self.br_table(rel, &blocks, trap);
                // Unreachable once the range is checked; with checks off a
                // foreign site lands here.
                let cur = self.b.current_block().expect("a block");
                self.switch_sealed(trap);
                self.b.ins().trap(TrapCode::unwrap_user(1));
                self.b.switch_to_block(cur);
            }
            Some(t) => {
                let b = self.target_block(t);
                self.jump(b);
            }
        }
    }

    /// An ON selector as an i64: a byte, as in GW-BASIC. 0 and past the list
    /// fall through; a value no byte holds is Illegal function call.
    fn on_selector(&mut self, expr: &Expr) -> Value {
        let n = self.gen_i64(expr);
        self.free_temps();
        self.check_range(n, 0, Some(255), RtError::Domain);
        n
    }

    // ---- FOR ----

    /// Where a FOR control variable lives, and the type it counts in:
    /// resolved as a read of the name is, so the loop writes where every
    /// other reference looks.
    fn for_control_var(&mut self, name: &str) -> (Place, DataType) {
        let place = self.var_place(name);
        let ty = match &place {
            Place::Var(_, t) => *t,
            Place::Mem(_, ty) => DataType::from_type_ref(ty),
        };
        (place, ty)
    }

    /// FOR: the control variable counts in its own type. The start is
    /// stored, then the limit and the step are each evaluated once, into
    /// variables of their own.
    fn gen_for(&mut self, var: &str, start: &Expr, end: &Expr, step: Option<&Expr>, body: &[Stmt]) {
        let (place, ct) = self.for_control_var(var);
        let s = self.gen_expr(start);
        let s = self.coerce(s, ct);
        self.store_place(&place, s);

        let ty = num_type(ct);
        let e = self.gen_expr(end);
        let e = self.coerce(e, ct).num();
        // Main's handler is main compiled a second time, and may GOTO into
        // a loop main started: in a trapping program both copies keep the
        // loop's limit and step in the same place, so a NEXT in either
        // sees them. Anywhere else they are SSA values.
        let shared = (self.cg.traps && self.kind != FnKind::Proc).then(|| {
            let key = end as *const Expr as usize;
            let n = self.cg.for_slots.len();
            match self.cg.for_slots.get(&key) {
                Some(id) => *id,
                None => {
                    let id = self.cg.global(&format!("_for_{}", n), 16);
                    self.cg.for_slots.insert(key, id);
                    id
                }
            }
        });
        let limit_var = self.b.declare_var(ty);
        self.b.def_var(limit_var, e);
        if let Some(id) = shared {
            self.store_num(ct, e, Loc::Data(id, 0));
        }

        let st = match step {
            Some(x) => {
                let v = self.gen_expr(x);
                self.coerce(v, ct).num()
            }
            None => {
                let one = Val::Num(DataType::Integer, self.iconst(types::I32, 1));
                self.coerce(one, ct).num()
            }
        };
        let step_var = self.b.declare_var(ty);
        self.b.def_var(step_var, st);
        if let Some(id) = shared {
            self.store_num(ct, st, Loc::Data(id, 8));
        }
        self.free_temps();

        // Which way the loop runs is the step's sign, almost always written
        // into the program and so decided here.
        let direction = match step {
            None => Some(true),
            Some(s) => self.const_f64(s).map(|v| v >= 0.0),
        };

        let head = self.b.create_block();
        let body_b = self.b.create_block();
        let exit = self.b.create_block();
        self.begin_block(head);
        let counter = self.load_place(&place).num();
        let limit = match shared {
            Some(id) => self.load_num(ct, Loc::Data(id, 0)),
            None => self.b.use_var(limit_var),
        };
        let past = |f: &mut Self, ascending: bool| -> Value {
            if ct.is_integer() {
                let cc = if ascending {
                    IntCC::SignedGreaterThan
                } else {
                    IntCC::SignedLessThan
                };
                f.b.ins().icmp(cc, counter, limit)
            } else {
                // Ascending, past means above; descending, below -- or
                // unordered.
                let cc = if ascending {
                    FloatCC::GreaterThan
                } else {
                    FloatCC::UnorderedOrLessThan
                };
                f.b.ins().fcmp(cc, counter, limit)
            }
        };
        match direction {
            Some(asc) => {
                let p = past(self, asc);
                self.brif(p, exit, body_b);
            }
            None => {
                let stepv = match shared {
                    Some(id) => self.load_num(ct, Loc::Data(id, 8)),
                    None => self.b.use_var(step_var),
                };
                let neg = if ct.is_integer() {
                    self.b.ins().icmp_imm_s(IntCC::SignedLessThan, stepv, 0)
                } else {
                    let z = if ct == DataType::Single {
                        self.b.ins().f32const(0.0)
                    } else {
                        self.f64const(0.0)
                    };
                    self.b.ins().fcmp(FloatCC::UnorderedOrLessThan, stepv, z)
                };
                let up = past(self, true);
                let down = past(self, false);
                let p = self.b.ins().select(neg, down, up);
                self.brif(p, exit, body_b);
            }
        }
        self.switch_sealed(body_b);
        self.loop_stack.push((true, exit));
        self.gen_body(body);
        self.loop_stack.pop();

        // The increment reloads the counter, which the body may assign. It
        // is done at the counter's own width, so a step past the type's
        // range is Overflow rather than a loop that never ends.
        let c = self.load_place(&place).num();
        let stepv = match shared {
            Some(id) => self.load_num(ct, Loc::Data(id, 8)),
            None => self.b.use_var(step_var),
        };
        let next = match ct {
            DataType::Integer => {
                let a = self.b.ins().ireduce(types::I16, c);
                let b = self.b.ins().ireduce(types::I16, stepv);
                let (r, of) = self.b.ins().sadd_overflow(a, b);
                self.check(of, RtError::Overflow);
                self.b.ins().sextend(types::I32, r)
            }
            DataType::Long => {
                let (r, of) = self.b.ins().sadd_overflow(c, stepv);
                self.check(of, RtError::Overflow);
                r
            }
            _ => self.b.ins().fadd(c, stepv),
        };
        self.store_place(&place, Val::Num(ct, next));
        self.b.ins().jump(head, &[]);
        self.b.seal_block(head);
        self.switch_sealed(exit);
    }

    /// A numeric constant known at compile time: a literal, a CONST, or a
    /// negated one.
    pub(super) fn const_f64(&self, expr: &Expr) -> Option<f64> {
        let lit = match expr {
            Expr::Literal(lit) => lit,
            Expr::Variable(name) => self.cg.symbols.consts.get(normalized(name))?,
            Expr::Unary {
                op: UnaryOp::Neg,
                operand,
            } => return self.const_f64(operand).map(|v| -v),
            _ => return None,
        };
        lit.as_f64()
    }

    // ---- SELECT CASE ----

    fn gen_select(&mut self, expr: &Expr, cases: &[(Option<Vec<CaseClause>>, Vec<Stmt>)]) {
        let end = self.b.create_block();
        // The selector is evaluated once, into a slot: a string as its pair,
        // a number as a Double.
        let is_string = self.expr_type(expr) == DataType::String;
        let v = self.gen_expr(expr);
        let slot = self.stack_slot(16);
        if is_string {
            // Owned for the whole statement: a CASE body may assign the
            // variable it was read from, and a later CASE still compares.
            let z = self.iconst(types::I64, 0);
            self.store_str_raw((z, z), Loc::Stack(slot, 0));
            self.assign_str(Loc::Stack(slot, 0), v);
        } else {
            let d = self.double_of(v);
            self.b.ins().stack_store(types::I64, d, slot, 0);
        }
        self.free_temps();

        for (clauses, body) in cases {
            let next = self.b.create_block();
            if let Some(clauses) = clauses {
                // Any alternative matching enters the body; all failing
                // moves on to the next CASE.
                let body_b = self.b.create_block();
                for clause in clauses {
                    self.gen_case_clause(clause, slot, is_string, body_b);
                }
                self.b.ins().jump(next, &[]);
                self.switch_sealed(body_b);
            }
            self.gen_body(body);
            self.b.ins().jump(end, &[]);
            self.switch_sealed(next);
        }
        self.begin_sealed(end);
        if is_string {
            self.free_slot_str(Loc::Stack(slot, 0));
        }
    }

    /// One CASE alternative: go to `body` when it matches.
    fn gen_case_clause(
        &mut self,
        clause: &CaseClause,
        slot: StackSlot,
        is_string: bool,
        body: Block,
    ) {
        // The selector compared with `e`: an i8 for `op`.
        let cmp = |f: &mut Self, op: BinaryOp, e: &Expr| -> Value {
            if is_string {
                let (rp, rl) = f.gen_expr(e).str();
                let (lp, ll) = f.load_str(Loc::Stack(slot, 0));
                let c = f.call_rt("_rt_strcmp", &[lp, ll, rp, rl])[0];
                f.free_temps();
                f.b.ins().icmp_imm_s(expr::int_cc(op), c, 0)
            } else {
                let x = f.gen_double(e);
                f.free_temps();
                let sel = f.b.ins().stack_load(types::I64, types::F64, slot, 0);
                f.b.ins().fcmp(expr::float_cc(op), sel, x)
            }
        };
        let next = self.b.create_block();
        match clause {
            CaseClause::Value(e) => {
                let c = cmp(self, BinaryOp::Eq, e);
                self.brif(c, body, next);
            }
            CaseClause::Range(lo, hi) => {
                // Inclusive at both ends; the low bound first.
                let c = cmp(self, BinaryOp::Ge, lo);
                let test_hi = self.b.create_block();
                self.brif(c, test_hi, next);
                self.switch_sealed(test_hi);
                let c = cmp(self, BinaryOp::Le, hi);
                self.brif(c, body, next);
            }
            CaseClause::Compare(op, e) => {
                let c = cmp(self, *op, e);
                self.brif(c, body, next);
            }
        }
        self.switch_sealed(next);
    }
}
