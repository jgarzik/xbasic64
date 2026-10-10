//! User procedures: their prologues, epilogues and calls
//!
//! Arguments pass by reference unless BYVAL, as in QuickBASIC, and the
//! reference is real: a by-reference parameter is the caller's variable's
//! address, so a write through it is seen at once -- by the caller, by
//! another parameter aliasing the same variable, and by an error handler
//! that runs while the procedure is active. A fixed-length string given to
//! an ordinary string parameter is the exception: it goes as a copy that
//! comes back, at its width, when the procedure returns.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use super::*;
use crate::lexer::normalized;

/// How one argument travels.
pub(super) enum Pass {
    /// An address: an array's descriptor, a record, or anything by reference.
    Pointer,
    /// A string by value: its pointer and length.
    Str,
    /// A number by value, as a Double the procedure narrows to this type.
    Number(DataType),
}

/// A fixed-length string passed to a by-reference string parameter, to be
/// given the procedure's value back, at its own width, once it returns.
enum FixedTarget {
    /// A variable or a field of one: storage that stays where it is.
    Loc(Loc),
    /// An element of an array, or a field `offset` bytes into one, found
    /// again from its subscripts: the procedure may have moved the array.
    Elem {
        name: String,
        subs: Vec<Value>,
        offset: i32,
    },
}

/// How the parameter `p` is passed.
pub(super) fn param_pass(p: &Param) -> Pass {
    if p.array || !p.byval || matches!(p.ty, Some(TypeRef::Record(_))) {
        return Pass::Pointer;
    }
    match param_value_type(p) {
        DataType::String => Pass::Str,
        t => Pass::Number(t),
    }
}

/// The type a parameter holds inside the procedure: its `AS` clause or its
/// name's suffix.
pub(super) fn param_value_type(p: &Param) -> DataType {
    match &p.ty {
        Some(t) => DataType::from_type_ref(t),
        None => DataType::from_suffix(&p.name),
    }
}

/// Number of GOSUB return sites in `stmts`, not counting procedure bodies,
/// which are scopes of their own. Each GOSUB and each ON ... GOSUB makes one.
pub(super) fn count_return_sites(stmts: &[Stmt]) -> usize {
    stmts
        .iter()
        .map(|stmt| match &stmt.kind {
            StmtKind::Sub { .. } | StmtKind::Function { .. } => 0,
            StmtKind::Gosub(_) | StmtKind::OnGosub { .. } => 1,
            _ => child_bodies(stmt).into_iter().map(count_return_sites).sum(),
        })
        .sum()
}

impl FnGen<'_> {
    /// Start a scope's run of GOSUB return site numbers: a block for each,
    /// made now so that a RETURN ahead of a GOSUB can dispatch to it.
    pub(super) fn begin_gosub_scope(&mut self, body: &[Stmt]) {
        let count = count_return_sites(body);
        self.gosub_first = self.cg.gosub_sites;
        self.cg.gosub_sites += count;
        self.gosub_blocks = (0..count).map(|_| self.b.create_block()).collect();
        self.gosub_next = 0;
    }

    /// Lower a SUB or FUNCTION.
    pub(super) fn gen_procedure(&mut self, stmt: &Stmt) {
        let (name, params, body, is_function) = match &stmt.kind {
            StmtKind::Sub {
                name, params, body, ..
            } => (name, params, body, false),
            StmtKind::Function {
                name, params, body, ..
            } => (name, params, body, true),
            _ => unreachable!("a procedure is a SUB or FUNCTION"),
        };
        let upper = name.to_uppercase();
        self.current_proc = Some(upper.clone());
        self.current_line = stmt.line;
        self.ssa_enabled = true;
        self.ssa_excluded = ssa::passed_to_procedures(body, &self.cg.symbols);
        self.branch_targets = ssa::branch_targets(body);
        let exit = self.b.create_block();
        self.exit_block = Some(exit);

        // The entry block jumps to the zeroing of the frame, which is filled
        // in last, once every slot is known; it then comes back here.
        let entry = self.b.current_block().expect("the entry block");
        let args = self.b.block_params(entry).to_vec();
        let zinit = self.b.create_block();
        let start = self.b.create_block();
        self.b.ins().jump(zinit, &[]);
        self.b.switch_to_block(start);

        let mut args = args.into_iter();
        for p in params {
            let pname = p.name.clone();
            match param_pass(p) {
                Pass::Pointer => {
                    let ptr = args.next().expect("an address argument");
                    if p.array {
                        // The caller's descriptor itself: a REDIM or ERASE
                        // in here reaches the caller.
                        let up = pname.to_uppercase();
                        if let Some(ty @ TypeRef::Record(_)) = &p.ty {
                            let words = self.cg.symbols.type_words(ty);
                            self.proc_record_elem_words.insert(up.clone(), words);
                        }
                        let ndims = self
                            .cg
                            .symbols
                            .lookup_array(&self.sema_scope(), &up)
                            .expect("sema registers an array parameter")
                            .rank;
                        self.proc_arrays.insert(
                            up,
                            ArrayInfo {
                                loc: Loc::Ptr(ptr, 0),
                                ndims,
                            },
                        );
                    } else if let Some(ty @ TypeRef::Record(_)) = &p.ty {
                        let loc = if p.byval {
                            // A copy of the caller's record, strings and all.
                            let words = self.cg.symbols.type_words(ty);
                            let slot = self.local_slot(8 * words as u32);
                            self.copy_record(Loc::Stack(slot, 0), ptr, ty);
                            Loc::Stack(slot, 0)
                        } else {
                            Loc::Ptr(ptr, 0)
                        };
                        self.proc_record_vars.insert(pname.clone(), loc);
                        self.proc_types.insert(pname, ty.clone());
                    } else {
                        self.proc_vars.insert(
                            pname,
                            VarInfo {
                                loc: Loc::Ptr(ptr, 0),
                                data_type: param_value_type(p),
                            },
                        );
                    }
                }
                Pass::Str => {
                    let ptr = args.next().expect("a string pointer");
                    let len = args.next().expect("a string length");
                    // A copy of its own, which the procedure may assign and
                    // frees at exit; the caller's string is left alone.
                    let slot = self.local_slot(16);
                    let loc = Loc::Stack(slot, 0);
                    self.assign_str(loc, Val::Str(ptr, len));
                    self.proc_vars.insert(
                        pname,
                        VarInfo {
                            loc,
                            data_type: DataType::String,
                        },
                    );
                }
                Pass::Number(t) => {
                    let x = args.next().expect("a number");
                    // The ordinary conversion: an argument rounds and range
                    // checks exactly as an assignment would.
                    let x = Val::Num(DataType::Double, x);
                    if !self.ssa_excluded.contains(&pname.to_uppercase()) {
                        let (var, t) = self.declare_ssa(&pname, t);
                        self.def_ssa(var, t, x);
                        continue;
                    }
                    let v = self.coerce(x, t);
                    let slot = self.local_slot(8);
                    let loc = Loc::Stack(slot, 0);
                    self.store_num(t, v.num(), loc);
                    self.proc_vars.insert(pname, VarInfo { loc, data_type: t });
                }
            }
        }

        self.reserve_array_descriptors();

        // A FUNCTION's result: a variable of its own name.
        let ret_type = is_function.then(|| self.cg.proc_return_type(&upper));
        if let Some(t) = ret_type
            && t != DataType::String
            && !self.ssa_excluded.contains(&upper)
        {
            self.declare_ssa(name, t);
        } else if let Some(t) = ret_type {
            let slot = self.local_slot(8 * storage::words_for(t) as u32);
            self.proc_vars.insert(
                name.to_string(),
                VarInfo {
                    loc: Loc::Stack(slot, 0),
                    data_type: t,
                },
            );
        }

        // A RESUME <label> leaves this procedure through its epilogue, with
        // any GOSUB it made still on the GOSUB stack; the stack pointer goes
        // back to where it was on entry, so they do not outlive it.
        let gosub_mark = if self.cg.traps && self.cg.gosub_used {
            let slot = self.stack_slot(8);
            let sp = self.global_addr("_gosub_sp", 8);
            let v = self
                .b
                .ins()
                .load(types::I64, MemFlagsData::trusted(), sp, 0);
            self.b.ins().stack_store(types::I64, v, slot, 0);
            Some(slot)
        } else {
            None
        };

        self.begin_gosub_scope(body);
        for s in body {
            self.gen_stmt(s);
        }
        self.begin_block(exit);
        self.free_locals(name);
        if let Some(slot) = gosub_mark {
            let v = self.b.ins().stack_load(types::I64, types::I64, slot, 0);
            let sp = self.global_addr("_gosub_sp", 8);
            self.b.ins().store(MemFlagsData::trusted(), v, sp, 0);
        }
        match ret_type {
            None => {
                self.b.ins().return_(&[]);
            }
            Some(t) => {
                let val = match self.ssa.get(name.as_str()).copied() {
                    Some((var, t)) => Val::Num(t, self.b.use_var(var)),
                    None => {
                        let info = self.proc_vars[name];
                        self.load_val(t, info.loc)
                    }
                };
                let rets = match val {
                    Val::Str(p, l) => vec![p, l],
                    Val::Num(_, v) => vec![v],
                };
                self.b.ins().return_(&rets);
            }
        }

        // Zero the frame, then go to the parameters' stores.
        self.b.switch_to_block(zinit);
        let slots = std::mem::take(&mut self.zeroed_slots);
        let zero = self.iconst(types::I64, 0);
        for (slot, bytes) in &slots {
            if *bytes <= 128 {
                for w in 0..(*bytes as i32 / 8) {
                    self.b.ins().stack_store(types::I64, zero, *slot, 8 * w);
                }
            } else {
                let a = self.b.ins().stack_addr(types::I64, *slot, 0);
                let z = self.iconst(types::I32, 0);
                let n = self.iconst(types::I64, i64::from(*bytes));
                self.call("memset", &[a, z, n]);
            }
        }
        self.b.ins().jump(start, &[]);
        self.after_terminator();
    }

    /// Free what this procedure's own storage holds, at its exit: string
    /// locals and BYVAL strings, the strings of local records, and local
    /// arrays with the strings in them. Not a by-reference parameter, which
    /// is the caller's; not a STATIC, which outlives the call; and not the
    /// FUNCTION's result, which is returned.
    fn free_locals(&mut self, own_name: &str) {
        let mut vars: Vec<(String, VarInfo)> = self
            .proc_vars
            .iter()
            .map(|(n, i)| (n.clone(), *i))
            .collect();
        vars.sort_by(|a, b| a.0.cmp(&b.0));
        for (n, info) in vars {
            if n != own_name
                && info.data_type == DataType::String
                && matches!(info.loc, Loc::Stack(..))
            {
                self.free_slot_str(info.loc);
            }
        }
        let mut records: Vec<(String, Loc)> = self
            .proc_record_vars
            .iter()
            .map(|(n, l)| (n.clone(), *l))
            .collect();
        records.sort_by(|a, b| a.0.cmp(&b.0));
        for (n, loc) in records {
            if matches!(loc, Loc::Stack(..))
                && let Some(ty) = self.typed_var(&n)
            {
                self.free_record_strings(loc, &ty);
            }
        }
        let mut arrays: Vec<(String, ArrayInfo)> = self
            .proc_arrays
            .iter()
            .map(|(n, i)| (n.clone(), *i))
            .collect();
        arrays.sort_by(|a, b| a.0.cmp(&b.0));
        for (n, info) in arrays {
            if matches!(info.loc, Loc::Stack(..)) {
                self.free_array_storage(&n, info.loc, info.ndims);
            }
        }
    }

    /// Call a user SUB or FUNCTION; a FUNCTION's result.
    pub(super) fn gen_call(&mut self, name: &str, args: &[Expr]) -> Option<Val> {
        let upper = name.to_uppercase();
        let params: Vec<Param> = self
            .cg
            .symbols
            .procs
            .get(&upper)
            .map(|p| p.params.clone())
            .unwrap_or_default();
        let is_function = self
            .cg
            .symbols
            .procs
            .get(&upper)
            .is_some_and(|p| p.is_function);

        let mut vals = Vec::new();
        // By-reference temporaries holding strings, freed after the call.
        let mut string_temps = Vec::new();
        // Arrays an element of which is passed by reference.
        let mut locked = Vec::new();
        // Fixed-length strings passed as such temporaries, and their widths.
        let mut copy_backs = Vec::new();
        for (arg, p) in args.iter().zip(&params) {
            match param_pass(p) {
                Pass::Pointer if p.array => {
                    let arr = match arg {
                        Expr::Variable(n) => n.to_uppercase(),
                        Expr::ArrayAccess { name, .. } | Expr::FnCall { name, .. } => {
                            name.to_uppercase()
                        }
                        _ => unreachable!("sema requires an array argument"),
                    };
                    let info = self
                        .lookup_array(&arr)
                        .expect("sema checked the array exists");
                    vals.push(self.addr(info.loc));
                }
                Pass::Pointer if matches!(p.ty, Some(TypeRef::Record(_))) => {
                    let a = self
                        .record_addr(arg)
                        .expect("sema checked a record argument");
                    vals.push(a);
                    locked.extend(self.indexed_array(arg));
                }
                Pass::Pointer => {
                    // A variable, an array element or a field of the
                    // parameter's own type is passed as its address, so the
                    // procedure's changes reach it; anything else is
                    // evaluated into a temporary, whose address is passed.
                    let ty = param_value_type(p);
                    // A fixed-length string goes as a copy of ordinary
                    // length, which comes back padded or cut to its width,
                    // as QuickBASIC does: the procedure cannot change the
                    // width, nor leave it holding a string it does not own.
                    let fixed = match ty {
                        DataType::String => self.fixed_string_arg(arg),
                        _ => None,
                    };
                    if let Some((target, width, now)) = fixed {
                        let slot = self.stack_slot(16);
                        let loc = Loc::Stack(slot, 0);
                        let z = self.iconst(types::I64, 0);
                        self.store_str_raw((z, z), loc);
                        let v = self.load_typed(now, &TypeRef::FixedString(width));
                        self.assign_str(loc, v);
                        string_temps.push(loc);
                        copy_backs.push((loc, target, width));
                        vals.push(self.addr(loc));
                        continue;
                    }
                    match self.lvalue_addr(arg, ty) {
                        Some(a) => {
                            vals.push(a);
                            locked.extend(self.indexed_array(arg));
                        }
                        None => {
                            let v = self.gen_expr(arg);
                            let slot = self.stack_slot(8 * storage::words_for(ty) as u32);
                            let loc = Loc::Stack(slot, 0);
                            match v {
                                Val::Str(..) => {
                                    // The procedure may assign it, freeing
                                    // what it held, so it owns a copy.
                                    let z = self.iconst(types::I64, 0);
                                    self.store_str_raw((z, z), loc);
                                    self.assign_str(loc, v);
                                    string_temps.push(loc);
                                }
                                v => {
                                    let n = self.coerce(v, ty).num();
                                    self.store_num(ty, n, loc);
                                }
                            }
                            vals.push(self.addr(loc));
                        }
                    }
                }
                Pass::Str => {
                    let (p, l) = self.gen_expr(arg).str();
                    vals.push(p);
                    vals.push(l);
                }
                Pass::Number(_) => {
                    let x = self.gen_double(arg);
                    vals.push(x);
                }
            }
        }

        let sym = format!("_proc_{}", mangle(&upper));
        let id = self.cg.funcs[&sym];
        let fref = self.func_ref(id);
        // The arrays are locked while the call runs, so that ERASE or
        // REDIM cannot free an element under its parameter. Every argument
        // is evaluated first, and the lock comes off before an error the
        // call left pending is looked at, so no path leaves it on.
        if !self.cg.opts.checks {
            locked.clear();
        }
        for info in &locked {
            self.add_lock(*info, 1);
        }
        let inst = self.b.ins().call(fref, &vals);
        let rets = self.b.inst_results(inst).to_vec();
        for info in &locked {
            self.add_lock(*info, -1);
        }
        // The procedure's statements stored lines of their own.
        self.err_line_block = None;
        // An error left pending goes to the fault block before any copy
        // back, so the target keeps its value; the temporary is not freed
        // on that path, which leaks it but frees nothing twice.
        self.error_check();
        for (loc, target, width) in copy_backs {
            let dst = match target {
                FixedTarget::Loc(l) => l,
                FixedTarget::Elem { name, subs, offset } => {
                    let info = self
                        .lookup_array(&name)
                        .expect("sema checked the array is declared");
                    self.autodim_if_null(&name, info);
                    Loc::Ptr(self.elem_addr(&name, &subs), offset)
                }
            };
            let (p, l) = self.load_str(loc);
            self.store_typed(dst, &TypeRef::FixedString(width), Val::Str(p, l));
        }
        for loc in string_temps {
            self.free_slot_str(loc);
        }
        if !is_function {
            return None;
        }
        let t = self.cg.proc_return_type(&upper);
        Some(match t {
            // The FUNCTION's result variable is gone; its string is this
            // statement's to free.
            DataType::String => {
                self.temps.push(rets[0]);
                Val::Str(rets[0], rets[1])
            }
            t => Val::Num(t, rets[0]),
        })
    }

    /// The array `arg` is an element of, or a field of an element of.
    fn indexed_array(&self, arg: &Expr) -> Option<ArrayInfo> {
        let name = match arg {
            Expr::ArrayAccess { name, indices }
            | Expr::FnCall {
                name,
                args: indices,
            } if !indices.is_empty() => name.clone(),
            Expr::Field { .. } => Self::flatten_indexed_field_path(arg)?.0,
            _ => return None,
        };
        self.lookup_array(&name.to_uppercase())
    }

    /// Add `n` to the lock count of an array's descriptor.
    fn add_lock(&mut self, info: ArrayInfo, n: i64) {
        let locks = self.load(types::I64, info.lock());
        let v = self.b.ins().iadd_imm_s(locks, n);
        self.store(v, info.lock());
    }

    /// Where `arg` is when it is a fixed-length string -- a variable, an
    /// array element or a record field -- with its width and its storage now.
    fn fixed_string_arg(&mut self, arg: &Expr) -> Option<(FixedTarget, usize, Loc)> {
        match arg {
            Expr::Variable(name) => {
                if self.cg.symbols.procs.contains_key(&name.to_uppercase()) {
                    return None;
                }
                match self.typed_storage(name)? {
                    (loc, TypeRef::FixedString(w)) if w > 0 => {
                        Some((FixedTarget::Loc(loc), w, loc))
                    }
                    _ => None,
                }
            }
            Expr::ArrayAccess { name, indices }
            | Expr::FnCall {
                name,
                args: indices,
            } if !indices.is_empty()
                && self.lookup_array(&name.to_uppercase()).is_some()
                && !self.cg.symbols.procs.contains_key(&name.to_uppercase())
                && self.array_elem_type(name).is_none() =>
            {
                let TypeRef::FixedString(w @ 1..) = self.array_value_type(name) else {
                    return None;
                };
                let (a, subs) = self.array_addr_and_subs(name, indices);
                let name = name.clone();
                let target = FixedTarget::Elem {
                    name,
                    subs,
                    offset: 0,
                };
                Some((target, w, Loc::Ptr(a, 0)))
            }
            Expr::Field { .. } => {
                if let Some((name, indices, fields)) = Self::flatten_indexed_field_path(arg) {
                    let base = self.array_elem_type(&name)?;
                    let (offset, ty) = self.field_byte_offset(&base, &fields)?;
                    let TypeRef::FixedString(w @ 1..) = ty else {
                        return None;
                    };
                    let (a, subs) = self.array_addr_and_subs(&name, &indices);
                    let target = FixedTarget::Elem { name, subs, offset };
                    return Some((target, w, Loc::Ptr(a, offset)));
                }
                let (name, fields) = Self::flatten_field_path(arg)?;
                match self.resolve_field_path(&name, &fields)? {
                    (loc, TypeRef::FixedString(w)) if w > 0 => {
                        Some((FixedTarget::Loc(loc), w, loc))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The address of `arg` when it is a variable, an array element or a
    /// record field of type `ty` -- something a by-reference parameter can
    /// share.
    fn lvalue_addr(&mut self, arg: &Expr, ty: DataType) -> Option<Value> {
        match arg {
            Expr::Variable(name) => {
                if self.cg.symbols.consts.contains_key(normalized(name))
                    || crate::sema::is_zero_arg_builtin(name)
                    || self.cg.symbols.procs.contains_key(&name.to_uppercase())
                    || self.expr_type(arg) != ty
                {
                    return None;
                }
                let loc = match self.typed_storage(name) {
                    Some((_, TypeRef::Record(_))) => return None,
                    Some((loc, _)) => loc,
                    None => self.get_var_info(name).loc,
                };
                Some(self.addr(loc))
            }
            Expr::ArrayAccess { name, indices }
            | Expr::FnCall {
                name,
                args: indices,
            } if !indices.is_empty()
                && self.lookup_array(&name.to_uppercase()).is_some()
                && !self.cg.symbols.procs.contains_key(&name.to_uppercase())
                && !matches!(
                    self.array_value_type(name),
                    TypeRef::Record(_) | TypeRef::FixedString(1..)
                )
                && self.array_elem_type(name).is_none()
                && DataType::from_type_ref(&self.array_value_type(name)) == ty =>
            {
                let indices = indices.clone();
                Some(self.array_addr(name, &indices))
            }
            Expr::Field { .. } => {
                let (name, fields) = Self::flatten_field_path(arg)?;
                let (loc, fty) = self.resolve_field_path(&name, &fields)?;
                if matches!(fty, TypeRef::Record(_) | TypeRef::FixedString(_))
                    || DataType::from_type_ref(&fty) != ty
                {
                    return None;
                }
                Some(self.addr(loc))
            }
            _ => None,
        }
    }

    /// The address of a record lvalue: a record variable, an element of an
    /// array of records, or a nested record field.
    pub(super) fn record_addr(&mut self, expr: &Expr) -> Option<Value> {
        match expr {
            Expr::Variable(name) => {
                let ty = self.typed_var(name)?;
                if !matches!(ty, TypeRef::Record(_)) {
                    return None;
                }
                let loc = self.get_record_loc(name, &ty);
                Some(self.addr(loc))
            }
            Expr::ArrayAccess { name, indices }
            | Expr::FnCall {
                name,
                args: indices,
            } => {
                if !matches!(self.array_elem_type(name), Some(TypeRef::Record(_))) {
                    return None;
                }
                let indices = indices.clone();
                Some(self.array_addr(name, &indices))
            }
            Expr::Field { .. } => {
                if let Some((name, indices, fields)) = Self::flatten_indexed_field_path(expr) {
                    let base = self.array_elem_type(&name)?;
                    let (offset, ty) = self.field_byte_offset(&base, &fields)?;
                    if !matches!(ty, TypeRef::Record(_)) {
                        return None;
                    }
                    let a = self.array_addr(&name, &indices);
                    return Some(self.b.ins().iadd_imm_s(a, i64::from(offset)));
                }
                let (name, fields) = Self::flatten_field_path(expr)?;
                let (loc, ty) = self.resolve_field_path(&name, &fields)?;
                if !matches!(ty, TypeRef::Record(_)) {
                    return None;
                }
                Some(self.addr(loc))
            }
            _ => None,
        }
    }
}
