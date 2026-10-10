//! Error trapping: ON ERROR, RESUME, and the handler function
//!
//! A trapped `_rt_error` sets `_err_pending` to 1 and returns. Generated code
//! tests the flag after every call that can fail, and an inline check's error
//! block jumps on after calling `_rt_error`; either way the statement's fault
//! block runs. It calls `_err_handler_fn` -- main's statements compiled a
//! second time, entered at the armed ON ERROR target -- on top of the frame
//! that faulted, as QuickBASIC does, and the handler's answer says what next:
//! 1 retry the statement, 2 go past it, 3 leave this function on the way to
//! main: a RESUME label (`_err_pending` 2) or the return site of a GOSUB the
//! handler RETURNed from (3). Each procedure then leaves through its
//! epilogue, and main dispatches on `_err_resume_label`.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use super::*;

impl FnGen<'_> {
    /// Where a trapped error goes from here: the current statement's fault
    /// block. Outside any statement -- a procedure's prologue -- the function
    /// is left, and its caller's statement faults instead.
    pub(super) fn error_exit(&mut self) -> Block {
        match self.cur_stmt {
            Some((id, st, sn)) => {
                if let Some((f, _, _)) = self.faults.get(&id) {
                    return *f;
                }
                let f = self.b.create_block();
                self.b.set_cold_block(f);
                self.faults.insert(id, (f, st, sn));
                f
            }
            None => self.unwind(),
        }
    }

    /// Where a trapped error at this check site goes: the error exit, or --
    /// when the statement has strings live here -- a cold block of the
    /// site's own that frees them first. Whichever way the handler resumes,
    /// the statement is done with them: a retry makes them again.
    pub(super) fn trap_exit(&mut self) -> Block {
        let exit = self.error_exit();
        if self.temps.is_empty() && self.str_slots.is_empty() {
            return exit;
        }
        let b = self.b.create_block();
        self.b.set_cold_block(b);
        let (temps, slots) = (self.temps.clone(), self.str_slots.clone());
        self.cleanups.push((b, exit, temps, slots));
        b
    }

    /// The cold blocks `trap_exit` made. Each has one predecessor, its
    /// check site, which every value it frees dominates.
    pub(super) fn fill_cleanups(&mut self) {
        for (block, exit, temps, slots) in std::mem::take(&mut self.cleanups) {
            self.b.switch_to_block(block);
            for p in temps {
                self.call("free", &[p]);
            }
            for loc in slots {
                self.free_slot_str(loc);
            }
            self.b.ins().jump(exit, &[]);
        }
    }

    /// Where this function goes to be left on the way to a RESUME label.
    fn unwind(&mut self) -> Block {
        if self.kind == FnKind::Proc {
            return self.exit_block.expect("a procedure has an exit");
        }
        match self.unwind_block {
            Some(b) => b,
            None => {
                let b = self.b.create_block();
                self.b.set_cold_block(b);
                self.unwind_block = Some(b);
                b
            }
        }
    }

    /// The blocks RESUME and RESUME NEXT jump to when the error was in the
    /// handler's own code, made on first use and filled with the rest.
    fn local_resume_blocks(&mut self) -> (Block, Block) {
        if let Some(b) = self.local_resume {
            return b;
        }
        let b = (self.b.create_block(), self.b.create_block());
        self.local_resume = Some(b);
        b
    }

    /// The fault blocks of this function's statements.
    pub(super) fn fill_fault_blocks(&mut self) {
        let mut faults: Vec<_> = self.faults.iter().map(|(id, b)| (*id, *b)).collect();
        faults.sort_by_key(|(id, _)| *id);
        if self.kind == FnKind::Handler {
            self.fill_local_faults(&faults);
            return self.fill_unwind();
        }
        for (_, (fault, st, sn)) in faults {
            let unwind = self.unwind();
            self.b.switch_to_block(fault);
            let p = self.load_rt_word("_err_pending");
            let trapped = self.b.ins().icmp_imm_s(IntCC::Equal, p, 1);
            let handle = self.b.create_block();
            self.brif(trapped, handle, unwind);
            self.b.switch_to_block(handle);
            self.store_rt_word("_err_pending", 0);
            let id = self.cg.funcs["_err_handler_fn"];
            let fref = self.func_ref(id);
            let inst = self.b.ins().call(fref, &[]);
            let r = self.b.inst_results(inst)[0];
            let not1 = self.b.create_block();
            let is1 = self.b.ins().icmp_imm_s(IntCC::Equal, r, 1);
            self.brif(is1, st, not1);
            self.b.switch_to_block(not1);
            let is2 = self.b.ins().icmp_imm_s(IntCC::Equal, r, 2);
            self.brif(is2, sn, unwind);
        }
        self.fill_unwind();
    }

    /// The handler's fault blocks: an error in its own code goes back to
    /// its entry, noting the statement, and RESUME then returns there.
    fn fill_local_faults(&mut self, faults: &[(usize, (Block, Block, Block))]) {
        let local = self.local_fault.expect("in the handler");
        let dispatch = self.handler_dispatch.expect("in the handler");
        for (id, (fault, _, _)) in faults {
            let unwind = self.unwind();
            self.b.switch_to_block(*fault);
            let p = self.load_rt_word("_err_pending");
            let trapped = self.b.ins().icmp_imm_s(IntCC::Equal, p, 1);
            let handle = self.b.create_block();
            self.brif(trapped, handle, unwind);
            self.b.switch_to_block(handle);
            self.store_rt_word("_err_pending", 0);
            let n = self.iconst(types::I64, *id as i64 + 1);
            self.b.def_var(local, n);
            self.b.ins().jump(dispatch, &[]);
        }
        if let Some((same_b, next_b)) = self.local_resume {
            let first = faults.first().map_or(0, |(id, _)| *id);
            for (block, next) in [(same_b, false), (next_b, true)] {
                self.b.switch_to_block(block);
                let lf = self.b.use_var(local);
                let zero = self.iconst(types::I64, 0);
                self.b.def_var(local, zero);
                let idx = self.b.ins().iadd_imm_s(lf, -(first as i64) - 1);
                let last = faults.last().map_or(0, |(id, _)| *id);
                let bad = self.b.create_block();
                let targets: Vec<Block> = (first..=last)
                    .map(|id| match faults.iter().find(|(i, _)| *i == id) {
                        Some((_, (_, st, sn))) => {
                            if next {
                                *sn
                            } else {
                                *st
                            }
                        }
                        None => bad,
                    })
                    .collect();
                self.br_table(idx, &targets, bad);
                self.b.switch_to_block(bad);
                self.b.ins().trap(TrapCode::unwrap_user(1));
            }
        }
    }

    /// Main's unwind block, or the handler's.
    fn fill_unwind(&mut self) {
        // Main's unwind: a RESUME <label> arrives here once every procedure
        // between has left, and goes on to the label, by number -- or to a
        // GOSUB return site, when the handler RETURNed (`_err_pending` 3).
        if let Some(u) = self.unwind_block {
            self.b.switch_to_block(u);
            match self.kind {
                FnKind::Main => {
                    let p = self.load_rt_word("_err_pending");
                    self.store_rt_word("_err_pending", 0);
                    let rl = self.rt_label_addr();
                    let idx = self
                        .b
                        .ins()
                        .load(types::I64, MemFlagsData::trusted(), rl, 0);
                    let to_site = self.b.create_block();
                    let to_label = self.b.create_block();
                    let is3 = self.b.ins().icmp_imm_s(IntCC::Equal, p, 3);
                    self.brif(is3, to_site, to_label);
                    let bad = self.b.create_block();

                    self.b.switch_to_block(to_label);
                    let labels: Vec<String> = self.cg.resume_targets.clone();
                    let blocks: Vec<Block> = labels.iter().map(|l| self.label_block(l)).collect();
                    self.br_table(idx, &blocks, bad);

                    self.b.switch_to_block(to_site);
                    let rel = self
                        .b
                        .ins()
                        .iadd_imm_s(idx, -(self.cg.main_gosub_scope.0 as i64));
                    let sites = self.gosub_blocks.clone();
                    self.br_table(rel, &sites, bad);

                    self.b.switch_to_block(bad);
                    self.b.ins().trap(TrapCode::unwrap_user(1));
                }
                FnKind::Handler => self.handler_return(3),
                FnKind::Proc => unreachable!("a procedure unwinds through its epilogue"),
            }
        }
    }

    /// The address of `_err_resume_label`, this program's: what main's
    /// unwind dispatches on.
    pub(super) fn rt_label_addr(&mut self) -> Value {
        self.global_addr("_err_resume_label", 8)
    }

    /// The index of main's `target` in the RESUME targets.
    pub(super) fn resume_index(&mut self, target: &GotoTarget) -> usize {
        let label = storage::main_label(target);
        self.cg
            .resume_targets
            .iter()
            .position(|l| *l == label)
            .expect("preprocess collected every RESUME and RETURN target")
    }

    /// Leave the handler with `code`.
    pub(super) fn handler_return(&mut self, code: i64) {
        let r = self.iconst(types::I32, code);
        let ret = self.handler_ret.expect("in the handler");
        self.b.ins().jump(ret, &[BlockArg::Value(r)]);
        self.after_terminator();
    }

    /// `_err_handler_fn`: main's statements compiled a second time.
    ///
    /// Every statement of main is here, not just the handler's, since a
    /// handler may GOTO anywhere in the module and is still the handler until
    /// it RESUMEs; running off the end of the program ends it, as in main.
    pub(super) fn gen_handler(&mut self, program: &Program) {
        let ret = self.b.create_block();
        self.b.append_block_param(ret, types::I32);
        self.handler_ret = Some(ret);
        self.branch_targets = ssa::branch_targets(&program.statements);

        // An error raised in the handler's own code -- which it reaches by
        // GOTO, re-armed, and runs as the program from then on -- comes back
        // here rather than calling the handler again from inside itself:
        // that nested one call deeper per error, and a program that kept
        // erroring overflowed the stack. `local_fault` says which statement
        // it was, for RESUME.
        let local = self.b.declare_var(types::I64);
        let zero = self.iconst(types::I64, 0);
        self.b.def_var(local, zero);
        self.local_fault = Some(local);
        let dispatch = self.b.create_block();
        self.handler_dispatch = Some(dispatch);
        self.begin_block(dispatch);

        // Enter at the armed target, by number.
        let h = self.load_rt_word("_err_handler");
        let idx = self.b.ins().iadd_imm_s(h, -1);
        let names = self.cg.err_handlers.clone();
        let entries: Vec<Block> = names.iter().map(|l| self.label_block(l)).collect();
        let bad = self.b.create_block();
        self.br_table(idx, &entries, bad);

        self.begin_gosub_scope(&program.statements);
        for stmt in &program.statements {
            if !matches!(stmt.kind, StmtKind::Sub { .. } | StmtKind::Function { .. }) {
                self.gen_stmt(stmt);
            }
        }
        self.call_rt("_rt_end", &[]);

        self.b.switch_to_block(bad);
        self.b.ins().trap(TrapCode::unwrap_user(1));

        self.b.switch_to_block(ret);
        let r = self.b.block_params(ret)[0];
        self.b.ins().return_(&[r]);
        self.after_terminator();
    }

    /// ON ERROR, RESUME and ERROR.
    pub(super) fn gen_trap_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            // RESUME returns from the handler to the fault site with what to
            // do. Anywhere else no handler can be running, so it is "RESUME
            // without error", which is never trapped -- and raised whatever
            // `--unsafe` says.
            StmtKind::Resume(target) => {
                if self.kind != FnKind::Handler {
                    self.raise(RtError::ResumeNoError);
                    return;
                }
                let active = self.load_rt_word("_err_active");
                let idle = self.b.ins().icmp_imm_s(IntCC::Equal, active, 0);
                self.raise_if(idle, RtError::ResumeNoError);
                self.store_rt_word("_err_active", 0);
                match target {
                    ResumeTarget::Same | ResumeTarget::Next => {
                        // Back to a statement of the handler's own code, if
                        // that is where the error was; otherwise to the
                        // fault site that called the handler.
                        let next = matches!(target, ResumeTarget::Next);
                        let (same_b, next_b) = self.local_resume_blocks();
                        let local = self.local_fault.expect("in the handler");
                        let lf = self.b.use_var(local);
                        let here = self.b.create_block();
                        let away = self.b.create_block();
                        self.brif(lf, here, away);
                        self.switch_sealed(here);
                        self.jump(if next { next_b } else { same_b });
                        self.switch_sealed(away);
                        self.handler_return(if next { 2 } else { 1 });
                    }
                    ResumeTarget::At(t) => {
                        let idx = self.resume_index(t);
                        let rl = self.rt_label_addr();
                        let i = self.iconst(types::I64, idx as i64);
                        self.b.ins().store(MemFlagsData::trusted(), i, rl, 0);
                        self.store_rt_word("_err_pending", 2);
                        self.handler_return(3);
                    }
                }
            }

            // `ON ERROR GOTO n` arms the handler by number; `GOTO 0` disarms
            // it. Both clear the in-handler flag, so re-arming is also how a
            // program that left its handler by GOTO starts trapping again.
            StmtKind::OnError(target) => {
                let number = match target {
                    Some(t) => {
                        let label = storage::main_label(t);
                        self.cg
                            .err_handlers
                            .iter()
                            .position(|h| *h == label)
                            .expect("preprocess collected every handler")
                            + 1
                    }
                    None => 0,
                };
                // In a handler that is handling an error, GOTO 0 stops the
                // program with that error, as GW-BASIC does: the handler is
                // saying it cannot deal with this one. Trapping is off by
                // then, so raising it again is fatal.
                let handling = (number == 0 && self.kind == FnKind::Handler)
                    .then(|| self.load_rt_word("_err_active"));
                self.store_rt_word("_err_handler", number as i64);
                self.store_rt_word("_err_active", 0);
                if let Some(active) = handling {
                    let reraise = self.b.create_block();
                    let done = self.b.create_block();
                    self.brif(active, reraise, done);
                    self.switch_sealed(reraise);
                    let code = self.call_rt("_rt_err", &[])[0];
                    let line = self.call_rt("_rt_erl", &[])[0];
                    self.call_rt("_rt_error_num", &[code, line]);
                    self.b.ins().jump(done, &[]);
                    self.switch_sealed(done);
                }
            }

            // `ERROR n`: raise the error GW-BASIC numbers n, 1 to 255.
            StmtKind::RaiseError(expr) => {
                let n = self.gen_i64(expr);
                self.check_range(n, 1, Some(255), RtError::Domain);
                let line = self.iconst(types::I64, i64::from(self.report_line()));
                self.call_rt("_rt_error_num", &[n, line]);
            }
            _ => unreachable!("not a trapping statement"),
        }
    }
}
