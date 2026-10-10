//! Where names live: variables, records, arrays, and assignment targets

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use super::*;
use crate::lexer::normalized;
use crate::sema::Scope as SemaScope;

/// Map a BASIC identifier to a symbol-safe name.
///
/// BASIC names keep their type suffixes (`A$`, `N%`), which are not valid
/// symbol characters. The mapping is injective because `_` escapes itself, so
/// distinct BASIC names cannot collide: `A_S` becomes `A__S` while `A$`
/// becomes `A_S`.
pub fn mangle(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    for c in name.chars() {
        match c {
            '_' => out.push_str("__"),
            '%' => out.push_str("_I"),
            '&' => out.push_str("_L"),
            '!' => out.push_str("_F"),
            '#' => out.push_str("_D"),
            '$' => out.push_str("_S"),
            '.' => out.push_str("_P"),
            c => out.push(c),
        }
    }
    out
}

/// The name a branch target has in main: a line number or a label.
pub(super) fn main_label(target: &GotoTarget) -> String {
    match target {
        GotoTarget::Line(n) => format!("_line_{}", n),
        GotoTarget::Label(s) => format!("_label_{}", mangle(&s.to_uppercase())),
    }
}

/// Number of 8-byte words a scalar of this type occupies.
pub(super) fn words_for(data_type: DataType) -> i32 {
    if data_type == DataType::String { 2 } else { 1 }
}

/// Size in bytes of one element of an array of `ty`, stored at its width.
pub(super) fn elem_size(ty: DataType) -> i32 {
    match ty {
        DataType::String => 16,
        DataType::Integer => 2,
        DataType::Long | DataType::Single => 4,
        DataType::Double => 8,
    }
}

impl FnGen<'_> {
    /// The scope semantic analysis would name for the code being generated.
    pub(super) fn sema_scope(&self) -> SemaScope {
        match &self.current_proc {
            Some(p) => SemaScope::Proc(p.clone()),
            None => SemaScope::Module,
        }
    }

    /// Whether the code being generated sees the module-level `name` (an array
    /// when `array`): always in main, and in a procedure only when shared.
    pub(super) fn module_visible(&self, name: &str, array: bool) -> bool {
        match &self.current_proc {
            None => true,
            Some(p) => self.cg.symbols.shares(p, name, array),
        }
    }

    /// A variable's storage, allocating it the first time it is seen.
    pub(super) fn get_var_info(&mut self, name: &str) -> VarInfo {
        let data_type = DataType::from_suffix(name);
        let words = words_for(data_type);

        if let Some(proc) = self.current_proc.clone() {
            if let Some(info) = self.proc_vars.get(name) {
                return *info;
            }
            // A STATIC local keeps its value between calls, so it lives in
            // static storage under the procedure's name.
            if self.cg.symbols.is_static(&proc, name, false) {
                let sym = format!("_static_{}_{}", mangle(&proc), mangle(name));
                let id = self.cg.global(&sym, 8 * words as u32);
                let info = VarInfo {
                    loc: Loc::Data(id, 0),
                    data_type,
                };
                self.proc_vars.insert(name.to_string(), info);
                return info;
            }
        }

        if self.module_visible(name, false) {
            if let Some(info) = self.cg.vars.get(name) {
                return *info;
            }
            let id = self
                .cg
                .global(&format!("_var_{}", mangle(name)), 8 * words as u32);
            let info = VarInfo {
                loc: Loc::Data(id, 0),
                data_type,
            };
            self.cg.vars.insert(name.to_string(), info);
            return info;
        }

        // Not shared, so local to this procedure: a fresh, zeroed slot per
        // invocation, which is what makes recursion work.
        let slot = self.local_slot(8 * words as u32);
        let info = VarInfo {
            loc: Loc::Stack(slot, 0),
            data_type,
        };
        self.proc_vars.insert(name.to_string(), info);
        info
    }

    /// Find an already-allocated variable slot, without creating one.
    pub(super) fn lookup_var(&self, name: &str) -> Option<VarInfo> {
        if self.current_proc.is_some()
            && let Some(info) = self.proc_vars.get(name)
        {
            return Some(*info);
        }
        if !self.module_visible(name, false) {
            return None;
        }
        self.cg.vars.get(name).copied()
    }

    /// Find an array, preferring the current procedure's own over a
    /// module-level array of the same name.
    pub(super) fn lookup_array(&self, name: &str) -> Option<ArrayInfo> {
        if self.current_proc.is_some()
            && let Some(info) = self.proc_arrays.get(name)
        {
            return Some(*info);
        }
        if !self.module_visible(name, true) {
            return None;
        }
        self.cg.arrays.get(name).copied()
    }

    /// Reserve a descriptor for every array declared in the current scope,
    /// before any code: an array used earlier in program order than its DIM
    /// still has somewhere to read from.
    pub(super) fn reserve_array_descriptors(&mut self) {
        let scope = self.sema_scope();
        let mut decls: Vec<(String, usize)> = self
            .cg
            .symbols
            .arrays
            .iter()
            .filter(|((s, _), _)| *s == scope)
            .map(|((_, name), info)| (name.clone(), info.rank))
            .collect();
        decls.sort();

        for (name, rank) in decls {
            if self.lookup_array(&name).is_some() {
                continue;
            }
            let bytes = 8 * (1 + 2 * rank as u32);
            let loc = match self.current_proc.clone() {
                Some(proc) if self.cg.symbols.is_static(&proc, &name, true) => {
                    let sym = format!("_sarr_{}_{}", mangle(&proc), mangle(&name));
                    Loc::Data(self.cg.global(&sym, bytes), 0)
                }
                Some(_) => Loc::Stack(self.local_slot(bytes), 0),
                None => Loc::Data(self.cg.global(&format!("_arr_{}", mangle(&name)), bytes), 0),
            };
            let info = ArrayInfo { loc, ndims: rank };
            if self.current_proc.is_some() {
                self.proc_arrays.insert(name, info);
            } else {
                self.cg.arrays.insert(name, info);
            }
        }
    }

    // ---- typed variables and records ----

    /// Declared type of a variable, when it was given one with `DIM ... AS`.
    pub(super) fn typed_var(&self, name: &str) -> Option<TypeRef> {
        if let Some(t) = self.proc_types.get(name) {
            return Some(t.clone());
        }
        self.cg.symbols.typed_var(&self.sema_scope(), name).cloned()
    }

    /// Whether `name` already has ordinary variable storage.
    /// A name held in an SSA variable counts: a BYVAL parameter declared
    /// `AS` is one, and its declared type is not typed storage.
    pub(super) fn has_plain_slot(&self, name: &str) -> bool {
        self.ssa.contains_key(name)
            || (self.current_proc.is_some() && self.proc_vars.contains_key(name))
            || (self.module_visible(name, false) && self.cg.vars.contains_key(name))
    }

    /// Typed storage for a variable declared with `AS`, if that is where it
    /// actually lives.
    pub(super) fn typed_storage(&mut self, name: &str) -> Option<(Loc, TypeRef)> {
        if self.has_plain_slot(name) && self.record_loc_of(name).is_none() {
            return None;
        }
        let ty = self.typed_var(name)?;
        let loc = self.get_record_loc(name, &ty);
        Some((loc, ty))
    }

    /// Whether the current procedure declares `name` itself, as a typed
    /// parameter or a local `DIM ... AS`.
    fn declared_in_current_proc(&self, name: &str) -> bool {
        let Some(proc) = &self.current_proc else {
            return false;
        };
        self.proc_types.contains_key(name)
            || self
                .cg
                .symbols
                .typed_var_in(&SemaScope::Proc(proc.clone()), name)
                .is_some()
    }

    /// Storage for a variable declared with `DIM ... AS`, allocating it the
    /// first time it is seen.
    pub(super) fn get_record_loc(&mut self, name: &str, ty: &TypeRef) -> Loc {
        if let Some(loc) = self.record_loc_of(name) {
            return loc;
        }
        let words = self.cg.symbols.type_words(ty);
        let bytes = 8 * words as u32;

        if self.declared_in_current_proc(name) {
            let proc = self.current_proc.clone().expect("declared in a procedure");
            let loc = if self.cg.symbols.is_static(&proc, name, false) {
                let sym = format!("_srec_{}_{}", mangle(&proc), mangle(name));
                Loc::Data(self.cg.global(&sym, bytes), 0)
            } else {
                Loc::Stack(self.local_slot(bytes), 0)
            };
            self.proc_record_vars.insert(name.to_string(), loc);
            return loc;
        }

        let loc = Loc::Data(self.cg.global(&format!("_rec_{}", mangle(name)), bytes), 0);
        self.cg.record_vars.insert(name.to_string(), loc);
        loc
    }

    /// Where a typed variable already lives, preferring the current
    /// procedure's own declarations over module-level ones.
    pub(super) fn record_loc_of(&self, name: &str) -> Option<Loc> {
        if self.current_proc.is_some()
            && let Some(loc) = self.proc_record_vars.get(name)
        {
            return Some(*loc);
        }
        if !self.module_visible(name, false) {
            return None;
        }
        self.cg.record_vars.get(name).copied()
    }

    /// Element size in words of an array of records.
    pub(super) fn record_elem_words_of(&self, name: &str) -> Option<i32> {
        let upper = name.to_uppercase();
        if self.current_proc.is_some()
            && let Some(w) = self.proc_record_elem_words.get(&upper)
        {
            return Some(*w);
        }
        if !self.module_visible(&upper, true) {
            return None;
        }
        self.cg.record_elem_words.get(&upper).copied()
    }

    /// Type of an array's elements, when it was declared `DIM a(n) AS T`.
    pub(super) fn array_elem_type(&self, name: &str) -> Option<TypeRef> {
        let ty = self.typed_var(name)?;
        (matches!(ty, TypeRef::Record(_)) && self.record_elem_words_of(name).is_some())
            .then_some(ty)
    }

    /// The type an element of the scalar array `name` holds: its `AS`
    /// clause -- `DIM A(3) AS INTEGER` is an array of INTEGERs whatever the
    /// name says -- or its name's suffix.
    pub(super) fn array_value_type(&self, name: &str) -> TypeRef {
        match self.typed_var(name) {
            Some(TypeRef::Record(_)) | None => type_ref_of(DataType::from_suffix(name)),
            Some(t) => t,
        }
    }

    /// Resolve `base.field...` to a storage location and the field's type.
    pub(super) fn resolve_field_path(
        &mut self,
        name: &str,
        fields: &[String],
    ) -> Option<(Loc, TypeRef)> {
        let mut ty = self.typed_var(name)?;
        let mut loc = self.get_record_loc(name, &ty);
        for field in fields {
            let TypeRef::Record(rec) = &ty else {
                return None;
            };
            let f = self.cg.symbols.records.get(normalized(rec))?.field(field)?;
            loc = loc.offset(f.word * 8);
            ty = f.ty.clone();
        }
        Some((loc, ty))
    }

    /// Byte offset of a field path within `base`, and the type it arrives at.
    pub(super) fn field_byte_offset(
        &self,
        base: &TypeRef,
        fields: &[String],
    ) -> Option<(i32, TypeRef)> {
        let mut ty = base.clone();
        let mut offset = 0i32;
        for field in fields {
            let TypeRef::Record(rec) = &ty else {
                return None;
            };
            let f = self.cg.symbols.records.get(normalized(rec))?.field(field)?;
            offset += f.word * 8;
            ty = f.ty.clone();
        }
        Some((offset, ty))
    }

    /// Load a value of declared type `ty` from `loc`.
    pub(super) fn load_typed(&mut self, loc: Loc, ty: &TypeRef) -> Val {
        match ty {
            // A whole record has no scalar value; sema rejects using one here.
            TypeRef::Record(_) => Val::Num(DataType::Double, self.f64const(0.0)),
            t => self.load_val(DataType::from_type_ref(t), loc),
        }
    }

    /// Store `v` into `loc`, of declared type `ty`, converting it.
    pub(super) fn store_typed(&mut self, loc: Loc, ty: &TypeRef, v: Val) {
        match ty {
            // A plain string variable: see `type_ref_of`.
            TypeRef::FixedString(0) => self.assign_str(loc, v),
            TypeRef::FixedString(width) => {
                // A fixed-length string holds exactly its declared width: a
                // short value is space-padded, a long one truncated.
                let (p, l) = v.str();
                let w = self.iconst(types::I64, *width as i64);
                let r = self.call_rt("_rt_fixed", &[p, l, w]);
                self.assign_str(loc, Val::Str(r[0], r[1]));
            }
            TypeRef::Record(_) => {}
            t => {
                let dt = DataType::from_type_ref(t);
                let v = self.coerce(v, dt);
                self.store_num(dt, v.num(), loc);
            }
        }
    }

    // ---- arrays ----

    /// Whether subscripts of `name` have a lower bound to take off.
    fn array_has_lower(&self, name: &str) -> bool {
        self.cg.symbols.option_base != 0
            || self
                .cg
                .symbols
                .lookup_array(&self.sema_scope(), &name.to_uppercase())
                .is_some_and(|a| a.has_lower)
    }

    /// Size in bytes of an element of the array `name`.
    pub(super) fn elem_size_for(&self, name: &str) -> i32 {
        if let Some(words) = self.record_elem_words_of(name) {
            return words * 8;
        }
        elem_size(DataType::from_type_ref(&self.array_value_type(name)))
    }

    /// Create `name` with GW-BASIC's default bounds if it has no storage yet.
    fn autodim_if_null(&mut self, name: &str, info: ArrayInfo) {
        let ptr = self.load(types::I64, info.loc);
        let create = self.b.create_block();
        let have = self.b.create_block();
        self.b.set_cold_block(create);
        self.brif(ptr, have, create);
        self.switch_sealed(create);
        let desc = self.addr(info.loc);
        let nd = self.iconst(types::I64, info.ndims as i64);
        let es = self.iconst(types::I64, i64::from(self.elem_size_for(name)));
        let base = self.iconst(types::I64, self.cg.symbols.option_base);
        self.call_rt("_rt_array_autodim", &[desc, nd, es, base]);
        self.begin_sealed(have);
    }

    /// An array element's address, bounds-checked.
    pub(super) fn array_addr(&mut self, name: &str, indices: &[Expr]) -> Value {
        let info = self
            .lookup_array(name)
            .expect("sema checked the array is declared");
        let elem_size = self.elem_size_for(name);

        // An array with no storage yet -- never DIMmed, or ERASEd -- is
        // created by this use, as GW-BASIC does.
        self.autodim_if_null(name, info);

        let ndims = indices.len() as i32;
        let has_lower = self.array_has_lower(name);

        // Row-major: linear = ((i * dim1) + j) * dim2 + k. Every subscript
        // is evaluated, in order, before the descriptor is read: evaluating
        // one may call a procedure that REDIMs the array.
        let mut subs = Vec::with_capacity(indices.len());
        for idx in indices {
            let v = self.gen_expr(idx);
            subs.push(self.i64_of(v));
        }
        let mut linear: Option<Value> = None;
        for (i, sub) in subs.into_iter().enumerate() {
            let mut sub = sub;
            if has_lower {
                let lo = self.load(types::I64, info.loc.offset(8 * (1 + ndims + i as i32)));
                sub = self.b.ins().isub(sub, lo);
            }
            let count = self.load(types::I64, info.loc.offset(8 * (1 + i as i32)));
            // One unsigned compare catches both a subscript below the lower
            // bound and one past the end.
            if self.cg.opts.checks {
                let bad = self
                    .b
                    .ins()
                    .icmp(IntCC::UnsignedGreaterThanOrEqual, sub, count);
                self.raise_if(bad, RtError::Subscript);
            }
            linear = Some(match linear {
                None => sub,
                Some(acc) => {
                    let m = self.b.ins().imul(acc, count);
                    self.b.ins().iadd(m, sub)
                }
            });
        }
        let linear = linear.expect("at least one subscript");
        let off = self.b.ins().imul_imm_s(linear, i64::from(elem_size));
        let base = self.load(types::I64, info.loc);
        self.b.ins().iadd(base, off)
    }

    /// Load an element of the array `name`.
    pub(super) fn array_load(&mut self, name: &str, indices: &[Expr]) -> Val {
        let ty = self.array_value_type(name);
        let a = self.array_addr(name, indices);
        self.load_typed(Loc::Ptr(a, 0), &ty)
    }

    /// Allocate or reallocate an array's storage.
    ///
    /// The descriptor -- word 0 the element pointer, word 1+i dimension i's
    /// element count, word 1+n+i its lower bound -- is reused when the array
    /// already exists, so REDIM resizes in place rather than orphaning it.
    pub(super) fn gen_array_alloc(&mut self, arr: &ArrayDecl, redim: bool, preserve: bool) {
        let elem_size = i64::from(self.elem_size_for(&arr.name));
        let ndims = arr.dimensions.len();

        let loc = match self.lookup_array(&arr.name).map(|i| i.loc) {
            Some(existing) => existing,
            None if self.current_proc.is_some() => {
                Loc::Stack(self.local_slot(8 * (1 + 2 * ndims as u32)), 0)
            }
            None => Loc::Data(
                self.cg.global(
                    &format!("_arr_{}", mangle(&arr.name)),
                    8 * (1 + 2 * ndims as u32),
                ),
                0,
            ),
        };

        // A DIM of an array that exists already is "Duplicate definition".
        if !redim && self.cg.opts.checks {
            let p = self.load(types::I64, loc);
            let exists = self.b.ins().icmp_imm_s(IntCC::NotEqual, p, 0);
            self.raise_if(exists, RtError::DuplicateDefinition);
        }

        // Each dimension: the lower bound as written, or the OPTION BASE, and
        // the element count from it to the upper one. A bound below its
        // lower one leaves no elements, which is Subscript out of range.
        //
        // Nothing reaches the descriptor until the new block exists. A
        // trapped error on the way -- a bad bound, a size too large, no
        // memory -- must leave the array as it was: new bounds beside the
        // old block would pass every bounds check and index past its end.
        let mut bounds = Vec::with_capacity(ndims);
        for (i, dim) in arr.dimensions.iter().enumerate() {
            let lo = match arr.lowers.get(i).cloned().flatten() {
                Some(lo) => {
                    let v = self.gen_expr(&lo);
                    self.i64_of(v)
                }
                None => self.iconst(types::I64, self.cg.symbols.option_base),
            };
            let v = self.gen_expr(dim);
            let hi = self.i64_of(v);
            let d = self.b.ins().isub(hi, lo);
            let count = self.b.ins().iadd_imm_s(d, 1);
            if self.cg.opts.checks {
                let bad = self
                    .b
                    .ins()
                    .icmp_imm_s(IntCC::SignedLessThanOrEqual, count, 0);
                self.raise_if(bad, RtError::Subscript);
            }
            bounds.push((lo, count));
        }

        let counts: Vec<Value> = bounds.iter().map(|(_, c)| *c).collect();
        let size = self.checked_size(&counts, elem_size);
        if preserve {
            // Elements past the new end are lost to realloc: free the
            // strings they hold first.
            let mut new_count = self.iconst(types::I64, 1);
            for c in &counts {
                new_count = self.b.ins().imul(new_count, *c);
            }
            self.free_array_strings_from(&arr.name, loc, ndims, new_count);
        }
        let (mem, old_size) = if preserve {
            let n = ndims as i32;
            let old_counts: Vec<Value> = (0..n)
                .map(|i| self.load(types::I64, loc.offset(8 * (1 + i))))
                .collect();
            let old_size = self.checked_size(&old_counts, elem_size);
            let old = self.load(types::I64, loc);
            (self.call("realloc", &[old, size])[0], Some(old_size))
        } else {
            // calloc: a fresh array reads as 0 / "".
            let one = self.iconst(types::I64, 1);
            (self.call("calloc", &[one, size])[0], None)
        };
        if self.cg.opts.checks {
            // On failure realloc leaves the old block as it was, and calloc
            // made none, so the descriptor still describes what it points at.
            let null = self.b.ins().icmp_imm_s(IntCC::Equal, mem, 0);
            self.raise_if(null, RtError::OutOfMemory);
        }

        if !preserve {
            // The old elements, if any, and the strings they hold.
            self.free_array_storage(&arr.name, loc, ndims);
        }
        self.store(mem, loc);
        let n = ndims as i32;
        for (i, (lo, count)) in bounds.into_iter().enumerate() {
            self.store(count, loc.offset(8 * (1 + i as i32)));
            self.store(lo, loc.offset(8 * (1 + n + i as i32)));
        }

        if let Some(old) = old_size {
            // Zero the newly added tail, from the old size up to the new one.
            let grown = self.b.ins().icmp(IntCC::UnsignedLessThan, old, size);
            let zero_it = self.b.create_block();
            let done = self.b.create_block();
            self.brif(grown, zero_it, done);
            self.switch_sealed(zero_it);
            let start = self.b.ins().iadd(mem, old);
            let len = self.b.ins().isub(size, old);
            let z = self.iconst(types::I32, 0);
            self.call("memset", &[start, z, len]);
            self.begin_sealed(done);
        }

        let info = ArrayInfo { loc, ndims };
        if self.current_proc.is_some() {
            self.proc_arrays.insert(arr.name.clone(), info);
        } else {
            self.cg.arrays.insert(arr.name.clone(), info);
        }
    }

    /// The number of elements the descriptor at `loc` describes.
    fn element_count(&mut self, loc: Loc, ndims: usize) -> Value {
        let mut n = self.iconst(types::I64, 1);
        for i in 0..ndims {
            let c = self.load(types::I64, loc.offset(8 * (1 + i as i32)));
            n = self.b.ins().imul(n, c);
        }
        n
    }

    /// Free the strings of elements `from..` of the array `name` whose
    /// descriptor is at `loc`, if it has storage and holds strings.
    fn free_array_strings_from(&mut self, name: &str, loc: Loc, ndims: usize, from: Value) {
        let Some((es, offsets)) = self.array_string_layout(name) else {
            return;
        };
        let ptr = self.load(types::I64, loc);
        let some = self.b.create_block();
        let done = self.b.create_block();
        self.brif(ptr, some, done);
        self.switch_sealed(some);
        let count = self.element_count(loc, ndims);
        self.free_element_strings(ptr, from, count, es, &offsets);
        self.begin_sealed(done);
    }

    /// Free the array `name`'s element block and the strings in it. The
    /// descriptor is left as it is.
    pub(super) fn free_array_storage(&mut self, name: &str, loc: Loc, ndims: usize) {
        let zero = self.iconst(types::I64, 0);
        self.free_array_strings_from(name, loc, ndims, zero);
        let ptr = self.load(types::I64, loc);
        self.call("free", &[ptr]);
    }

    /// The bytes `counts` elements of `elem_size` take.
    ///
    /// A size that does not fit 64 bits is Out of memory. Wrapping instead
    /// -- 65536^4 two-byte elements is 2^65 bytes, which wraps to 0 -- would
    /// allocate a small block that every subscript's bounds check still
    /// admits, and writes would land outside it.
    fn checked_size(&mut self, counts: &[Value], elem_size: i64) -> Value {
        let mut n = self.iconst(types::I64, elem_size);
        for c in counts {
            n = if self.cg.opts.checks {
                let (p, of) = self.b.ins().umul_overflow(n, *c);
                self.raise_if(of, RtError::OutOfMemory);
                p
            } else {
                self.b.ins().imul(n, *c)
            };
        }
        n
    }

    /// Emit storage for one DIM/REDIM declarator.
    pub(super) fn gen_declarator(&mut self, decl: &Declarator, redim: bool, preserve: bool) {
        match (&decl.dimensions, &decl.ty) {
            (Some(dims), ty) => {
                // An array of records has elements of the record's size; one
                // declared AS a scalar type sizes them by that type.
                if let Some(ty @ TypeRef::Record(_)) = ty {
                    let words = self.cg.symbols.type_words(ty);
                    let key = decl.name.to_uppercase();
                    if self.current_proc.is_some() {
                        self.proc_record_elem_words.insert(key, words);
                    } else {
                        self.cg.record_elem_words.insert(key, words);
                    }
                }
                let arr = ArrayDecl {
                    name: decl.name.clone(),
                    dimensions: dims.clone(),
                    lowers: decl.lowers.clone(),
                };
                self.gen_array_alloc(&arr, redim, preserve);
            }
            // A typed scalar: allocating its storage is all that is needed,
            // since data objects and the frame prologue already zero it.
            (None, Some(ty)) => {
                let ty = ty.clone();
                self.get_record_loc(&decl.name, &ty);
            }
            (None, None) => {}
        }
    }

    // ---- assignment targets ----

    /// Split `arr(i).f...` into its array name, subscripts and field path.
    pub(super) fn flatten_indexed_field_path(
        expr: &Expr,
    ) -> Option<(String, Vec<Expr>, Vec<String>)> {
        let mut fields = Vec::new();
        let mut cur = expr;
        loop {
            match cur {
                Expr::Field { base, field } => {
                    fields.push(field.clone());
                    cur = base;
                }
                Expr::ArrayAccess { name, indices }
                | Expr::FnCall {
                    name,
                    args: indices,
                } => {
                    fields.reverse();
                    return Some((name.clone(), indices.clone(), fields));
                }
                _ => return None,
            }
        }
    }

    /// Split a field-access expression into its base variable and field path.
    pub(super) fn flatten_field_path(expr: &Expr) -> Option<(String, Vec<String>)> {
        let mut fields = Vec::new();
        let mut cur = expr;
        loop {
            match cur {
                Expr::Field { base, field } => {
                    fields.push(field.clone());
                    cur = base;
                }
                Expr::Variable(name) => {
                    fields.reverse();
                    return Some((name.clone(), fields));
                }
                _ => return None,
            }
        }
    }

    /// The expression form of an assignment target.
    pub(super) fn lvalue_expr(target: &LValue) -> Expr {
        let mut expr = match &target.indices {
            Some(indices) => Expr::ArrayAccess {
                name: target.name.clone(),
                indices: indices.clone(),
            },
            None => Expr::Variable(target.name.clone()),
        };
        for field in &target.fields {
            expr = Expr::Field {
                base: Box::new(expr),
                field: field.clone(),
            };
        }
        expr
    }

    /// Where an assignment target lives and its declared type, its address
    /// computed now -- after any value it is to receive.
    pub(super) fn lvalue_loc(&mut self, target: &LValue) -> (Loc, TypeRef) {
        if !target.fields.is_empty() {
            if let Some(indices) = &target.indices {
                let base = self
                    .array_elem_type(&target.name)
                    .expect("sema checked this is an array of records");
                let (offset, ty) = self
                    .field_byte_offset(&base, &target.fields)
                    .expect("sema checked the field path");
                let a = self.array_addr(&target.name, indices);
                return (Loc::Ptr(a, offset), ty);
            }
            return self
                .resolve_field_path(&target.name, &target.fields)
                .expect("sema resolved this field path");
        }
        if let Some(indices) = &target.indices {
            let a = self.array_addr(&target.name, indices);
            return (Loc::Ptr(a, 0), self.array_value_type(&target.name));
        }
        if let Some((loc, ty)) = self.typed_storage(&target.name) {
            return (loc, ty);
        }
        let info = self.get_var_info(&target.name);
        (info.loc, type_ref_of(info.data_type))
    }

    /// Where an assignment target lives: in memory, or -- a plain scalar
    /// nothing else can reach -- in an SSA variable.
    pub(super) fn lvalue_place(&mut self, target: &LValue) -> Place {
        if target.fields.is_empty()
            && target.indices.is_none()
            && self.typed_storage(&target.name).is_none()
            && let Some((v, t)) = self.ssa_var(&target.name)
        {
            return Place::Var(v, t);
        }
        let (loc, ty) = self.lvalue_loc(target);
        Place::Mem(loc, ty)
    }

    /// The place a plain variable name lives.
    pub(super) fn var_place(&mut self, name: &str) -> Place {
        self.lvalue_place(&LValue {
            name: name.to_string(),
            indices: None,
            fields: Vec::new(),
        })
    }

    /// Store `v` into `place`, converting it to the place's type.
    pub(super) fn store_place(&mut self, place: &Place, v: Val) {
        match place {
            Place::Mem(loc, ty) => self.store_typed(*loc, ty, v),
            Place::Var(var, t) => self.def_ssa(*var, *t, v),
        }
    }

    /// The value at `place`.
    pub(super) fn load_place(&mut self, place: &Place) -> Val {
        match place {
            Place::Mem(loc, ty) => self.load_typed(*loc, ty),
            Place::Var(var, t) => Val::Num(*t, self.b.use_var(*var)),
        }
    }

    /// Store `v` into `target`, converting it to the target's type. A
    /// string target owns what it holds: a temporary, or a copy.
    pub(super) fn store_lvalue(&mut self, target: &LValue, v: Val) {
        let place = self.lvalue_place(target);
        self.store_place(&place, v);
    }

    /// Store a string into a variable without copying it: what makes a FIELD
    /// variable alias the record buffer.
    pub(super) fn bind_alias(&mut self, target: &LValue, p: Value, l: Value) {
        let (loc, _) = self.lvalue_loc(target);
        self.bind_str(loc, p, l);
    }
}

/// Where an assignment target lives.
#[derive(Clone)]
pub(super) enum Place {
    Mem(Loc, TypeRef),
    Var(Variable, DataType),
}

/// The declared-type form of a plain BASIC type: a string variable is a
/// string of no fixed width, which `store_typed` stores as a string.
pub(super) fn type_ref_of(t: DataType) -> TypeRef {
    match t {
        DataType::Integer => TypeRef::Integer,
        DataType::Long => TypeRef::Long,
        DataType::Single => TypeRef::Single,
        DataType::Double => TypeRef::Double,
        // Width 0 is never a declared width; store_typed treats it as plain.
        DataType::String => TypeRef::FixedString(0),
    }
}
