//! String ownership: who frees a string's bytes, and when
//!
//! A string value is a (pointer, length) pair, and the pointer may be a
//! fresh allocation, a slice of another string (LEFT$, MID$), a static
//! buffer (LINE INPUT), a literal, or a window into a FIELD record buffer.
//! Only a fresh allocation may be freed, so ownership is tracked explicitly:
//!
//! - In a slot -- a variable, an array element, a record field -- bit 63 of
//!   the length word says whether the slot owns its bytes (`OWNED`).
//!   Assigning to it stores an owned copy and frees the old value if it was
//!   owned; a FIELD variable's bytes are the buffer's, so it owns nothing.
//! - In a statement, the fresh results of the helpers rtsig marks `fresh`,
//!   and of user FUNCTIONs, are temporaries. A store takes one over rather
//!   than copying it; whatever is left is freed when the statement is done,
//!   or -- in a compound statement -- before control branches into a body.
//! - ERASE, REDIM and a procedure's exit free what they discard, and a
//!   whole-record copy duplicates its string fields, so no two slots ever
//!   own the same bytes.
//!
//! A loaded string is borrowed, and stays valid until something reassigns
//! its slot. Within one statement only a user FUNCTION can do that while the
//! value is still in use, so in a statement that calls one, string loads are
//! copies.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use super::*;
use crate::lexer::normalized;
use crate::sema::for_each_expr_mut;

impl FnGen<'_> {
    /// `v` as a string this code owns: a temporary taken over, or a copy.
    pub(super) fn owned_str(&mut self, v: Val) -> (Value, Value) {
        let (p, l) = v.str();
        if let Some(i) = self.temps.iter().position(|t| *t == p) {
            self.temps.swap_remove(i);
            return (p, l);
        }
        let r = self.call_rt("_rt_strdup", &[p, l]);
        let taken = self.temps.pop();
        debug_assert_eq!(taken, Some(r[0]), "the copy is the newest temporary");
        (r[0], r[1])
    }

    /// Assign `v` to the string slot at `loc`: the slot owns a copy (or the
    /// temporary itself), and its old value is freed if the slot owned it.
    pub(super) fn assign_str(&mut self, loc: Loc, v: Val) {
        let (p, l) = self.owned_str(v);
        let (old_p, old_l) = self.load_str_raw(loc);
        let owned = self.b.ins().bor_imm_s(l, OWNED);
        self.store_str_raw((p, owned), loc);
        self.free_owned(old_p, old_l);
    }

    /// Point the string slot at `loc` at bytes it does not own -- a FIELD
    /// window -- freeing its old value if it owned that.
    pub(super) fn bind_str(&mut self, loc: Loc, p: Value, l: Value) {
        let (old_p, old_l) = self.load_str_raw(loc);
        self.store_str_raw((p, l), loc);
        self.free_owned(old_p, old_l);
    }

    /// Free the string at `loc` if the slot owns it, and empty the slot.
    pub(super) fn free_slot_str(&mut self, loc: Loc) {
        let (p, raw) = self.load_str_raw(loc);
        let z = self.iconst(types::I64, 0);
        self.store_str_raw((z, z), loc);
        self.free_owned(p, raw);
    }

    /// Free `p` when the raw length word `raw` says its slot owned it.
    fn free_owned(&mut self, p: Value, raw: Value) {
        let owned = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, raw, 0);
        let free = self.b.create_block();
        let done = self.b.create_block();
        let from = self.b.current_block();
        self.brif(owned, free, done);
        self.switch_sealed(free);
        self.call("free", &[p]);
        self.begin_sealed(done);
        self.carry_err_line(from, done);
    }

    /// Free this statement's temporaries.
    pub(super) fn free_temps(&mut self) {
        for p in std::mem::take(&mut self.temps) {
            self.call("free", &[p]);
        }
    }

    /// A string just loaded from a slot, copied when this statement calls a
    /// user FUNCTION that could free the slot's bytes before it is used.
    pub(super) fn guard_load(&mut self, v: Val) -> Val {
        match v {
            Val::Str(p, l) if self.copy_loads => {
                let r = self.call_rt("_rt_strdup", &[p, l]);
                Val::Str(r[0], r[1])
            }
            v => v,
        }
    }

    /// Byte offsets of the string fields in a record of type `ty`, nested
    /// records included.
    pub(super) fn string_offsets(&self, ty: &TypeRef) -> Vec<i32> {
        let mut out = Vec::new();
        self.collect_string_offsets(ty, 0, &mut out);
        out
    }

    fn collect_string_offsets(&self, ty: &TypeRef, base: i32, out: &mut Vec<i32>) {
        match ty {
            TypeRef::FixedString(_) => out.push(base),
            TypeRef::Record(rec) => {
                if let Some(info) = self.cg.symbols.records.get(normalized(rec)) {
                    for (_, f) in &info.fields {
                        self.collect_string_offsets(&f.ty, base + 8 * f.word, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// Where the strings are in an element of the array `name`: its byte
    /// size and the strings' offsets in it. None when it holds no strings.
    pub(super) fn array_string_layout(&self, name: &str) -> Option<(i64, Vec<i32>)> {
        let es = i64::from(self.elem_size_for(name));
        let offsets = match self.array_elem_type(name) {
            Some(ty) => self.string_offsets(&ty),
            None if DataType::from_type_ref(&self.array_value_type(name)) == DataType::String => {
                vec![0]
            }
            None => Vec::new(),
        };
        (!offsets.is_empty()).then_some((es, offsets))
    }

    /// Free the owned strings of elements `from..to` of an element block.
    pub(super) fn free_element_strings(
        &mut self,
        base: Value,
        from: Value,
        to: Value,
        es: i64,
        offsets: &[i32],
    ) {
        let i = self.b.declare_var(types::I64);
        self.b.def_var(i, from);
        let head = self.b.create_block();
        let body = self.b.create_block();
        let done = self.b.create_block();
        self.begin_block(head);
        let iv = self.b.use_var(i);
        let more = self.b.ins().icmp(IntCC::UnsignedLessThan, iv, to);
        self.brif(more, body, done);
        self.switch_sealed(body);
        let off = self.b.ins().imul_imm_s(iv, es);
        let elem = self.b.ins().iadd(base, off);
        for o in offsets {
            self.free_slot_str(Loc::Ptr(elem, *o));
        }
        let next = self.b.ins().iadd_imm_s(iv, 1);
        self.b.def_var(i, next);
        self.b.ins().jump(head, &[]);
        self.b.seal_block(head);
        self.switch_sealed(done);
    }

    /// Free the owned strings of a record at `loc`.
    pub(super) fn free_record_strings(&mut self, loc: Loc, ty: &TypeRef) {
        for o in self.string_offsets(ty) {
            self.free_slot_str(loc.offset(o));
        }
    }

    /// Copy the record at `src` over the one at `dst`, both of type `ty`.
    ///
    /// The words are copied, then each string field is given its own copy,
    /// so that the two records never own the same bytes. The copies are made
    /// before anything at `dst` is freed: `dst` and `src` may be the same.
    pub(super) fn copy_record(&mut self, dst: Loc, src: Value, ty: &TypeRef) {
        let words = self.cg.symbols.type_words(ty);
        let offsets = self.string_offsets(ty);
        let mut dups = Vec::with_capacity(offsets.len());
        for o in &offsets {
            let (p, l) = self.load_str(Loc::Ptr(src, *o));
            let r = self.call_rt("_rt_strdup", &[p, l]);
            self.temps.pop();
            dups.push((r[0], r[1]));
        }
        self.free_record_strings(dst, ty);
        for w in 0..words {
            if offsets.iter().any(|o| *o == 8 * w || *o + 8 == 8 * w) {
                continue;
            }
            let v = self
                .b
                .ins()
                .load(types::I64, MemFlagsData::trusted(), src, 8 * w);
            self.store(v, dst.offset(8 * w));
        }
        for (o, (p, l)) in offsets.into_iter().zip(dups) {
            let owned = self.b.ins().bor_imm_s(l, OWNED);
            self.store_str_raw((p, owned), dst.offset(o));
        }
    }

    /// Whether `stmt`'s own expressions call a user FUNCTION.
    pub(super) fn calls_user_function(&self, stmt: &Stmt) -> bool {
        let mut copy = ssa::shallow(stmt);
        let mut exprs = Vec::new();
        for_each_expr_mut(&mut copy, &mut |e: &mut Expr| exprs.push(e.clone()));
        let procs = &self.cg.symbols.procs;
        let is_fn = |n: &str| procs.get(&n.to_uppercase()).is_some_and(|p| p.is_function);
        while let Some(e) = exprs.pop() {
            match e {
                Expr::Variable(n) => {
                    if is_fn(&n) && self.current_proc.as_deref() != Some(n.to_uppercase().as_str())
                    {
                        return true;
                    }
                }
                Expr::FnCall { name, args }
                | Expr::ArrayAccess {
                    name,
                    indices: args,
                } => {
                    if is_fn(&name) {
                        return true;
                    }
                    exprs.extend(args);
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
        false
    }
}
