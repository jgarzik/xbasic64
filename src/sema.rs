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

/// Highest BASIC file number the runtime's handle table has a slot for.
///
/// The table is 16 pointers wide and slot 0 is the console, so a program may
/// use 1 through 15.
pub const MAX_FILE_NUM: i64 = 15;

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

/// Whether a bare identifier of this name is a call rather than a variable.
///
/// `TIMER` and `RND` are the only builtins that take no argument, so a bare
/// mention of either is a call. Without this they parsed as ordinary variable
/// reads and quietly returned the zero of a slot nobody ever wrote.
pub fn is_zero_arg_builtin(name: &str) -> bool {
    builtin(&name.to_uppercase()).is_some_and(|(_, min, _)| *min == 0)
}

fn builtin(name: &str) -> Option<&'static (&'static str, usize, usize)> {
    BUILTINS.iter().find(|(n, _, _)| *n == name)
}

/// A procedure declaration.
#[derive(Debug, Clone)]
pub struct ProcInfo {
    pub is_function: bool,
    pub params: Vec<Param>,
    /// Result type from `FUNCTION f AS T`; None means the name's suffix
    /// decides, which is also the only option for a SUB.
    pub ret_ty: Option<TypeRef>,
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

    /// Look up a typed variable declared in exactly `scope`, with no fallback.
    ///
    /// Codegen uses this to tell a procedure-local record from a module-level
    /// one it merely refers to, which decides frame slot versus `.bss`.
    pub fn typed_var_in(&self, scope: &Scope, name: &str) -> Option<&TypeRef> {
        self.typed_vars.get(&(scope.clone(), name.to_uppercase()))
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

    // Pass 1: collect declarations

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
                StmtKind::Dim { decls } | StmtKind::Redim { decls, .. } => {
                    let is_redim = matches!(stmt.kind, StmtKind::Redim { .. });
                    for decl in decls {
                        let key = (scope.clone(), decl.name.to_uppercase());

                        if let Some(ty) = &decl.ty {
                            if let TypeRef::Record(r) = ty {
                                if !self.symbols.records.contains_key(&r.to_uppercase()) {
                                    self.error(stmt.line, format!("undefined TYPE '{}'", r));
                                }
                            }
                            if self.symbols.typed_vars.contains_key(&key) && !is_redim {
                                self.error(
                                    stmt.line,
                                    format!("'{}' is already declared", decl.name),
                                );
                            }
                            self.symbols.typed_vars.insert(key.clone(), ty.clone());
                        }

                        let Some(dims) = &decl.dimensions else {
                            continue;
                        };
                        if dims.is_empty() {
                            self.error(
                                stmt.line,
                                format!("array '{}' needs at least one dimension", decl.name),
                            );
                        }
                        if let Some(prev) = self.symbols.arrays.get(&key) {
                            if !is_redim {
                                self.error(
                                    stmt.line,
                                    format!("array '{}' is already declared", decl.name),
                                );
                            } else if prev.rank != dims.len() {
                                self.error(
                                    stmt.line,
                                    format!(
                                        "REDIM of '{}' must keep its {} dimension(s)",
                                        decl.name, prev.rank
                                    ),
                                );
                            }
                        }
                        self.symbols.arrays.insert(
                            key,
                            ArrayInfo {
                                rank: dims.len(),
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
                    let ret_ty = match &stmt.kind {
                        StmtKind::Function { ret_ty, .. } => ret_ty.clone(),
                        _ => None,
                    };

                    // A declared result type and a type suffix must agree, or
                    // there is no telling which the program meant.
                    if let Some(ty) = &ret_ty {
                        let declared = DataType::from_type_ref(ty);
                        let suffixed = DataType::from_suffix(name);
                        if name.ends_with(|c| "%&!#$".contains(c)) && declared != suffixed {
                            self.error(
                                stmt.line,
                                format!(
                                    "'{}' is declared AS a different type than its name's suffix",
                                    name
                                ),
                            );
                        }
                    }
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
                            ret_ty,
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

    // Pass 2: check uses

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
                // A bare mention of these is a call, so letting one also name a
                // variable would make the write and the read mean different
                // things.
                if indices.is_none() && is_zero_arg_builtin(name) {
                    self.error(
                        line,
                        format!(
                            "'{}' is a built-in function and cannot be assigned to",
                            name.to_uppercase()
                        ),
                    );
                }
                if let Some(idx) = indices {
                    self.check_array_use(name, idx.len(), scope, line);
                    for e in idx {
                        self.check_expr(e, scope, line);
                        self.require_numeric(e, scope, line, "an array subscript");
                    }
                    // An array element is typed like a scalar of the same name.
                    self.check_assign_types(name, value, scope, line);
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
            StmtKind::Print {
                file_num,
                items,
                using,
                ..
            } => {
                if let Some(file_num) = file_num {
                    if using.is_some() {
                        self.error(line, "PRINT # USING is not supported");
                    }
                    self.check_file_num(file_num, scope, line);
                } else {
                    self.check_using(using.as_ref(), line);
                }
                for item in items {
                    if let PrintItem::Expr(e) = item {
                        self.check_expr(e, scope, line);
                        self.reject_record_value(e, scope, line);
                    }
                }
            }
            StmtKind::Input { file_num, vars, .. } => {
                if let Some(file_num) = file_num {
                    self.check_file_num(file_num, scope, line);
                }
                for var in vars {
                    self.check_lvalue(var, scope, line);
                }
            }
            StmtKind::LineInput { file_num, var, .. } => {
                if let Some(file_num) = file_num {
                    self.check_file_num(file_num, scope, line);
                }
                self.check_lvalue(var, scope, line);
                // Codegen reads a string and stores it as-is, so a numeric
                // target used to be handed a pointer to reinterpret as a
                // double.
                if self.expr_is_string(&lvalue_as_expr(var), scope) == Some(false) {
                    self.error(
                        line,
                        format!(
                            "LINE INPUT needs a string variable, but '{}' is numeric",
                            var.name
                        ),
                    );
                }
            }
            StmtKind::Read(vars) => {
                for var in vars {
                    self.check_lvalue(var, scope, line);
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
                var,
                start,
                end,
                step,
                body,
            } => {
                if var.ends_with('$') {
                    self.error(line, "a FOR control variable must be numeric");
                }
                self.check_expr(start, scope, line);
                self.require_numeric(start, scope, line, "a FOR start value");
                self.check_expr(end, scope, line);
                self.require_numeric(end, scope, line, "a FOR limit");
                if let Some(s) = step {
                    self.check_expr(s, scope, line);
                    self.require_numeric(s, scope, line, "a FOR step");
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
            StmtKind::Dim { decls } => {
                for decl in decls {
                    for d in decl.dimensions.iter().flatten() {
                        self.check_expr(d, scope, line);
                    }
                }
            }
            StmtKind::Redim { decls, preserve } => {
                for decl in decls {
                    let dims = decl.dimensions.as_deref().unwrap_or(&[]);
                    for d in dims {
                        self.check_expr(d, scope, line);
                    }
                    // Only the last dimension may change under PRESERVE, which
                    // is QuickBASIC's own rule: any other change would need the
                    // elements remapped rather than the block simply grown.
                    if *preserve && dims.len() > 1 {
                        self.error(
                            line,
                            format!(
                                "REDIM PRESERVE of '{}' may only change its last dimension",
                                decl.name
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
            StmtKind::Open {
                filename, file_num, ..
            } => {
                self.check_expr(filename, scope, line);
                if self.expr_is_string(filename, scope) == Some(false) {
                    self.error(line, "OPEN needs a string filename");
                }
                self.check_file_num(file_num, scope, line);
            }
            StmtKind::Close {
                file_num: Some(file_num),
            } => {
                self.check_file_num(file_num, scope, line);
            }
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

    /// Report a mismatched argument type for a builtin, if there is one.
    ///
    /// Codegen coerces builtin arguments without checking, so a string passed
    /// where a number belongs used to abort the compiler.
    fn builtin_arg_type_error(&self, name: &str, args: &[Expr], scope: &Scope) -> Option<String> {
        // LBOUND and UBOUND take an array *name*, whose element type says
        // nothing about the call -- `LBOUND(N$)` used to be rejected for
        // passing a string.
        if let ("LBOUND" | "UBOUND", Some(first)) = (name, args.first()) {
            let Expr::Variable(arr) = first else {
                return Some(format!("argument 1 of '{}' must be an array name", name));
            };
            let Some(info) = self.symbols.lookup_array(scope, &arr.to_uppercase()) else {
                return Some(format!("'{}' is not a declared array", arr));
            };
            let rank = info.rank;

            let dim = args.get(1)?;
            if self.expr_is_string(dim, scope) == Some(true) {
                return Some(format!("argument 2 of '{}' must be numeric", name));
            }
            // A constant dimension is checked here; a computed one is checked
            // at run time, where reading past the descriptor used to return
            // whatever happened to follow it.
            if let Some(Literal::Integer(n)) = self.const_eval(dim) {
                if n < 1 || n as usize > rank {
                    return Some(format!(
                        "'{}' has {} dimension{}, so dimension {} does not exist",
                        arr,
                        rank,
                        if rank == 1 { "" } else { "s" },
                        n
                    ));
                }
            }
            return None;
        }

        // Which arguments must be strings; every other one must be numeric.
        let string_args: &[usize] = match name {
            "LEN" | "ASC" | "VAL" | "LTRIM$" | "RTRIM$" | "UCASE$" | "LCASE$" => &[0],
            "LEFT$" | "RIGHT$" | "MID$" => &[0],
            // INSTR is either (haystack, needle) or (start, haystack, needle).
            "INSTR" if args.len() == 2 => &[0, 1],
            "INSTR" => &[1, 2],
            // STRING$ takes a count and either a code or a string.
            "STRING$" => &[],
            _ => &[],
        };
        // Builtins that take no string argument at all.
        let all_numeric = !matches!(
            name,
            "LEN"
                | "ASC"
                | "VAL"
                | "LEFT$"
                | "RIGHT$"
                | "MID$"
                | "INSTR"
                | "LTRIM$"
                | "RTRIM$"
                | "UCASE$"
                | "LCASE$"
                | "STRING$"
                | "LBOUND"
                | "UBOUND"
        );

        for (i, arg) in args.iter().enumerate() {
            let is_string = self.expr_is_string(arg, scope)?;
            let want_string = string_args.contains(&i);
            if all_numeric && is_string {
                return Some(format!("'{}' takes a numeric argument", name));
            }
            if !all_numeric && is_string != want_string && name != "STRING$" {
                return Some(format!(
                    "argument {} of '{}' must be {}",
                    i + 1,
                    name,
                    if want_string { "a string" } else { "numeric" }
                ));
            }
        }
        None
    }

    /// Reject a whole record where a scalar value is required.
    ///
    /// A record is legitimate as an assignment source or a record argument, so
    /// this is applied only at the sites that consume a value: PRINT items,
    /// operands, conditions, subscripts and loop bounds.
    fn reject_record_value(&mut self, e: &Expr, scope: &Scope, line: u32) {
        let name = match e {
            Expr::Variable(n) => n.clone(),
            Expr::Field { .. } => match flatten_field_path(e) {
                Some((base, fields)) => {
                    // Resolve the path; only a record-typed result is a problem.
                    let mut ty = match self.symbols.typed_var(scope, &base) {
                        Some(t) => t.clone(),
                        None => return,
                    };
                    for f in &fields {
                        let TypeRef::Record(rec) = &ty else { return };
                        let Some(info) = self.symbols.records.get(&rec.to_uppercase()) else {
                            return;
                        };
                        match info.field(f) {
                            Some(fi) => ty = fi.ty.clone(),
                            None => return,
                        }
                    }
                    if let TypeRef::Record(r) = ty {
                        self.error(
                            line,
                            format!("a whole {} record has no value; use one of its fields", r),
                        );
                    }
                    return;
                }
                None => return,
            },
            _ => return,
        };
        if let Some(TypeRef::Record(r)) = self.symbols.typed_var(scope, &name) {
            let r = r.clone();
            self.error(
                line,
                format!("a whole {} record has no value; use one of its fields", r),
            );
        }
    }

    /// Require an expression to be numeric.
    fn require_numeric(&mut self, e: &Expr, scope: &Scope, line: u32, what: &str) {
        if self.expr_is_string(e, scope) == Some(true) {
            self.error(line, format!("{} must be numeric, not a string", what));
        }
        self.reject_record_value(e, scope, line);
    }

    /// Check an assignment or input target.
    ///
    /// Rebuilds the access as an expression so the array, subscript and field
    /// checks that already exist do the work, then refuses a whole record --
    /// which has no value to read into any more than it has one to print.
    fn check_lvalue(&mut self, target: &LValue, scope: &Scope, line: u32) {
        let e = lvalue_as_expr(target);
        self.check_expr(&e, scope, line);
        self.reject_record_value(&e, scope, line);
    }

    /// A file number must be numeric, and within the runtime's handle table.
    ///
    /// The table holds slots 1-15; slot 0 is the console. Nothing checked the
    /// index, so a constant outside that range indexed off the end of the
    /// table at run time. A non-constant is checked by codegen instead.
    fn check_file_num(&mut self, e: &Expr, scope: &Scope, line: u32) {
        self.check_expr(e, scope, line);
        self.check_file_num_value(e, scope, line);
    }

    /// The range half of [`Self::check_file_num`], for a caller that has
    /// already walked the expression. Walking it twice reports anything wrong
    /// inside it twice.
    fn check_file_num_value(&mut self, e: &Expr, scope: &Scope, line: u32) {
        self.require_numeric(e, scope, line, "a file number");
        if let Some(n) = self.const_eval(e).and_then(|l| match l {
            Literal::Integer(n) => Some(n),
            Literal::Float(f) => Some(f as i64),
            Literal::String(_) => None,
        }) && !(1..=MAX_FILE_NUM).contains(&n)
        {
            self.error(
                line,
                format!("file number {} is not between 1 and {}", n, MAX_FILE_NUM),
            );
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
            // Each argument must match its parameter's type class. Collected
            // first so that reporting can borrow self mutably.
            let param_strings: Vec<bool> = proc
                .params
                .iter()
                .map(|p| match &p.ty {
                    Some(t) => matches!(t, TypeRef::FixedString(_)),
                    None => p.name.ends_with('$'),
                })
                .collect();
            let (expected, is_function) = (proc.params.len(), proc.is_function);

            // A record parameter takes the address of the caller's variable,
            // so the argument has to be a record lvalue of the same type --
            // anything else would hand the callee a value to dereference.
            let param_records: Vec<Option<String>> = proc
                .params
                .iter()
                .map(|p| match &p.ty {
                    Some(TypeRef::Record(r)) => Some(r.to_uppercase()),
                    _ => None,
                })
                .collect();

            for (i, arg) in args.iter().enumerate() {
                let Some(Some(want)) = param_records.get(i) else {
                    continue;
                };
                match self.record_type_of(arg, scope) {
                    Some(TypeRef::Record(got)) if got.to_uppercase() == *want => {}
                    Some(TypeRef::Record(got)) => self.error(
                        line,
                        format!(
                            "argument {} of '{}' is TYPE {}, but a TYPE {} value was given",
                            i + 1,
                            name,
                            want,
                            got
                        ),
                    ),
                    _ => self.error_with_note(
                        line,
                        format!("argument {} of '{}' must be TYPE {}", i + 1, name, want),
                        "pass a variable declared with DIM ... AS, or one of its record fields"
                            .to_string(),
                    ),
                }
            }

            for (i, arg) in args.iter().enumerate() {
                if matches!(param_records.get(i), Some(Some(_))) {
                    continue;
                }
                let Some(want_string) = param_strings.get(i).copied() else {
                    break;
                };
                if let Some(is_string) = self.expr_is_string(arg, scope) {
                    if is_string != want_string {
                        let (got, wanted) = if is_string {
                            ("string", "numeric")
                        } else {
                            ("numeric", "string")
                        };
                        self.error(
                            line,
                            format!(
                                "argument {} of '{}' is {}, but a {} value was given",
                                i + 1,
                                name,
                                wanted,
                                got
                            ),
                        );
                    }
                }
            }

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
            } else if let Some(bad) = self.builtin_arg_type_error(&upper, args, scope) {
                self.error(line, bad);
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
            } else if matches!(upper.as_str(), "EOF" | "LOF") {
                // check_expr has already walked the argument on the way here.
                self.check_file_num_value(&args[0], scope, line);
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
                    self.require_numeric(i, scope, line, "an array subscript");
                }
            }
            Expr::Unary { operand, .. } => self.check_expr(operand, scope, line),
            Expr::Binary { op, left, right } => {
                self.check_expr(left, scope, line);
                self.check_expr(right, scope, line);
                self.reject_record_value(left, scope, line);
                self.reject_record_value(right, scope, line);
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

    /// The record type an expression denotes, or None when it denotes no
    /// record at all.
    ///
    /// Unlike [`Self::check_field_path`] this reports nothing; it is used to
    /// decide whether an argument can be passed where a record is expected.
    fn record_type_of(&self, expr: &Expr, scope: &Scope) -> Option<TypeRef> {
        let (name, fields) = flatten_field_path(expr)?;
        let mut ty = self.symbols.typed_var(scope, &name).cloned()?;
        for field in &fields {
            let TypeRef::Record(rec) = &ty else {
                return None;
            };
            ty = self
                .symbols
                .records
                .get(&rec.to_uppercase())?
                .field(field)?
                .ty
                .clone();
        }
        matches!(ty, TypeRef::Record(_)).then_some(ty)
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
/// Rebuild an assignment target as the expression that reads it.
///
/// Lets one set of checks serve both sides: `A(I).X` as a target and as a
/// value are the same access, and only one of them had ever been checked.
fn lvalue_as_expr(target: &LValue) -> Expr {
    let mut e = match &target.indices {
        Some(indices) => Expr::ArrayAccess {
            name: target.name.clone(),
            indices: indices.clone(),
        },
        None => Expr::Variable(target.name.clone()),
    };
    for field in &target.fields {
        e = Expr::Field {
            base: Box::new(e),
            field: field.clone(),
        };
    }
    e
}

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
