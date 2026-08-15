//! Semantic analysis.
//!
//! Runs between the parser and the code generator. Its job is to answer the
//! questions codegen would otherwise have to guess at, and to reject bad
//! programs with a diagnostic instead of a panic or a linker error.
//!
//! Before this pass existed, an unknown name was emitted as a call to
//! `_proc_<NAME>` and surfaced as `ld: undefined reference to _proc_PRIN`, and
//! an undeclared array aborted the compiler with a Rust panic. Both are now
//! errors that name the offending identifier and the line it is on.
//!
//! # What it collects
//!
//! [`Symbols`] records every procedure (with its parameter list), every array
//! declaration and its rank, and every branch target -- both numeric line
//! numbers and named labels.
//!
//! # Scoping
//!
//! Arrays are tracked per scope, since a `DIM` inside a procedure is local to
//! it and shadows a module-level array of the same name. Scalars need no such
//! table: their type comes from the name's suffix.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::parser::*;
use std::collections::{HashMap, HashSet};

/// Builtin functions, with the argument counts they accept.
///
/// Kept here rather than in codegen so that "is this name known?" has a single
/// answer. Codegen still owns *how* each one is emitted.
const BUILTINS: &[(&str, usize, usize)] = &[
    // name, min args, max args
    ("ABS", 1, 1),
    ("ASC", 1, 1),
    ("ATN", 1, 1),
    ("CDBL", 1, 1),
    ("CHR$", 1, 1),
    ("CINT", 1, 1),
    ("CLNG", 1, 1),
    ("COS", 1, 1),
    ("CSNG", 1, 1),
    ("EOF", 1, 1),
    ("EXP", 1, 1),
    ("FIX", 1, 1),
    ("HEX$", 1, 1),
    ("INSTR", 2, 3),
    ("INT", 1, 1),
    ("LBOUND", 1, 2),
    ("LCASE$", 1, 1),
    ("LEFT$", 2, 2),
    ("LEN", 1, 1),
    ("LOF", 1, 1),
    ("LTRIM$", 1, 1),
    ("LOG", 1, 1),
    ("MID$", 2, 3),
    ("OCT$", 1, 1),
    ("RIGHT$", 2, 2),
    ("RTRIM$", 1, 1),
    ("RND", 0, 1),
    ("SGN", 1, 1),
    ("SPC", 1, 1),
    ("SIN", 1, 1),
    ("SPACE$", 1, 1),
    ("SQR", 1, 1),
    ("STR$", 1, 1),
    ("STRING$", 2, 2),
    ("TAB", 1, 1),
    ("UBOUND", 1, 2),
    ("UCASE$", 1, 1),
    ("TAN", 1, 1),
    ("TIMER", 0, 1),
    ("VAL", 1, 1),
];

fn builtin(name: &str) -> Option<&'static (&'static str, usize, usize)> {
    BUILTINS.iter().find(|(n, _, _)| *n == name)
}

/// A procedure declaration.
#[derive(Debug, Clone)]
pub struct ProcInfo {
    pub is_function: bool,
    pub params: Vec<Param>,
    pub line: u32,
}

/// Layout of one field within a record.
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// Offset from the start of the record, in 8-byte words.
    pub word: i32,
    pub ty: TypeRef,
}

/// A user-defined TYPE, laid out.
///
/// Every field occupies a whole number of 8-byte words, matching how scalars
/// are stored everywhere else, so a field is addressed the same way a variable
/// is: a base location plus a word offset.
#[derive(Debug, Clone, Default)]
pub struct RecordInfo {
    /// Fields in declaration order, by upper-case name.
    pub fields: Vec<(String, FieldInfo)>,
    /// Total size in 8-byte words.
    pub words: i32,
    pub line: u32,
}

impl RecordInfo {
    pub fn field(&self, name: &str) -> Option<&FieldInfo> {
        let upper = name.to_uppercase();
        self.fields
            .iter()
            .find(|(n, _)| *n == upper)
            .map(|(_, f)| f)
    }
}

/// An array declaration.
#[derive(Debug, Clone)]
pub struct ArrayInfo {
    pub rank: usize,
    pub line: u32,
}

/// Which scope a name was declared in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {
    Module,
    Proc(String),
}

/// Everything the analysis learned about a program.
#[derive(Debug, Default)]
pub struct Symbols {
    pub procs: HashMap<String, ProcInfo>,
    pub arrays: HashMap<(Scope, String), ArrayInfo>,
    /// Named labels (`Retry:`) available as branch targets.
    pub labels: HashSet<String>,
    /// Numeric line-number labels available as branch targets.
    pub lines: HashSet<u32>,
    /// CONST names and their folded values.
    pub consts: HashMap<String, Literal>,
    /// Lowest legal subscript, set by OPTION BASE. Defaults to 0.
    pub option_base: i64,
    /// User-defined TYPEs, by upper-case name.
    pub records: HashMap<String, RecordInfo>,
    /// Variables declared with `DIM ... AS`, by (scope, upper-case name).
    pub typed_vars: HashMap<(Scope, String), TypeRef>,
}

impl Symbols {
    /// Size of a declared type, in 8-byte words.
    ///
    /// A string field is a (pointer, length) pair, so two words; a fixed-length
    /// string is stored the same way, with its declared length used only to
    /// pad or truncate on assignment.
    pub fn type_words(&self, ty: &TypeRef) -> i32 {
        match ty {
            TypeRef::Integer | TypeRef::Long | TypeRef::Single | TypeRef::Double => 1,
            TypeRef::FixedString(_) => 2,
            TypeRef::Record(name) => self
                .records
                .get(&name.to_uppercase())
                .map(|r| r.words)
                .unwrap_or(1),
        }
    }

    /// Look up a variable's declared type, preferring the current scope.
    pub fn typed_var(&self, scope: &Scope, name: &str) -> Option<&TypeRef> {
        let upper = name.to_uppercase();
        if let Scope::Proc(_) = scope {
            if let Some(t) = self.typed_vars.get(&(scope.clone(), upper.clone())) {
                return Some(t);
            }
        }
        self.typed_vars.get(&(Scope::Module, upper))
    }

    /// Look up an array visible from `scope`: a procedure-local declaration
    /// shadows a module-level one of the same name.
    pub fn lookup_array(&self, scope: &Scope, name: &str) -> Option<&ArrayInfo> {
        if let Scope::Proc(_) = scope {
            if let Some(info) = self.arrays.get(&(scope.clone(), name.to_string())) {
                return Some(info);
            }
        }
        self.arrays.get(&(Scope::Module, name.to_string()))
    }
}

/// A problem found in the program.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Source line, or 0 when unknown.
    pub line: u32,
    pub message: String,
    /// Optional follow-up line, e.g. a spelling suggestion.
    pub note: Option<String>,
}

/// Analyze a program: collect its symbols and report any problems.
pub fn analyze(program: &Program) -> (Symbols, Vec<Diagnostic>) {
    let mut a = Analyzer::default();
    a.collect(&program.statements, &Scope::Module);
    a.check(&program.statements, &Scope::Module);
    (a.symbols, a.diagnostics)
}

#[derive(Default)]
struct Analyzer {
    symbols: Symbols,
    diagnostics: Vec<Diagnostic>,
    /// Enclosing loops, innermost last: true for FOR, false for WHILE/DO.
    loops: Vec<bool>,
    /// Whether the walk is currently inside a procedure body.
    in_proc: bool,
    /// Whether an OPTION BASE has already been seen.
    seen_option_base: bool,
}

impl Analyzer {
    fn error(&mut self, line: u32, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            line,
            message: message.into(),
            note: None,
        });
    }

    fn error_with_note(&mut self, line: u32, message: impl Into<String>, note: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            line,
            message: message.into(),
            note: Some(note.into()),
        });
    }

    // ------------------------------------------------------------------
    // Pass 1: collect declarations
    // ------------------------------------------------------------------

    /// Record procedures, arrays and branch targets.
    ///
    /// Done as its own pass so that order does not matter: a program may call a
    /// procedure defined further down, or index an array before the `DIM` that
    /// declares it appears textually.
    fn collect(&mut self, stmts: &[Stmt], scope: &Scope) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Label(n) => {
                    self.symbols.lines.insert(*n);
                }
                StmtKind::LabelName(name) => {
                    if !self.symbols.labels.insert(name.clone()) {
                        self.error(stmt.line, format!("duplicate label '{}'", name));
                    }
                }
                StmtKind::TypeDef { name, fields } => {
                    let upper = name.to_uppercase();
                    if let Some(prev) = self.symbols.records.get(&upper) {
                        let prev_line = prev.line;
                        self.error_with_note(
                            stmt.line,
                            format!("TYPE '{}' is already defined", name),
                            format!("previous definition is on line {}", prev_line),
                        );
                    }
                    if scope != &Scope::Module {
                        self.error(
                            stmt.line,
                            format!("TYPE '{}' must be defined at module level", name),
                        );
                    }
                    // Lay the record out now. A field whose type is another
                    // record needs that one already defined, which is also what
                    // makes a cycle impossible.
                    let mut info = RecordInfo {
                        line: stmt.line,
                        ..Default::default()
                    };
                    for f in fields {
                        let fu = f.name.to_uppercase();
                        if info.field(&fu).is_some() {
                            self.error(
                                stmt.line,
                                format!("field '{}' is declared twice in TYPE '{}'", f.name, name),
                            );
                            continue;
                        }
                        if let TypeRef::Record(inner) = &f.ty {
                            let iu = inner.to_uppercase();
                            if iu == upper {
                                self.error(
                                    stmt.line,
                                    format!("TYPE '{}' cannot contain itself", name),
                                );
                                continue;
                            }
                            if !self.symbols.records.contains_key(&iu) {
                                self.error(
                                    stmt.line,
                                    format!("field '{}' uses undefined TYPE '{}'", f.name, inner),
                                );
                                continue;
                            }
                        }
                        let words = self.symbols.type_words(&f.ty);
                        info.fields.push((
                            fu,
                            FieldInfo {
                                word: info.words,
                                ty: f.ty.clone(),
                            },
                        ));
                        info.words += words;
                    }
                    self.symbols.records.insert(upper, info);
                }
                StmtKind::DimTyped {
                    name,
                    ty,
                    dimensions,
                } => {
                    if let TypeRef::Record(r) = ty {
                        if !self.symbols.records.contains_key(&r.to_uppercase()) {
                            self.error(stmt.line, format!("undefined TYPE '{}'", r));
                        }
                    }
                    let key = (scope.clone(), name.to_uppercase());
                    if self.symbols.typed_vars.contains_key(&key) {
                        self.error(stmt.line, format!("'{}' is already declared", name));
                    }
                    self.symbols.typed_vars.insert(key.clone(), ty.clone());
                    if let Some(dims) = dimensions {
                        self.symbols.arrays.insert(
                            key,
                            ArrayInfo {
                                rank: dims.len(),
                                line: stmt.line,
                            },
                        );
                    }
                }
                StmtKind::OptionBase(n) => {
                    if self.seen_option_base {
                        self.error(stmt.line, "OPTION BASE may appear only once");
                    } else if !self.symbols.arrays.is_empty() {
                        self.error(stmt.line, "OPTION BASE must come before any DIM");
                    }
                    self.seen_option_base = true;
                    self.symbols.option_base = *n;
                }
                StmtKind::Const { name, value } => {
                    let upper = name.to_uppercase();
                    if self.symbols.consts.contains_key(&upper) {
                        self.error(stmt.line, format!("'{}' is already defined", name));
                    }
                    match self.const_eval(value) {
                        Some(lit) => {
                            self.symbols.consts.insert(upper, lit);
                        }
                        None => self.error(
                            stmt.line,
                            format!("CONST '{}' must have a constant value", name),
                        ),
                    }
                }
                // REDIM may also introduce an array, so both are collected.
                StmtKind::Dim { arrays } | StmtKind::Redim { arrays, .. } => {
                    let is_redim = matches!(stmt.kind, StmtKind::Redim { .. });
                    for arr in arrays {
                        let key = (scope.clone(), arr.name.clone());
                        if let Some(prev) = self.symbols.arrays.get(&key) {
                            if !is_redim {
                                self.error(
                                    stmt.line,
                                    format!("array '{}' is already declared", arr.name),
                                );
                            } else if prev.rank != arr.dimensions.len() {
                                self.error(
                                    stmt.line,
                                    format!(
                                        "REDIM of '{}' must keep its {} dimension(s)",
                                        arr.name, prev.rank
                                    ),
                                );
                            }
                        }
                        self.symbols.arrays.insert(
                            key,
                            ArrayInfo {
                                rank: arr.dimensions.len(),
                                line: stmt.line,
                            },
                        );
                    }
                }
                StmtKind::Sub {
                    name, params, body, ..
                }
                | StmtKind::Function {
                    name, params, body, ..
                } => {
                    let is_function = matches!(stmt.kind, StmtKind::Function { .. });
                    if scope != &Scope::Module {
                        self.error(
                            stmt.line,
                            format!("'{}' cannot be defined inside another procedure", name),
                        );
                    }
                    if let Some(prev) = self.symbols.procs.get(name) {
                        let prev_line = prev.line;
                        self.error_with_note(
                            stmt.line,
                            format!("'{}' is already defined", name),
                            format!("previous definition is on line {}", prev_line),
                        );
                    }
                    self.symbols.procs.insert(
                        name.clone(),
                        ProcInfo {
                            is_function,
                            params: params.clone(),
                            line: stmt.line,
                        },
                    );
                    // A parameter with an AS clause is a typed variable inside
                    // the body, so field access on it resolves.
                    for p in params {
                        if let Some(ty) = &p.ty {
                            if let TypeRef::Record(r) = ty {
                                if !self.symbols.records.contains_key(&r.to_uppercase()) {
                                    self.error(
                                        stmt.line,
                                        format!(
                                            "parameter '{}' uses undefined TYPE '{}'",
                                            p.name, r
                                        ),
                                    );
                                }
                            }
                            self.symbols.typed_vars.insert(
                                (Scope::Proc(name.clone()), p.name.to_uppercase()),
                                ty.clone(),
                            );
                        }
                    }
                    self.collect(body, &Scope::Proc(name.clone()));
                }
                _ => {
                    for body in child_bodies(stmt) {
                        self.collect(body, scope);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Pass 2: check uses
    // ------------------------------------------------------------------

    fn check(&mut self, stmts: &[Stmt], scope: &Scope) {
        for stmt in stmts {
            self.check_stmt(stmt, scope);
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt, scope: &Scope) {
        let line = stmt.line;
        match &stmt.kind {
            StmtKind::Let {
                name,
                indices,
                value,
            } => {
                self.check_expr(value, scope, line);
                if let Some(idx) = indices {
                    self.check_array_use(name, idx.len(), scope, line);
                    for e in idx {
                        self.check_expr(e, scope, line);
                    }
                } else if let Some(target_ty) = self.symbols.typed_var(scope, name).cloned() {
                    if let TypeRef::Record(rname) = &target_ty {
                        // The source may be another record variable, or an
                        // element of an array of the same record type.
                        let src_name = match value {
                            Expr::Variable(src)
                            | Expr::ArrayAccess { name: src, .. }
                            | Expr::FnCall { name: src, .. } => Some(src),
                            _ => None,
                        };
                        let matches_type = src_name.is_some_and(|src| {
                            matches!(
                                self.symbols.typed_var(scope, src),
                                Some(TypeRef::Record(sname))
                                    if sname.to_uppercase() == rname.to_uppercase()
                            )
                        });
                        if !matches_type {
                            self.error(
                                line,
                                format!("'{}' can only be assigned another {} record", name, rname),
                            );
                        }
                    }
                } else {
                    self.check_assign_types(name, value, scope, line);
                }
            }
            StmtKind::FieldAssign { target, value } => {
                self.check_expr(value, scope, line);
                // Rebuild the access as an expression to reuse one checker.
                let mut e = Expr::Variable(target.name.clone());
                for f in &target.fields {
                    e = Expr::Field {
                        base: Box::new(e),
                        field: f.clone(),
                    };
                }
                if let Some(ty) = self.check_field_path(&e, scope, line) {
                    if matches!(ty, TypeRef::Record(_)) {
                        self.error(line, "cannot assign to a whole record; assign its fields");
                    }
                }
            }
            StmtKind::Call { name, args } => {
                self.check_call(name, args, scope, line, true);
            }
            StmtKind::Print { items, using, .. } => {
                self.check_using(using.as_ref(), line);
                for item in items {
                    if let PrintItem::Expr(e) = item {
                        self.check_expr(e, scope, line);
                    }
                }
            }
            StmtKind::PrintFile { items, using, .. } => {
                if using.is_some() {
                    self.error(line, "PRINT # USING is not supported");
                }
                for item in items {
                    if let PrintItem::Expr(e) = item {
                        self.check_expr(e, scope, line);
                    }
                }
            }
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.check_expr(condition, scope, line);
                self.check(then_branch, scope);
                if let Some(eb) = else_branch {
                    self.check(eb, scope);
                }
            }
            StmtKind::For {
                start,
                end,
                step,
                body,
                ..
            } => {
                self.check_expr(start, scope, line);
                self.check_expr(end, scope, line);
                if let Some(s) = step {
                    self.check_expr(s, scope, line);
                }
                self.loops.push(true);
                self.check(body, scope);
                self.loops.pop();
            }
            StmtKind::While { condition, body } => {
                self.check_expr(condition, scope, line);
                self.loops.push(false);
                self.check(body, scope);
                self.loops.pop();
            }
            StmtKind::DoLoop {
                condition, body, ..
            } => {
                if let Some(c) = condition {
                    self.check_expr(c, scope, line);
                }
                self.loops.push(false);
                self.check(body, scope);
                self.loops.pop();
            }
            StmtKind::SelectCase { expr, cases } => {
                self.check_expr(expr, scope, line);
                for (clauses, body) in cases {
                    for clause in clauses.iter().flatten() {
                        match clause {
                            CaseClause::Value(e) | CaseClause::Compare(_, e) => {
                                self.check_expr(e, scope, line)
                            }
                            CaseClause::Range(lo, hi) => {
                                self.check_expr(lo, scope, line);
                                self.check_expr(hi, scope, line);
                            }
                        }
                    }
                    self.check(body, scope);
                }
            }
            StmtKind::Goto(target) => self.check_target(target, line, "GOTO"),
            StmtKind::Gosub(target) => self.check_target(target, line, "GOSUB"),
            StmtKind::OnGoto { expr, targets } => {
                self.check_expr(expr, scope, line);
                for t in targets {
                    self.check_target(t, line, "ON ... GOTO");
                }
            }
            StmtKind::Restore(Some(target)) => self.check_target(target, line, "RESTORE"),
            StmtKind::Dim { arrays } => {
                for arr in arrays {
                    for d in &arr.dimensions {
                        self.check_expr(d, scope, line);
                    }
                }
            }
            StmtKind::Redim { arrays, preserve } => {
                for arr in arrays {
                    for d in &arr.dimensions {
                        self.check_expr(d, scope, line);
                    }
                    // Only the last dimension may change under PRESERVE, which
                    // is QuickBASIC's own rule: any other change would need the
                    // elements remapped rather than the block simply grown.
                    if *preserve && arr.dimensions.len() > 1 {
                        self.error(
                            line,
                            format!(
                                "REDIM PRESERVE of '{}' may only change its last dimension",
                                arr.name
                            ),
                        );
                    }
                }
            }
            StmtKind::Sub { name, body, .. } | StmtKind::Function { name, body, .. } => {
                let saved_loops = std::mem::take(&mut self.loops);
                let was_in_proc = std::mem::replace(&mut self.in_proc, true);
                self.check(body, &Scope::Proc(name.clone()));
                self.in_proc = was_in_proc;
                self.loops = saved_loops;
            }
            StmtKind::ExitLoop { is_for } => {
                if !self.loops.iter().any(|f| f == is_for) {
                    let what = if *is_for { "FOR" } else { "DO or WHILE" };
                    self.error(line, format!("EXIT outside of a {} loop", what));
                }
            }
            StmtKind::ExitProc => {
                if !self.in_proc {
                    self.error(line, "EXIT SUB/FUNCTION outside of a procedure");
                }
            }
            StmtKind::Swap(a, b) => {
                for t in [a, b] {
                    if let Some(indices) = &t.indices {
                        self.check_array_use(&t.name, indices.len(), scope, line);
                        for e in indices {
                            self.check_expr(e, scope, line);
                        }
                    }
                }
                if a.name.ends_with('$') != b.name.ends_with('$') {
                    self.error(line, "SWAP requires both values to be the same type");
                }
            }
            StmtKind::Open { filename, .. } => self.check_expr(filename, scope, line),
            _ => {}
        }
    }

    /// Fold a constant expression, or return None when it is not constant.
    ///
    /// Small on purpose: literals, references to earlier CONSTs, and the
    /// arithmetic that appears in real constant declarations.
    fn const_eval(&self, e: &Expr) -> Option<Literal> {
        match e {
            Expr::Literal(l) => Some(l.clone()),
            Expr::Variable(n) => self.symbols.consts.get(&n.to_uppercase()).cloned(),
            Expr::Unary { op, operand } => {
                let v = self.const_eval(operand)?;
                match (op, v) {
                    (UnaryOp::Neg, Literal::Integer(n)) => Some(Literal::Integer(-n)),
                    (UnaryOp::Neg, Literal::Float(f)) => Some(Literal::Float(-f)),
                    _ => None,
                }
            }
            Expr::Binary { op, left, right } => {
                let (l, r) = (self.const_eval(left)?, self.const_eval(right)?);
                let (a, b) = match (&l, &r) {
                    (Literal::Integer(a), Literal::Integer(b)) => {
                        // Integer arithmetic stays integer where it can.
                        return match op {
                            BinaryOp::Add => Some(Literal::Integer(a.checked_add(*b)?)),
                            BinaryOp::Sub => Some(Literal::Integer(a.checked_sub(*b)?)),
                            BinaryOp::Mul => Some(Literal::Integer(a.checked_mul(*b)?)),
                            BinaryOp::IntDiv if *b != 0 => Some(Literal::Integer(a / b)),
                            BinaryOp::Mod if *b != 0 => Some(Literal::Integer(a % b)),
                            BinaryOp::Div if *b != 0 => Some(Literal::Float(*a as f64 / *b as f64)),
                            _ => None,
                        };
                    }
                    (Literal::Integer(a), Literal::Float(b)) => (*a as f64, *b),
                    (Literal::Float(a), Literal::Integer(b)) => (*a, *b as f64),
                    (Literal::Float(a), Literal::Float(b)) => (*a, *b),
                    _ => return None,
                };
                match op {
                    BinaryOp::Add => Some(Literal::Float(a + b)),
                    BinaryOp::Sub => Some(Literal::Float(a - b)),
                    BinaryOp::Mul => Some(Literal::Float(a * b)),
                    BinaryOp::Div if b != 0.0 => Some(Literal::Float(a / b)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// A PRINT USING format must be a string literal, since it is parsed at
    /// compile time into a sequence of runtime calls.
    fn check_using(&mut self, using: Option<&Expr>, line: u32) {
        match using {
            None => {}
            Some(Expr::Literal(Literal::String(_))) => {}
            Some(_) => self.error(
                line,
                "PRINT USING requires a literal format string".to_string(),
            ),
        }
    }

    /// Check a branch target exists.
    fn check_target(&mut self, target: &GotoTarget, line: u32, what: &str) {
        match target {
            GotoTarget::Line(n) => {
                if !self.symbols.lines.contains(n) {
                    self.error(line, format!("{} target line {} does not exist", what, n));
                }
            }
            GotoTarget::Label(name) => {
                if !self.symbols.labels.contains(name) {
                    let known: Vec<&str> = self.symbols.labels.iter().map(|s| s.as_str()).collect();
                    match closest(name, &known) {
                        Some(sug) => self.error_with_note(
                            line,
                            format!("{} target '{}' is not defined", what, name),
                            format!("did you mean '{}'?", sug),
                        ),
                        None => {
                            self.error(line, format!("{} target '{}' is not defined", what, name))
                        }
                    }
                }
            }
        }
    }

    /// Check a call, either as a statement (`is_stmt`) or in an expression.
    fn check_call(&mut self, name: &str, args: &[Expr], scope: &Scope, line: u32, is_stmt: bool) {
        for a in args {
            self.check_expr(a, scope, line);
        }
        let upper = name.to_uppercase();

        // An array indexed in an expression parses as a call when the DIM was
        // not seen first, so resolve against arrays before reporting.
        if self.symbols.lookup_array(scope, &upper).is_some() {
            self.check_array_use(&upper, args.len(), scope, line);
            return;
        }

        if let Some(proc) = self.symbols.procs.get(&upper) {
            let (expected, is_function) = (proc.params.len(), proc.is_function);
            if is_stmt && is_function {
                self.error(
                    line,
                    format!(
                        "'{}' is a FUNCTION; its result cannot be discarded by calling it as a statement",
                        name
                    ),
                );
            } else if !is_stmt && !is_function {
                self.error(
                    line,
                    format!(
                        "'{}' is a SUB and has no value to use in an expression",
                        name
                    ),
                );
            }
            if expected != args.len() {
                self.error(
                    line,
                    format!(
                        "'{}' expects {} argument{}, but {} {} given",
                        name,
                        expected,
                        if expected == 1 { "" } else { "s" },
                        args.len(),
                        if args.len() == 1 { "was" } else { "were" }
                    ),
                );
            }
            return;
        }

        if let Some((_, min, max)) = builtin(&upper) {
            if is_stmt {
                self.error(
                    line,
                    format!("'{}' is a function and cannot be used as a statement", name),
                );
            } else if args.len() < *min || args.len() > *max {
                let expected = if min == max {
                    format!("{} argument{}", min, if *min == 1 { "" } else { "s" })
                } else {
                    format!("{} to {} arguments", min, max)
                };
                self.error(
                    line,
                    format!(
                        "'{}' expects {}, but {} {} given",
                        name,
                        expected,
                        args.len(),
                        if args.len() == 1 { "was" } else { "were" }
                    ),
                );
            }
            return;
        }

        // `NAME(...)` is ambiguous between a call and an array reference, so
        // say both rather than guessing which the user meant.
        let what = if is_stmt {
            "subroutine"
        } else if args.is_empty() {
            "function"
        } else {
            "function or array"
        };
        let mut known: Vec<&str> = self.symbols.procs.keys().map(|s| s.as_str()).collect();
        known.extend(BUILTINS.iter().map(|(n, _, _)| *n));
        known.extend(STATEMENT_KEYWORDS);
        match closest(&upper, &known) {
            Some(sug) => self.error_with_note(
                line,
                format!("unknown {} '{}'", what, name),
                format!("did you mean '{}'?", sug),
            ),
            None => self.error(line, format!("unknown {} '{}'", what, name)),
        }
    }

    /// Check an array is declared and indexed with the right number of
    /// subscripts.
    fn check_array_use(&mut self, name: &str, given: usize, scope: &Scope, line: u32) {
        let upper = name.to_uppercase();
        match self.symbols.lookup_array(scope, &upper) {
            Some(info) => {
                if info.rank != given {
                    let (rank, decl_line) = (info.rank, info.line);
                    self.error_with_note(
                        line,
                        format!(
                            "array '{}' has {} dimension{}, but {} subscript{} given",
                            name,
                            rank,
                            if rank == 1 { "" } else { "s" },
                            given,
                            if given == 1 { " was" } else { "s were" }
                        ),
                        format!("'{}' is declared on line {}", name, decl_line),
                    );
                }
            }
            None => {
                self.error_with_note(
                    line,
                    format!("array '{}' is used but never declared", name),
                    format!("add a DIM {}(...) before using it", name),
                );
            }
        }
    }

    /// Reject assigning a string to a numeric variable or vice versa. These
    /// used to reach codegen and abort it with a panic.
    fn check_assign_types(&mut self, name: &str, value: &Expr, scope: &Scope, line: u32) {
        let target_is_string = name.ends_with('$');
        if let Some(value_is_string) = self.expr_is_string(value, scope) {
            if target_is_string != value_is_string {
                let (from, to) = if value_is_string {
                    ("string", "numeric")
                } else {
                    ("numeric", "string")
                };
                self.error(
                    line,
                    format!(
                        "type mismatch: cannot assign {} value to {} variable '{}'",
                        from, to, name
                    ),
                );
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr, scope: &Scope, line: u32) {
        match expr {
            Expr::Literal(_) | Expr::Variable(_) => {}
            Expr::ArrayAccess { name, indices } => {
                self.check_array_use(name, indices.len(), scope, line);
                for i in indices {
                    self.check_expr(i, scope, line);
                }
            }
            Expr::Unary { operand, .. } => self.check_expr(operand, scope, line),
            Expr::Binary { op, left, right } => {
                self.check_expr(left, scope, line);
                self.check_expr(right, scope, line);
                self.check_binary_types(*op, left, right, scope, line);
            }
            Expr::FnCall { name, args } => {
                self.check_call(name, args, scope, line, false);
            }
            Expr::Field { .. } => {
                self.check_field_path(expr, scope, line);
            }
        }
    }

    /// Check that a field path names real fields of a real record type.
    ///
    /// Returns the field's type when the path resolves.
    fn check_field_path(&mut self, expr: &Expr, scope: &Scope, line: u32) -> Option<TypeRef> {
        let (name, fields) = flatten_field_path(expr)?;
        let Some(mut ty) = self.symbols.typed_var(scope, &name).cloned() else {
            self.error_with_note(
                line,
                format!("'{}' is not a record variable", name),
                format!("declare it with DIM {} AS <type>", name),
            );
            return None;
        };
        for field in &fields {
            let TypeRef::Record(rec) = &ty else {
                self.error(
                    line,
                    format!("'{}' has no fields; it is not a record", name),
                );
                return None;
            };
            let info = self.symbols.records.get(&rec.to_uppercase())?;
            match info.field(field) {
                Some(f) => ty = f.ty.clone(),
                None => {
                    let known: Vec<&str> = info.fields.iter().map(|(n, _)| n.as_str()).collect();
                    match closest(&field.to_uppercase(), &known) {
                        Some(sug) => self.error_with_note(
                            line,
                            format!("TYPE '{}' has no field '{}'", rec, field),
                            format!("did you mean '{}'?", sug),
                        ),
                        None => {
                            self.error(line, format!("TYPE '{}' has no field '{}'", rec, field))
                        }
                    }
                    return None;
                }
            }
        }
        Some(ty)
    }

    /// Reject mixing strings and numbers in an operator that cannot take them.
    fn check_binary_types(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        scope: &Scope,
        line: u32,
    ) {
        let (l, r) = (
            self.expr_is_string(left, scope),
            self.expr_is_string(right, scope),
        );
        let (Some(l), Some(r)) = (l, r) else { return };

        let comparison = matches!(
            op,
            BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge
        );

        if l != r {
            self.error(
                line,
                "type mismatch: cannot mix string and numeric operands".to_string(),
            );
            return;
        }

        // Both strings: only concatenation and comparison are defined.
        if l && !comparison && op != BinaryOp::Add {
            self.error(
                line,
                format!("operator {} cannot be applied to strings", op_name(op)),
            );
        }
    }

    /// Whether an expression yields a string, or `None` when unknown (an
    /// unresolved call, whose own diagnostic will already have been reported).
    fn expr_is_string(&self, expr: &Expr, scope: &Scope) -> Option<bool> {
        match expr {
            Expr::Literal(Literal::String(_)) => Some(true),
            Expr::Literal(_) => Some(false),
            Expr::Variable(name) => Some(name.ends_with('$')),
            Expr::ArrayAccess { name, .. } => Some(name.ends_with('$')),
            Expr::Unary { .. } => Some(false),
            Expr::Binary { op, left, .. } => {
                if matches!(
                    op,
                    BinaryOp::Eq
                        | BinaryOp::Ne
                        | BinaryOp::Lt
                        | BinaryOp::Gt
                        | BinaryOp::Le
                        | BinaryOp::Ge
                ) {
                    Some(false) // comparisons yield -1/0
                } else {
                    self.expr_is_string(left, scope)
                }
            }
            Expr::Field { .. } => {
                let (name, fields) = flatten_field_path(expr)?;
                let mut ty = self.symbols.typed_var(scope, &name).cloned()?;
                for field in &fields {
                    let TypeRef::Record(rec) = &ty else {
                        return None;
                    };
                    let info = self.symbols.records.get(&rec.to_uppercase())?;
                    ty = info.field(field)?.ty.clone();
                }
                Some(matches!(ty, TypeRef::FixedString(_)))
            }
            Expr::FnCall { name, .. } => {
                let upper = name.to_uppercase();
                if self.symbols.lookup_array(scope, &upper).is_some()
                    || self.symbols.procs.contains_key(&upper)
                    || builtin(&upper).is_some()
                {
                    Some(upper.ends_with('$'))
                } else {
                    None
                }
            }
        }
    }
}

fn op_name(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::IntDiv => "\\",
        BinaryOp::Mod => "MOD",
        BinaryOp::Pow => "^",
        BinaryOp::Eq => "=",
        BinaryOp::Ne => "<>",
        BinaryOp::Lt => "<",
        BinaryOp::Gt => ">",
        BinaryOp::Le => "<=",
        BinaryOp::Ge => ">=",
        BinaryOp::And => "AND",
        BinaryOp::Or => "OR",
        BinaryOp::Xor => "XOR",
    }
}

/// Split a field-access expression into its base variable and field path.
fn flatten_field_path(expr: &Expr) -> Option<(String, Vec<String>)> {
    let mut fields = Vec::new();
    let mut cur = expr;
    loop {
        match cur {
            Expr::Field { base, field } => {
                fields.push(field.clone());
                cur = base;
            }
            // `arr(i).f` resolves against the array's element type, so the
            // subscripts do not affect which field is named.
            Expr::Variable(name) | Expr::ArrayAccess { name, .. } | Expr::FnCall { name, .. } => {
                fields.reverse();
                return Some((name.clone(), fields));
            }
            _ => return None,
        }
    }
}

/// Statement keywords, so that a misspelling suggests the keyword rather than
/// only user-defined names.
const STATEMENT_KEYWORDS: &[&str] = &[
    "PRINT", "INPUT", "LET", "DIM", "IF", "FOR", "NEXT", "WHILE", "WEND", "GOTO", "GOSUB",
    "RETURN", "SUB", "FUNCTION", "SELECT", "CASE", "READ", "DATA", "RESTORE", "CLS", "OPEN",
    "CLOSE", "END", "STOP",
];

/// Nearest known name within a small edit distance, for "did you mean".
fn closest<'a>(name: &str, known: &[&'a str]) -> Option<&'a str> {
    // Allow a little more slack for longer names, but never so much that
    // unrelated identifiers match.
    let budget = match name.len() {
        0..=3 => 1,
        4..=6 => 2,
        _ => 3,
    };
    known
        .iter()
        .map(|k| (*k, edit_distance(name, k)))
        .filter(|(k, d)| *d <= budget && *k != name)
        .min_by_key(|(_, d)| *d)
        .map(|(k, _)| k)
}

/// Levenshtein distance.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}
