//! Array descriptors read once per loop rather than once per element
//!
//! Every element access reads its array's descriptor: the pointer, to test
//! it for null and to add the offset to, and each dimension's count -- and
//! lower bound, if it has one -- to check and scale the subscript. Cranelift
//! hoists a load out of a loop only when it is marked `readonly`, which these
//! are not: DIM, REDIM and ERASE write descriptors in the same function.
//!
//! So a loop whose body cannot change a descriptor reads those words into
//! `Variable`s just before its head, and every access in it uses those. A
//! body is clean when it, and every statement nested in it, has none of:
//!
//! - DIM, REDIM, ERASE or OPTION BASE;
//! - a call to a SUB, FUNCTION or DEF FN, which may REDIM a shared array or
//!   one passed to it;
//! - GOSUB, RETURN or RESUME, which leave for code that may;
//! - a label something branches to, which enters the body other than
//!   through the head, past the reads.
//!
//! A program with ON ERROR caches nothing: its handler may REDIM, and RESUME
//! re-enters a statement mid-loop.
//!
//! The reads create nothing. An array with no storage yet is created by its
//! first access, as anywhere else, and that access reads the new descriptor
//! into the same `Variable`s, so later iterations see it. A loop that runs
//! no times has created no array, and a DIM after it is not a duplicate.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use super::*;
use crate::sema::{for_each_expr_mut, walk_stmts};

/// An array's descriptor words, as `Variable`s: the pointer, a count per
/// dimension, and a lower bound per dimension when it has them.
#[derive(Clone, Debug)]
pub(super) struct Hoisted {
    pub(super) ptr: Variable,
    pub(super) counts: Vec<Variable>,
    pub(super) lowers: Vec<Variable>,
}

impl FnGen<'_> {
    /// The arrays a loop whose condition is `cond` and body is `body`
    /// subscripts, upper case, or None when it may change a descriptor.
    fn clean_loop_arrays(&self, cond: Option<&Expr>, body: &[Stmt]) -> Option<Vec<String>> {
        if self.cg.traps {
            return None;
        }
        let mut clean = true;
        let mut names: Vec<String> = Vec::new();
        let mut exprs: Vec<Expr> = cond.into_iter().cloned().collect();
        walk_stmts(body, &mut |stmt: &Stmt| {
            let mut indexed = |lv: &LValue| {
                if lv.indices.is_some() {
                    names.push(lv.name.to_uppercase());
                }
            };
            match &stmt.kind {
                StmtKind::Dim { .. }
                | StmtKind::Redim { .. }
                | StmtKind::Erase(_)
                | StmtKind::OptionBase(_)
                | StmtKind::Call { .. }
                | StmtKind::Gosub(_)
                | StmtKind::OnGosub { .. }
                | StmtKind::Return(_)
                | StmtKind::Resume(_)
                | StmtKind::OnError(_) => clean = false,
                StmtKind::Label(n) => {
                    let label = storage::main_label(&GotoTarget::Line(*n));
                    clean &= !self.branch_targets.contains(&label);
                }
                StmtKind::LabelName(s) => {
                    let label = storage::main_label(&GotoTarget::Label(s.clone()));
                    clean &= !self.branch_targets.contains(&label);
                }
                StmtKind::Let {
                    name,
                    indices: Some(_),
                    ..
                } => names.push(name.to_uppercase()),
                StmtKind::Input { vars, .. } | StmtKind::Read(vars) => {
                    vars.iter().for_each(&mut indexed)
                }
                StmtKind::Swap(a, b) => {
                    indexed(a);
                    indexed(b);
                }
                StmtKind::LineInput { var: target, .. }
                | StmtKind::MidAssign { target, .. }
                | StmtKind::FieldAssign { target, .. }
                | StmtKind::SetField { target, .. } => indexed(target),
                _ => {}
            }
            let mut copy = ssa::shallow(stmt);
            for_each_expr_mut(&mut copy, &mut |e: &mut Expr| exprs.push(e.clone()));
        });

        // A FUNCTION's own name, inside it, is its result, not a call.
        let procs = &self.cg.symbols.procs;
        let calls = |n: &str| {
            let up = n.to_uppercase();
            procs.contains_key(&up) && self.current_proc.as_deref() != Some(up.as_str())
        };
        while let Some(e) = exprs.pop() {
            match e {
                Expr::Variable(n) => clean &= !calls(&n),
                Expr::FnCall { name, args } => {
                    clean &= !procs.contains_key(&name.to_uppercase());
                    exprs.extend(args);
                }
                Expr::ArrayAccess { name, indices } => {
                    if procs.contains_key(&name.to_uppercase()) {
                        clean = false;
                    } else {
                        names.push(name.to_uppercase());
                    }
                    exprs.extend(indices);
                }
                Expr::Unary { operand, .. } | Expr::Paren(operand) => exprs.push(*operand),
                Expr::Binary { left, right, .. } => {
                    exprs.push(*left);
                    exprs.push(*right);
                }
                Expr::Field { base, .. } => exprs.push(*base),
                Expr::Literal(_) => {}
            }
        }
        clean.then_some(names)
    }

    /// Before a loop's head: read the descriptor of every array the loop --
    /// its condition `cond` and its body -- subscripts into `Variable`s, if
    /// nothing in it can change one. Returns what to give `end_hoist` once
    /// the loop is compiled.
    pub(super) fn begin_hoist(
        &mut self,
        cond: Option<&Expr>,
        body: &[Stmt],
    ) -> HashMap<String, Hoisted> {
        let saved = self.hoisted.clone();
        let Some(names) = self.clean_loop_arrays(cond, body) else {
            return saved;
        };
        for name in names {
            // A loop inside one that read it already reads it again for
            // nothing: nothing between the two can have changed it.
            if self.hoisted.contains_key(&name) {
                continue;
            }
            let Some(info) = self.lookup_array(&name) else {
                continue;
            };
            let lowers = if self.array_has_lower(&name) {
                info.ndims
            } else {
                0
            };
            let mut declare = |n: usize| -> Vec<Variable> {
                (0..n).map(|_| self.b.declare_var(types::I64)).collect()
            };
            let ptr = declare(1)[0];
            let counts = declare(info.ndims);
            let lowers = declare(lowers);
            let h = Hoisted {
                ptr,
                counts,
                lowers,
            };
            self.read_descriptor(info, &h);
            self.hoisted.insert(name, h);
        }
        saved
    }

    /// After the loop: what was read before it is not valid after it.
    pub(super) fn end_hoist(&mut self, saved: HashMap<String, Hoisted>) {
        self.hoisted = saved;
    }

    /// Read the descriptor at `info` into `h`'s variables.
    fn read_descriptor(&mut self, info: ArrayInfo, h: &Hoisted) {
        let p = self.load(types::I64, info.loc);
        self.b.def_var(h.ptr, p);
        let n = info.ndims as i32;
        for (i, &v) in h.counts.iter().enumerate() {
            let c = self.load(types::I64, info.loc.offset(8 * (1 + i as i32)));
            self.b.def_var(v, c);
        }
        for (i, &v) in h.lowers.iter().enumerate() {
            let lo = self.load(types::I64, info.loc.offset(8 * (1 + n + i as i32)));
            self.b.def_var(v, lo);
        }
    }

    /// The descriptor words of `name` read before the loop being compiled,
    /// if they were.
    pub(super) fn hoisted(&self, name: &str) -> Option<Hoisted> {
        self.hoisted.get(&name.to_uppercase()).cloned()
    }

    /// After `_rt_array_autodim` has created `name`: read the new descriptor
    /// into the loop's variables, if they hold it.
    pub(super) fn reread_hoisted(&mut self, name: &str, info: ArrayInfo) {
        if let Some(h) = self.hoisted(name) {
            self.read_descriptor(info, &h);
        }
    }
}
