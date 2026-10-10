//! Expressions: types, coercions, operators and conditions

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use super::*;
use crate::lexer::normalized;

/// True for the six relational operators.
pub(super) fn is_comparison(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge
    )
}

/// The signed integer condition for a comparison: integers and string
/// comparisons, whose results read like memcmp's.
pub(super) fn int_cc(op: BinaryOp) -> IntCC {
    match op {
        BinaryOp::Eq => IntCC::Equal,
        BinaryOp::Ne => IntCC::NotEqual,
        BinaryOp::Lt => IntCC::SignedLessThan,
        BinaryOp::Gt => IntCC::SignedGreaterThan,
        BinaryOp::Le => IntCC::SignedLessThanOrEqual,
        BinaryOp::Ge => IntCC::SignedGreaterThanOrEqual,
        _ => unreachable!("only comparisons"),
    }
}

/// The float condition for a comparison. NaN follows IEEE: every comparison
/// with it is false except `<>`.
pub(super) fn float_cc(op: BinaryOp) -> FloatCC {
    match op {
        BinaryOp::Eq => FloatCC::Equal,
        BinaryOp::Ne => FloatCC::NotEqual,
        BinaryOp::Lt => FloatCC::LessThan,
        BinaryOp::Gt => FloatCC::GreaterThan,
        BinaryOp::Le => FloatCC::LessThanOrEqual,
        BinaryOp::Ge => FloatCC::GreaterThanOrEqual,
        _ => unreachable!("only comparisons"),
    }
}

/// Promote two types to a common type for binary operations.
pub(super) fn promote_types(left: DataType, right: DataType, op: BinaryOp) -> DataType {
    // Comparisons return -1/0, a LONG.
    if is_comparison(op) {
        return DataType::Long;
    }
    match op {
        // `/` and `^` always produce Double, as in GW-BASIC.
        BinaryOp::Div | BinaryOp::Pow => return DataType::Double,
        // `\`, MOD and the bitwise operators convert their operands to LONG.
        BinaryOp::IntDiv
        | BinaryOp::Mod
        | BinaryOp::And
        | BinaryOp::Or
        | BinaryOp::Xor
        | BinaryOp::Eqv
        | BinaryOp::Imp => return DataType::Long,
        _ => {}
    }
    if left == DataType::String && right == DataType::String {
        return DataType::String;
    }
    match (left, right) {
        (DataType::Double, _) | (_, DataType::Double) => DataType::Double,
        (DataType::Single, _) | (_, DataType::Single) => DataType::Single,
        (DataType::Long, _) | (_, DataType::Long) => DataType::Long,
        _ => DataType::Integer,
    }
}

/// Result type of ABS, which preserves its argument's type.
pub(super) fn abs_result_type(arg: DataType) -> DataType {
    match arg {
        DataType::Integer | DataType::Long => DataType::Long,
        DataType::Single => DataType::Single,
        _ => DataType::Double,
    }
}

impl FnGen<'_> {
    /// The type of an expression, without generating it.
    pub(super) fn expr_type(&self, expr: &Expr) -> DataType {
        match expr {
            Expr::Literal(lit) => lit.numeric_type().unwrap_or(DataType::String),
            Expr::Variable(name) if crate::sema::is_zero_arg_builtin(name) => {
                self.fn_return_type(name)
            }
            Expr::Variable(name) => match self.cg.symbols.consts.get(normalized(name)) {
                Some(lit) => lit.numeric_type().unwrap_or(DataType::String),
                // An allocated slot knows its own type -- a parameter declared
                // `N AS INTEGER` the declared one, not its name's.
                None => match self.lookup_var(name) {
                    Some(info) => info.data_type,
                    None => match self.typed_var(name) {
                        Some(t) => DataType::from_type_ref(&t),
                        None => {
                            let upper = name.to_uppercase();
                            let is_own = self.current_proc.as_deref() == Some(upper.as_str());
                            match self.cg.symbols.procs.get(&upper) {
                                Some(p) if p.is_function && !is_own => {
                                    self.cg.proc_return_type(&upper)
                                }
                                _ => DataType::from_suffix(name),
                            }
                        }
                    },
                },
            },
            Expr::ArrayAccess { name, .. } => DataType::from_suffix(name),
            Expr::FnCall { name, args } => self.call_return_type(name, args),
            Expr::Field { .. } => self.field_expr_type(expr),
            // NOT is bitwise and yields an integer, as AND, OR and XOR do.
            Expr::Unary {
                op: UnaryOp::Not, ..
            } => DataType::Long,
            Expr::Unary { operand, .. } => self.expr_type(operand),
            Expr::Binary { left, right, op } => {
                promote_types(self.expr_type(left), self.expr_type(right), *op)
            }
        }
    }

    /// Return type of a call, including builtins whose result type depends on
    /// their argument.
    pub(super) fn call_return_type(&self, name: &str, args: &[Expr]) -> DataType {
        if name.eq_ignore_ascii_case("ABS") && !args.is_empty() {
            return abs_result_type(self.expr_type(&args[0]));
        }
        self.fn_return_type(name)
    }

    pub(super) fn fn_return_type(&self, name: &str) -> DataType {
        let upper = name.to_uppercase();
        if let Some(proc) = self.cg.symbols.procs.get(&upper)
            && let Some(ty) = &proc.ret_ty
        {
            return DataType::from_type_ref(ty);
        }
        if upper.ends_with('$') {
            return DataType::String;
        }
        match upper.as_str() {
            "CINT" => DataType::Integer,
            "LEN" | "ASC" | "INSTR" | "CLNG" => DataType::Long,
            "EOF" | "LBOUND" | "UBOUND" | "POS" | "FRE" | "ERR" | "ERL" => DataType::Long,
            "CSNG" => DataType::Single,
            _ => DataType::from_suffix(name),
        }
    }

    /// Result type of a field access.
    fn field_expr_type(&self, expr: &Expr) -> DataType {
        let (name, fields) = match Self::flatten_indexed_field_path(expr) {
            Some((n, _, f)) => (n, f),
            None => match Self::flatten_field_path(expr) {
                Some(v) => v,
                None => return DataType::Double,
            },
        };
        let Some(base) = self.typed_var(&name) else {
            return DataType::Double;
        };
        match self.field_byte_offset(&base, &fields) {
            Some((_, ty)) => DataType::from_type_ref(&ty),
            None => DataType::Double,
        }
    }

    // ---- conversions ----

    /// Convert `v` to type `to`, as an assignment would.
    ///
    /// Integer to float is exact or rounds; a float to an integer rounds half
    /// to even and is Overflow when it does not fit; a Double too large for
    /// a SINGLE is Overflow; a LONG too large for an INTEGER is Overflow.
    pub(super) fn coerce(&mut self, v: Val, to: DataType) -> Val {
        let from = v.ty();
        if from == to {
            return v;
        }
        let x = v.num();
        let r = match (from, to) {
            (DataType::Integer, DataType::Long) => {
                // Only an unchecked INTEGER result can stray past 16 bits;
                // it is truncated here, as the 16-bit store would.
                if self.cg.opts.checks {
                    x
                } else {
                    let n = self.b.ins().ireduce(types::I16, x);
                    self.b.ins().sextend(types::I32, n)
                }
            }
            (DataType::Long, DataType::Integer) => {
                self.check_i16(x);
                x
            }
            (DataType::Integer | DataType::Long, DataType::Single) => {
                self.b.ins().fcvt_from_sint(types::F32, x)
            }
            (DataType::Integer | DataType::Long, DataType::Double) => {
                self.b.ins().fcvt_from_sint(types::F64, x)
            }
            (DataType::Single, DataType::Double) => self.b.ins().fpromote(types::F64, x),
            (DataType::Double, DataType::Single) => {
                let s = self.b.ins().fdemote(types::F32, x);
                self.check_float(s, DataType::Single);
                s
            }
            (DataType::Single | DataType::Double, DataType::Integer | DataType::Long) => {
                let n = self.float_to_long(x, from);
                if to == DataType::Integer {
                    self.check_i16(n);
                }
                n
            }
            _ => unreachable!("sema rejects converting between a string and a number"),
        };
        Val::Num(to, r)
    }

    /// A float rounded half to even to an `i32`: Overflow when the result is
    /// outside LONG's range, or NaN.
    fn float_to_long(&mut self, x: Value, from: DataType) -> Value {
        // A SINGLE widens exactly, and rounds the same in either width.
        let x = if from == DataType::Single {
            self.b.ins().fpromote(types::F64, x)
        } else {
            x
        };
        let r = self.b.ins().nearest(x);
        if self.cg.opts.checks {
            let lo = self.f64const(-2147483648.0);
            let hi = self.f64const(2147483647.0);
            let ge = self.b.ins().fcmp(FloatCC::GreaterThanOrEqual, r, lo);
            let le = self.b.ins().fcmp(FloatCC::LessThanOrEqual, r, hi);
            let ok = self.b.ins().band(ge, le);
            let bad = self.b.ins().bxor_imm_s(ok, 1);
            self.raise_if(bad, RtError::Overflow);
        }
        self.b.ins().fcvt_to_sint_sat(types::I32, r)
    }

    /// A number as a LONG `i32`, converted as assignment converts.
    pub(super) fn long_of(&mut self, v: Val) -> Value {
        self.coerce(v, DataType::Long).num()
    }

    /// A number as an `i64`: subscripts, string positions and counts.
    pub(super) fn i64_of(&mut self, v: Val) -> Value {
        let n = self.long_of(v);
        self.b.ins().sextend(types::I64, n)
    }

    /// A number as a Double.
    pub(super) fn double_of(&mut self, v: Val) -> Value {
        self.coerce(v, DataType::Double).num()
    }

    /// Evaluate `expr` as a Double.
    pub(super) fn gen_double(&mut self, expr: &Expr) -> Value {
        let v = self.gen_expr(expr);
        self.double_of(v)
    }

    /// Evaluate `expr` as an `i64` -- a LONG, sign-extended.
    pub(super) fn gen_i64(&mut self, expr: &Expr) -> Value {
        let v = self.gen_expr(expr);
        self.i64_of(v)
    }

    // ---- expressions ----

    /// Generate an expression.
    pub(super) fn gen_expr(&mut self, expr: &Expr) -> Val {
        match expr {
            Expr::Literal(lit) => self.gen_literal(lit),

            Expr::Variable(name) => {
                // A CONST is substituted with its folded value.
                if let Some(lit) = self.cg.symbols.consts.get(normalized(name)).cloned() {
                    return self.gen_literal(&lit);
                }
                // A variable declared with `AS` lives in typed storage.
                if let Some((loc, ty)) = self.typed_storage(name) {
                    return self.load_typed(loc, &ty);
                }
                let upper = name.to_uppercase();
                // The builtins that take no argument are named bare.
                if crate::sema::is_zero_arg_builtin(&upper) {
                    return self.gen_fn_call(&upper, &[]);
                }
                // So is a parameterless FUNCTION -- except inside its own
                // body, where the name is its result variable.
                let is_own_name = self.current_proc.as_deref() == Some(upper.as_str());
                if !is_own_name
                    && self
                        .cg
                        .symbols
                        .procs
                        .get(&upper)
                        .is_some_and(|p| p.is_function && p.params.is_empty())
                {
                    return self
                        .gen_call(&upper, &[])
                        .expect("a FUNCTION returns a value");
                }
                let info = self.get_var_info(name);
                self.load_val(info.data_type, info.loc)
            }

            Expr::ArrayAccess { name, indices } => self.array_load(name, indices),

            Expr::Unary { op, operand } => {
                let v = self.gen_expr(operand);
                match op {
                    UnaryOp::Neg => match v.ty() {
                        t @ (DataType::Integer | DataType::Long) => {
                            let x = v.num();
                            // -(-32768) does not fit an INTEGER, nor
                            // -(-2147483648) a LONG.
                            let zero = self.iconst(types::I32, 0);
                            let r = self.int_arith(BinaryOp::Sub, t, zero, x);
                            Val::Num(t, r)
                        }
                        t => {
                            let r = self.b.ins().fneg(v.num());
                            Val::Num(t, r)
                        }
                    },
                    // NOT is a bitwise complement, like AND and OR beside it.
                    UnaryOp::Not => {
                        let n = self.long_of(v);
                        let r = self.b.ins().bnot(n);
                        Val::Num(DataType::Long, r)
                    }
                }
            }

            Expr::Binary { op, left, right } => self.gen_binary(*op, left, right),

            Expr::FnCall { name, args } => self.gen_fn_call(name, args),

            Expr::Field { .. } => {
                if let Some((name, indices, fields)) = Self::flatten_indexed_field_path(expr) {
                    let base = self
                        .array_elem_type(&name)
                        .expect("sema checked this is an array of records");
                    let (offset, ty) = self
                        .field_byte_offset(&base, &fields)
                        .expect("sema checked the field path");
                    let a = self.array_addr(&name, &indices);
                    return self.load_typed(Loc::Ptr(a, offset), &ty);
                }
                let (name, fields) =
                    Self::flatten_field_path(expr).expect("sema rejects a field of a non-record");
                let (loc, ty) = self
                    .resolve_field_path(&name, &fields)
                    .expect("sema resolved this field path");
                self.load_typed(loc, &ty)
            }
        }
    }

    pub(super) fn gen_literal(&mut self, lit: &Literal) -> Val {
        match lit {
            Literal::Integer(n) | Literal::Long(n) => match i32::try_from(*n) {
                Ok(v) => {
                    let ty = lit.numeric_type().expect("numeric");
                    Val::Num(ty, self.iconst(types::I32, i64::from(v)))
                }
                // Wider than LONG: a Double rather than a truncation.
                Err(_) => Val::Num(DataType::Double, self.f64const(*n as f64)),
            },
            Literal::Float(f) => Val::Num(DataType::Double, self.f64const(*f)),
            Literal::Single(f) => Val::Num(DataType::Single, self.b.ins().f32const(*f as f32)),
            Literal::String(s) => {
                let (p, l) = self.str_literal(s);
                Val::Str(p, l)
            }
        }
    }

    /// `+`, `-` or `*` on integers of type `ty`, held in 32 bits: a LONG
    /// result that overflows, or an INTEGER one that leaves 16 bits, is
    /// Overflow.
    fn int_arith(&mut self, op: BinaryOp, ty: DataType, a: Value, b: Value) -> Value {
        if ty == DataType::Long && self.cg.opts.checks {
            let (r, of) = match op {
                BinaryOp::Add => self.b.ins().sadd_overflow(a, b),
                BinaryOp::Sub => self.b.ins().ssub_overflow(a, b),
                _ => self.b.ins().smul_overflow(a, b),
            };
            self.raise_if(of, RtError::Overflow);
            return r;
        }
        let r = match op {
            BinaryOp::Add => self.b.ins().iadd(a, b),
            BinaryOp::Sub => self.b.ins().isub(a, b),
            _ => self.b.ins().imul(a, b),
        };
        if ty == DataType::Integer {
            self.check_i16(r);
        }
        r
    }

    fn gen_binary(&mut self, op: BinaryOp, left: &Expr, right: &Expr) -> Val {
        let lt = self.expr_type(left);
        let rt = self.expr_type(right);

        // String concatenation.
        if lt == DataType::String && rt == DataType::String && op == BinaryOp::Add {
            let (lp, ll) = self.gen_expr(left).str();
            let (rp, rl) = self.gen_expr(right).str();
            let r = self.call_rt("_rt_strcat", &[lp, ll, rp, rl]);
            return Val::Str(r[0], r[1]);
        }

        // String comparison, through _rt_strcmp's memcmp-style result.
        if is_comparison(op) && lt == DataType::String && rt == DataType::String {
            let c = self.gen_string_compare(left, right);
            let t = self.b.ins().icmp_imm_s(int_cc(op), c, 0);
            return self.bool_val(t);
        }

        // Comparisons work in the promoted type but return LONG.
        let work = if is_comparison(op) {
            promote_types(lt, rt, BinaryOp::Add)
        } else {
            promote_types(lt, rt, op)
        };
        let l = self.gen_expr(left);
        let l = self.coerce(l, work).num();
        let r = self.gen_expr(right);
        let r = self.coerce(r, work).num();
        let integer = work.is_integer();

        let v = match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                if integer {
                    self.int_arith(op, work, l, r)
                } else {
                    let v = match op {
                        BinaryOp::Add => self.b.ins().fadd(l, r),
                        BinaryOp::Sub => self.b.ins().fsub(l, r),
                        _ => self.b.ins().fmul(l, r),
                    };
                    self.check_float(v, work);
                    v
                }
            }
            BinaryOp::Div => {
                // A divisor written into the program is decided here: a zero
                // one always raises, any other is never tested.
                let divisor = self.const_f64(right);
                if divisor == Some(0.0) && self.cg.opts.checks {
                    self.raise(RtError::DivideByZero);
                } else if divisor.is_none() && self.cg.opts.checks {
                    // Both +0.0 and -0.0.
                    let zero = self.f64const(0.0);
                    let z = self.b.ins().fcmp(FloatCC::Equal, r, zero);
                    self.raise_if(z, RtError::DivideByZero);
                }
                let v = self.b.ins().fdiv(l, r);
                self.check_float(v, DataType::Double);
                v
            }
            BinaryOp::IntDiv | BinaryOp::Mod => {
                if self.cg.opts.checks {
                    let z = self.b.ins().icmp_imm_s(IntCC::Equal, r, 0);
                    self.raise_if(z, RtError::DivideByZero);
                    // INT_MIN / -1 overflows the quotient.
                    let m1 = self.b.ins().icmp_imm_s(IntCC::Equal, r, -1);
                    let min = self
                        .b
                        .ins()
                        .icmp_imm_s(IntCC::Equal, l, i64::from(i32::MIN));
                    let both = self.b.ins().band(m1, min);
                    self.raise_if(both, RtError::Overflow);
                }
                if op == BinaryOp::IntDiv {
                    self.b.ins().sdiv(l, r)
                } else {
                    self.b.ins().srem(l, r)
                }
            }
            BinaryOp::Pow => self.gen_pow(l, r),
            _ if is_comparison(op) => {
                let c = if integer {
                    self.b.ins().icmp(int_cc(op), l, r)
                } else {
                    self.b.ins().fcmp(float_cc(op), l, r)
                };
                return self.bool_val(c);
            }
            BinaryOp::And => self.b.ins().band(l, r),
            BinaryOp::Or => self.b.ins().bor(l, r),
            BinaryOp::Xor => self.b.ins().bxor(l, r),
            // EQV is NOT (a XOR b); IMP is (NOT a) OR b.
            BinaryOp::Eqv => {
                let x = self.b.ins().bxor(l, r);
                self.b.ins().bnot(x)
            }
            BinaryOp::Imp => {
                let n = self.b.ins().bnot(l);
                self.b.ins().bor(n, r)
            }
            _ => unreachable!("every operator is handled"),
        };
        Val::Num(promote_types(lt, rt, op), v)
    }

    /// `l ^ r`, both Doubles: zero to a negative power is Division by zero,
    /// a NaN result Illegal function call, an infinite one Overflow.
    fn gen_pow(&mut self, l: Value, r: Value) -> Value {
        if self.cg.opts.checks {
            let zero = self.f64const(0.0);
            let lz = self.b.ins().fcmp(FloatCC::Equal, l, zero);
            let rn = self.b.ins().fcmp(FloatCC::LessThan, r, zero);
            let both = self.b.ins().band(lz, rn);
            self.raise_if(both, RtError::DivideByZero);
        }
        let v = self.call("pow", &[l, r])[0];
        if self.cg.opts.checks {
            let nan = self.b.ins().fcmp(FloatCC::Unordered, v, v);
            self.raise_if(nan, RtError::Domain);
        }
        self.check_float(v, DataType::Double);
        v
    }

    /// Compare two string expressions: `_rt_strcmp`'s signed i32.
    pub(super) fn gen_string_compare(&mut self, left: &Expr, right: &Expr) -> Value {
        let (lp, ll) = self.gen_expr(left).str();
        let (rp, rl) = self.gen_expr(right).str();
        self.call_rt("_rt_strcmp", &[lp, ll, rp, rl])[0]
    }

    /// Evaluate `cond` as a branch condition: an i8, true when the BASIC
    /// value is true. A float is true when it is not zero, NaN included.
    pub(super) fn gen_condition(&mut self, cond: &Expr) -> Value {
        if let Expr::Binary { op, left, right } = cond
            && is_comparison(*op)
        {
            let lt = self.expr_type(left);
            let rt = self.expr_type(right);
            if lt == DataType::String && rt == DataType::String {
                let c = self.gen_string_compare(left, right);
                return self.b.ins().icmp_imm_s(int_cc(*op), c, 0);
            }
            if lt != DataType::String && rt != DataType::String {
                let work = promote_types(lt, rt, BinaryOp::Add);
                let l = self.gen_expr(left);
                let l = self.coerce(l, work).num();
                let r = self.gen_expr(right);
                let r = self.coerce(r, work).num();
                return if work.is_integer() {
                    self.b.ins().icmp(int_cc(*op), l, r)
                } else {
                    self.b.ins().fcmp(float_cc(*op), l, r)
                };
            }
        }
        let v = self.gen_expr(cond);
        self.truth(v)
    }

    /// Whether a number is true: nonzero, with NaN true.
    pub(super) fn truth(&mut self, v: Val) -> Value {
        match v.ty() {
            DataType::Integer | DataType::Long => {
                self.b.ins().icmp_imm_s(IntCC::NotEqual, v.num(), 0)
            }
            _ => {
                let d = self.double_of(v);
                let zero = self.f64const(0.0);
                self.b.ins().fcmp(FloatCC::NotEqual, d, zero)
            }
        }
    }
}
