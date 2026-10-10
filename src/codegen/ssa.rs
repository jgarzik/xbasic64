//! Which scalars live in SSA values rather than in memory
//!
//! A numeric scalar is held in a Cranelift `Variable` -- so in a register,
//! with no load or store per use -- when nothing outside the function can
//! reach its storage:
//!
//! - not passed to a procedure, whose by-reference parameter would need its
//!   address;
//! - in main, not shared with any procedure or DEF FN, and not in a program
//!   with ON ERROR, whose handler is a function of its own that reads and
//!   writes main's variables in memory;
//! - in a procedure, not shared and not STATIC, which both live in memory.
//!
//! A `Variable` needs no dominance discipline of the code generating it:
//! GOTO into a loop, or RESUME re-entering a statement, simply gives the
//! variable a block parameter there. One never assigned on some path reads
//! as zero, which is BASIC's unassigned value.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use super::*;
use crate::sema::{for_each_expr_mut, walk_stmts};
use std::collections::HashSet;

/// A copy of `stmt` without its nested bodies, which `walk_stmts` visits on
/// their own: copying them at every level would be quadratic in nesting.
pub(super) fn shallow(stmt: &Stmt) -> Stmt {
    let kind = match &stmt.kind {
        StmtKind::If { condition, .. } => StmtKind::If {
            condition: condition.clone(),
            then_branch: Vec::new(),
            else_branch: None,
        },
        StmtKind::For {
            var,
            start,
            end,
            step,
            ..
        } => StmtKind::For {
            var: var.clone(),
            start: start.clone(),
            end: end.clone(),
            step: step.clone(),
            body: Vec::new(),
        },
        StmtKind::While { condition, .. } => StmtKind::While {
            condition: condition.clone(),
            body: Vec::new(),
        },
        StmtKind::DoLoop {
            condition,
            cond_at_start,
            is_until,
            ..
        } => StmtKind::DoLoop {
            condition: condition.clone(),
            cond_at_start: *cond_at_start,
            is_until: *is_until,
            body: Vec::new(),
        },
        StmtKind::SelectCase { expr, cases } => StmtKind::SelectCase {
            expr: expr.clone(),
            cases: cases.iter().map(|(c, _)| (c.clone(), Vec::new())).collect(),
        },
        StmtKind::Sub { .. } | StmtKind::Function { .. } => StmtKind::End,
        k => k.clone(),
    };
    Stmt {
        line: stmt.line,
        kind,
    }
}

/// Upper-case names in `body` that are passed directly as arguments to a
/// user procedure, and so may be shared with a by-reference parameter. One
/// in parentheses, `(A)`, is a copy, and is not.
pub(super) fn passed_to_procedures(body: &[Stmt], symbols: &Symbols) -> HashSet<String> {
    let mut taken = HashSet::new();
    let procs = &symbols.procs;
    let visit_args = |args: &[Expr], taken: &mut HashSet<String>| {
        for a in args {
            if let Expr::Variable(n) = a {
                taken.insert(n.to_uppercase());
            }
        }
    };
    let mut exprs: Vec<Expr> = Vec::new();
    walk_stmts(body, &mut |stmt: &Stmt| {
        if matches!(stmt.kind, StmtKind::Sub { .. } | StmtKind::Function { .. }) {
            return;
        }
        if let StmtKind::Call { args, .. } = &stmt.kind {
            visit_args(args, &mut taken);
        }
        let mut copy = shallow(stmt);
        for_each_expr_mut(&mut copy, &mut |e: &mut Expr| exprs.push(e.clone()));
    });
    while let Some(e) = exprs.pop() {
        match e {
            Expr::FnCall { name, args }
            | Expr::ArrayAccess {
                name,
                indices: args,
            } => {
                if procs.contains_key(&name.to_uppercase()) {
                    visit_args(&args, &mut taken);
                }
                exprs.extend(args);
            }
            Expr::Unary { operand, .. } | Expr::Paren(operand) => exprs.push(*operand),
            Expr::Binary { left, right, .. } => {
                exprs.push(*left);
                exprs.push(*right);
            }
            Expr::Field { base, .. } => exprs.push(*base),
            Expr::Literal(_) | Expr::Variable(_) => {}
        }
    }
    taken
}

/// Upper-case names any expression or target in `body` mentions.
fn names_in(body: &[Stmt]) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut exprs: Vec<Expr> = Vec::new();
    walk_stmts(body, &mut |stmt: &Stmt| {
        let mut copy = shallow(stmt);
        for_each_expr_mut(&mut copy, &mut |e: &mut Expr| exprs.push(e.clone()));
        match &stmt.kind {
            StmtKind::Let { name, .. } | StmtKind::For { var: name, .. } => {
                names.insert(name.to_uppercase());
            }
            StmtKind::Input { vars, .. } | StmtKind::Read(vars) => {
                names.extend(vars.iter().map(|v| v.name.to_uppercase()));
            }
            StmtKind::LineInput { var, .. } => {
                names.insert(var.name.to_uppercase());
            }
            StmtKind::Swap(a, b) => {
                names.insert(a.name.to_uppercase());
                names.insert(b.name.to_uppercase());
            }
            _ => {}
        }
    });
    while let Some(e) = exprs.pop() {
        match e {
            Expr::Variable(n) => {
                names.insert(n.to_uppercase());
            }
            Expr::FnCall { args, .. } | Expr::ArrayAccess { indices: args, .. } => {
                exprs.extend(args)
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
    names
}

/// Module-level names that must stay in memory: shared with a procedure,
/// seen by a DEF FN, or passed to a procedure from main.
pub(super) fn module_in_memory(program: &Program, symbols: &Symbols) -> HashSet<String> {
    let main: Vec<Stmt> = program
        .statements
        .iter()
        .filter(|s| !matches!(s.kind, StmtKind::Sub { .. } | StmtKind::Function { .. }))
        .cloned()
        .collect();
    let mut keep = passed_to_procedures(&main, symbols);
    keep.extend(symbols.shared_all.vars.iter().cloned());
    for sets in symbols.proc_shared.values() {
        keep.extend(sets.vars.iter().cloned());
    }
    // A DEF FN sees every module-level name, so whatever its body mentions.
    for stmt in &program.statements {
        if let StmtKind::Function {
            body, def_fn: true, ..
        } = &stmt.kind
        {
            keep.extend(names_in(body));
        }
    }
    keep
}

impl FnGen<'_> {
    /// The SSA variable holding `name`, declaring it on first use, or None
    /// when the name lives in memory.
    pub(super) fn ssa_var(&mut self, name: &str) -> Option<(Variable, DataType)> {
        if let Some(v) = self.ssa.get(name) {
            return Some(*v);
        }
        if !self.ssa_enabled {
            return None;
        }
        let upper = name.to_uppercase();
        let ty = DataType::from_suffix(name);
        if ty == DataType::String
            || self.ssa_excluded.contains(&upper)
            || self.typed_var(name).is_some()
            || self.cg.symbols.consts.contains_key(&upper)
            || self.proc_vars.contains_key(name)
        {
            return None;
        }
        match self.current_proc.clone() {
            None => {
                if self.cg.module_in_memory.contains(&upper) {
                    return None;
                }
            }
            Some(p) => {
                if self.cg.symbols.is_static(&p, name, false) || self.module_visible(name, false) {
                    return None;
                }
            }
        }
        Some(self.declare_ssa(name, ty))
    }

    /// Hold `name`, of type `ty`, in a fresh SSA variable.
    pub(super) fn declare_ssa(&mut self, name: &str, ty: DataType) -> (Variable, DataType) {
        let v = self.b.declare_var(num_type(ty));
        self.ssa.insert(name.to_string(), (v, ty));
        (v, ty)
    }

    /// Assign an SSA variable, converting the value to its type.
    pub(super) fn def_ssa(&mut self, var: Variable, ty: DataType, v: Val) {
        let n = self.coerce(v, ty).num();
        // An unchecked INTEGER result may stray past 16 bits; a store would
        // keep only those, so the variable does too.
        let n = if ty == DataType::Integer && !self.cg.opts.checks {
            let h = self.b.ins().ireduce(types::I16, n);
            self.b.ins().sextend(types::I32, h)
        } else {
            n
        };
        self.b.def_var(var, n);
    }
}

/// Labels in `body` that something branches to: GOTO, GOSUB, ON ...,
/// RETURN line, RESUME line. Only these need a block of their own; any
/// other line number is just a place in straight-line code, and a block per
/// line would be one more the SSA builder has to visit.
pub(super) fn branch_targets(body: &[Stmt]) -> HashSet<String> {
    let mut targets = HashSet::new();
    walk_stmts(body, &mut |stmt: &Stmt| match &stmt.kind {
        StmtKind::Goto(t)
        | StmtKind::Gosub(t)
        | StmtKind::Return(Some(t))
        | StmtKind::Resume(ResumeTarget::At(t))
        | StmtKind::OnError(Some(t)) => {
            targets.insert(storage::main_label(t));
        }
        StmtKind::OnGoto { targets: ts, .. } | StmtKind::OnGosub { targets: ts, .. } => {
            targets.extend(ts.iter().map(storage::main_label));
        }
        _ => {}
    });
    targets
}
