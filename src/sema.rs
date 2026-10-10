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

use crate::lexer::normalized;
use crate::parser::*;
use std::collections::{HashMap, HashSet};

/// Highest BASIC file number the runtime's handle table has a slot for.
///
/// The table is 16 pointers wide and slot 0 is the console, so a program may
/// use 1 through 15.
/// The most variables one INPUT statement may read; the runtime's field table
/// (`MAX_INPUT_FIELDS` in input.s) holds this many.
pub const MAX_INPUT_VARS: usize = 64;

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
    ("CVD", 1, 1),
    ("CVI", 1, 1),
    ("CVL", 1, 1),
    ("CVS", 1, 1),
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
    ("LOC", 1, 1),
    ("LOF", 1, 1),
    ("LTRIM$", 1, 1),
    ("LOG", 1, 1),
    ("MID$", 2, 3),
    ("MKD$", 1, 1),
    ("MKI$", 1, 1),
    ("MKL$", 1, 1),
    ("MKS$", 1, 1),
    ("OCT$", 1, 1),
    ("POS", 1, 1),
    ("DATE$", 0, 0),
    ("TIME$", 0, 0),
    ("FRE", 1, 1),
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
    // Both take no argument at all, so a bare mention of either is a call --
    // see `is_zero_arg_builtin`, without which they would read as variables
    // nobody ever wrote.
    ("ERR", 0, 0),
    ("ERL", 0, 0),
    ("VAL", 1, 1),
];

/// GW-BASIC names xbasic64 does not implement, and what to say about each.
///
/// Without this table these names are not errors at all: an unrecognised name
/// is simply a new variable, so `PRINT DATE$` printed an empty string and
/// `ON ERROR GOTO 100` compiled into a computed GOTO on a variable that is
/// always zero -- falling through, silently, exactly where a program expected
/// its error handler to take over. A GW-BASIC program would compile clean and
/// then misbehave at run time, which is the worst outcome available.
///
/// The second field is the reason, phrased to say whether waiting will help.
/// LANGREF's "Not supported" section is the prose version of this table; the
/// two are meant to be read together.
const UNSUPPORTED: &[(&str, &str)] = &[
    // Planned: implementable on both platforms, not written yet.
    // Not "unimplemented": ERROR n works. It stays here so that every *other*
    // mention -- `PRINT ERROR`, `X = ERROR + 1` -- says why rather than
    // becoming a variable that reads as zero.
    (
        "ERROR",
        "ERROR is a statement, not a value; write ERROR n to raise error n",
    ),
    (
        "INKEY$",
        "INKEY$ needs raw console input, which is not implemented yet",
    ),
    ("INPUT$", "INPUT$ is not implemented yet"),
    ("CSRLIN", "console cursor position is not implemented yet"),
    ("VIEW", "console windowing is not implemented yet"),
    ("SLEEP", "SLEEP is not implemented yet"),
    ("SHELL", "SHELL is not implemented yet"),
    ("ENVIRON$", "ENVIRON$ is not implemented yet"),
    ("ENVIRON", "ENVIRON is not implemented yet"),
    ("KILL", "KILL is not implemented yet"),
    ("NAME", "NAME is not implemented yet"),
    ("FILES", "FILES is not implemented yet"),
    ("CHDIR", "CHDIR is not implemented yet"),
    ("MKDIR", "MKDIR is not implemented yet"),
    ("RMDIR", "RMDIR is not implemented yet"),
    // Refused for now with a reason, rather than a promise about the future.
    // Some of these describe a machine xbasic64 does not target -- the 8086's
    // memory map, its I/O ports, its display adapters -- and some are simply
    // not written yet.
    (
        "PEEK",
        "direct memory access has no meaning in a 64-bit hosted program",
    ),
    (
        "POKE",
        "direct memory access has no meaning in a 64-bit hosted program",
    ),
    (
        "VARPTR",
        "direct memory access has no meaning in a 64-bit hosted program",
    ),
    (
        "VARPTR$",
        "direct memory access has no meaning in a 64-bit hosted program",
    ),
    ("VARSEG", "segmented addressing does not exist on x86-64"),
    ("USR", "USR calls 8086 machine code; use a SUB instead"),
    ("INP", "port I/O is not available to a user-mode program"),
    ("OUT", "port I/O is not available to a user-mode program"),
    ("WAIT", "port I/O is not available to a user-mode program"),
    ("BLOAD", "memory images are tied to the 8086 memory map"),
    ("BSAVE", "memory images are tied to the 8086 memory map"),
    ("SCREEN", "graphics modes are not supported"),
    ("PSET", "graphics is not supported by this compiler"),
    ("PRESET", "graphics is not supported by this compiler"),
    ("CIRCLE", "graphics is not supported by this compiler"),
    ("PAINT", "graphics is not supported by this compiler"),
    ("DRAW", "graphics is not supported by this compiler"),
    ("PALETTE", "graphics is not supported by this compiler"),
    ("WINDOW", "graphics is not supported by this compiler"),
    ("PMAP", "graphics is not supported by this compiler"),
    ("POINT", "graphics is not supported by this compiler"),
    ("SOUND", "SOUND is not implemented yet"),
    ("PLAY", "PLAY is not implemented yet"),
    ("PEN", "light-pen input is not supported"),
    ("STICK", "joystick input is not supported"),
    ("STRIG", "joystick input is not supported"),
    ("KEY", "KEY is not implemented yet"),
    ("ERDEV", "device error codes are specific to MS-DOS drivers"),
    (
        "EXTERR",
        "extended error codes are specific to MS-DOS; use ERR and ERL",
    ),
    (
        "ERDEV$",
        "device error codes are specific to MS-DOS drivers",
    ),
    (
        "IOCTL",
        "device control strings are specific to MS-DOS drivers",
    ),
    (
        "IOCTL$",
        "device control strings are specific to MS-DOS drivers",
    ),
    ("MOTOR", "there is no cassette motor to control"),
    ("COM", "serial-port event trapping is not supported"),
    ("PCOPY", "video pages are not supported"),
    ("LPRINT", "LPRINT is not implemented yet"),
    ("LPOS", "LPOS is not implemented yet"),
    (
        "LCOPY",
        "LCOPY copies the screen to a printer, which is not supported",
    ),
    (
        "CHAIN",
        "a compiled program cannot load another program's code",
    ),
    ("COMMON", "COMMON passes variables to a CHAINed program"),
    ("RUN", "RUN is an interpreter command"),
    ("LOAD", "LOAD is an interpreter command"),
    ("SAVE", "SAVE is an interpreter command"),
    ("MERGE", "MERGE is an interpreter command"),
    ("LIST", "LIST is an interpreter command"),
    ("LLIST", "LLIST is an interpreter command"),
    ("DELETE", "DELETE is an interpreter command"),
    ("NEW", "NEW is an interpreter command"),
    ("EDIT", "EDIT is an interpreter command"),
    ("RENUM", "RENUM is an interpreter command"),
    ("AUTO", "AUTO is an interpreter command"),
    ("CONT", "CONT is an interpreter command"),
    ("TRON", "TRON is an interpreter command"),
    ("TROFF", "TROFF is an interpreter command"),
    ("CLEAR", "CLEAR sets interpreter memory limits"),
];

/// The reason a GW-BASIC name is unavailable, when it is one this compiler
/// knows about but does not implement.
pub fn unsupported_reason(name: &str) -> Option<&'static str> {
    let upper = name.to_uppercase();
    UNSUPPORTED
        .iter()
        .find(|(n, _)| *n == upper)
        .map(|(_, why)| *why)
}

/// Whether a bare identifier of this name is a call rather than a variable.
///
/// `TIMER` and `RND` are the only builtins that take no argument, so a bare
/// mention of either is a call. Without this they parsed as ordinary variable
/// reads and quietly returned the zero of a slot nobody ever wrote.
pub fn is_zero_arg_builtin(name: &str) -> bool {
    builtin(normalized(name)).is_some_and(|(_, min, _)| *min == 0)
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
    /// Some DIM or REDIM of it gives a lower bound (`1 TO 10`), so a
    /// subscript has to have the lower bound taken off. Under OPTION BASE 1
    /// every array has one too; codegen checks that itself.
    pub has_lower: bool,
    /// No DIM declares it: it was created by being used, as GW-BASIC does.
    pub implicit: bool,
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
    /// Named labels (`Retry:`) available as branch targets, and the scope
    /// each one is in.
    ///
    /// A branch may not leave its scope: module code and each procedure have
    /// their own frame, so code reached across the boundary would run with the
    /// wrong one. An `ON ERROR` handler and a `RESUME` target must be at
    /// module level.
    pub labels: HashMap<String, Scope>,
    /// Numeric line-number labels available as branch targets, likewise.
    pub lines: HashMap<u32, Scope>,
    /// CONST names and their folded values.
    pub consts: HashMap<String, Literal>,
    /// Lowest legal subscript, set by OPTION BASE. Defaults to 0.
    pub option_base: i64,
    /// User-defined TYPEs, by upper-case name.
    pub records: HashMap<String, RecordInfo>,
    /// Variables declared with `DIM ... AS`, by (scope, upper-case name).
    pub typed_vars: HashMap<(Scope, String), TypeRef>,
    /// Module-level scalars and arrays `DIM SHARED` makes visible inside every
    /// procedure.
    pub shared_all: NameSets,
    /// Per procedure, the module-level names its SHARED statements use.
    pub proc_shared: HashMap<String, NameSets>,
    /// Per procedure, the locals its STATIC statements keep between calls.
    pub proc_static: HashMap<String, NameSets>,
    /// Procedures declared `SUB name STATIC`, whose every local is static.
    pub static_procs: HashSet<String>,
    /// FUNCTIONs written as `DEF FN`, which see every module-level name.
    pub def_fns: HashSet<String>,
}

/// Upper-case names of scalars and of arrays, which BASIC keeps apart: `A`
/// and `A()` are different things.
#[derive(Debug, Default, Clone)]
pub struct NameSets {
    pub vars: HashSet<String>,
    pub arrays: HashSet<String>,
}

impl NameSets {
    fn insert(&mut self, name: &str, array: bool) {
        let upper = name.to_uppercase();
        if array {
            self.arrays.insert(upper);
        } else {
            self.vars.insert(upper);
        }
    }

    fn contains(&self, name: &str, array: bool) -> bool {
        let upper = name.to_uppercase();
        if array {
            self.arrays.contains(&upper)
        } else {
            self.vars.contains(&upper)
        }
    }
}

impl Symbols {
    /// Whether procedure `proc` uses the module-level `name` (an array when
    /// `array`) rather than a local of its own, as QuickBASIC decides it: the
    /// module DIMs it SHARED, or the procedure says SHARED. A DEF FN is part
    /// of the module and sees everything. A procedure used to see every name
    /// the module used.
    pub fn shares(&self, proc: &str, name: &str, array: bool) -> bool {
        self.def_fns.contains(&proc.to_uppercase())
            || self.shared_all.contains(name, array)
            || self
                .proc_shared
                .get(&proc.to_uppercase())
                .is_some_and(|s| s.contains(name, array))
    }

    /// Whether procedure `proc`'s own `name` keeps its value between calls.
    pub fn is_static(&self, proc: &str, name: &str, array: bool) -> bool {
        self.static_procs.contains(&proc.to_uppercase())
            || self
                .proc_static
                .get(&proc.to_uppercase())
                .is_some_and(|s| s.contains(name, array))
    }

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
                .get(normalized(name))
                .map(|r| r.words)
                .unwrap_or(1),
        }
    }

    /// Look up a variable's declared type, preferring the current scope.
    pub fn typed_var(&self, scope: &Scope, name: &str) -> Option<&TypeRef> {
        let upper = name.to_uppercase();
        if let Scope::Proc(p) = scope {
            if let Some(t) = self.typed_vars.get(&(scope.clone(), upper.clone())) {
                return Some(t);
            }
            if !self.shares(p, &upper, false) {
                return None;
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

    /// Look up an array visible from `scope`: a procedure's own, or a
    /// module-level one it shares.
    pub fn lookup_array(&self, scope: &Scope, name: &str) -> Option<&ArrayInfo> {
        if let Scope::Proc(p) = scope {
            if let Some(info) = self.arrays.get(&(scope.clone(), name.to_string())) {
                return Some(info);
            }
            if !self.shares(p, name, true) {
                return None;
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
    /// Worth saying, but the program is still compiled.
    pub warning: bool,
}

/// Analyze a program: collect its symbols, resolve `A(1)`, and report problems.
///
/// The program is taken by `&mut` because of the middle step. `A(1)` is either
/// an array element or a call, and only the finished symbol table can say
/// which. The parser used to guess from the DIM statements it had read so far,
/// which meant the same source produced a different AST depending on whether
/// its DIM came earlier or later in the file, and ignored scope entirely.
pub fn analyze(program: &mut Program, checks: bool) -> (Symbols, Vec<Diagnostic>) {
    apply_default_types(program);

    let mut a = Analyzer {
        checks,
        ..Default::default()
    };
    walk_stmts(&program.statements, &mut |stmt| {
        if let StmtKind::Erase(names) = &stmt.kind {
            a.erased.extend(names.iter().cloned());
        }
    });
    a.collect(&program.statements, &Scope::Module);
    a.check_name_collisions();
    a.check_return_has_a_gosub(&program.statements);
    a.check_resume_has_a_handler(&program.statements);
    a.resolve_array_accesses(&mut program.statements, &Scope::Module);
    a.check(&program.statements, &Scope::Module);
    (a.symbols, a.diagnostics)
}

/// Give every unsuffixed name the suffix its `DEF*` statement implies.
///
/// `DEFINT A-Z` makes `X` an INTEGER. Rather than teach the nineteen places that
/// ask "what type is this name" to consult a table -- `DataType::from_suffix` in
/// thirteen of them and `is_string_var` in six more -- this rewrites `X` to `X%`
/// once, up front, and every one of them then answers correctly without
/// changing. Two oracles disagreeing is exactly the bug shape that made
/// `LEN(S)` fail on a `DIM S AS STRING * 4`, and `DEFSTR` would have recreated
/// it precisely.
///
/// Merging is the correct reading, not a side effect: in GW-BASIC `DEFINT A`
/// makes `A` and `A%` the same variable, and rewriting the first to the second
/// is what makes them share storage here.
///
/// Two kinds of name are left alone. A builtin is not a variable, so `DEFSTR
/// A-Z` must not turn `LEN` into `LEN$`. A procedure is not one either, and its
/// name has to read the same at its definition and at every call, so both are
/// excluded together rather than half-rewritten.
fn apply_default_types(program: &mut Program) {
    let table = collect_def_types(&program.statements);
    if table.iter().all(|t| *t == DataType::Double) {
        return; // no DEF* in the program, or all of them redundant
    }
    let procs = procedure_names(&program.statements);
    rewrite_names(&mut program.statements, &table, &procs);
}

/// The default type for each initial letter, `A` first.
///
/// GW-BASIC applies a `DEF*` from where it appears onwards; this applies it to
/// the whole program. In practice these sit on a listing's first line, and a
/// name whose type changed halfway through a program would be a different
/// variable in every place that reads it.
fn collect_def_types(stmts: &[Stmt]) -> [DataType; 26] {
    let mut table = [DataType::Double; 26];
    walk_stmts(stmts, &mut |stmt| {
        if let StmtKind::DefType { ty, ranges } = &stmt.kind {
            for (first, last) in ranges {
                for c in *first..=*last {
                    table[(c as u8 - b'A') as usize] = *ty;
                }
            }
        }
    });
    table
}

/// Every SUB and FUNCTION name in the program, upper-cased.
fn procedure_names(stmts: &[Stmt]) -> HashSet<String> {
    let mut names = HashSet::new();
    walk_stmts(stmts, &mut |stmt| match &stmt.kind {
        StmtKind::Sub { name, .. } | StmtKind::Function { name, .. } => {
            names.insert(name.clone());
        }
        _ => {}
    });
    names
}

/// The suffix `name` should carry, or `None` to leave it alone.
fn defaulted(name: &str, table: &[DataType; 26], procs: &HashSet<String>) -> Option<String> {
    if name.ends_with(['%', '&', '!', '#', '$']) {
        return None; // an explicit suffix always wins
    }
    // A procedure, a builtin, and a GW-BASIC name this compiler refuses are
    // all names that do not denote a variable, so none of them takes a default
    // type. The third was missing: `DEFINT A-Z` renamed `ERROR` to `ERROR%`,
    // `unsupported_reason` looks the unsuffixed spelling up and missed, and
    // `ON ERROR GOTO 100` went back to compiling as a computed GOTO on a
    // variable that is always zero -- the exact silent fall-through the
    // UNSUPPORTED table exists to prevent.
    if procs.contains(name) || builtin(name).is_some() || unsupported_reason(name).is_some() {
        return None;
    }
    let first = name.chars().next()?;
    if !first.is_ascii_uppercase() {
        return None;
    }
    let suffix = match table[(first as u8 - b'A') as usize] {
        DataType::Integer => '%',
        DataType::Long => '&',
        DataType::Single => '!',
        DataType::String => '$',
        DataType::Double => return None, // already the default
    };
    Some(format!("{}{}", name, suffix))
}

/// Rewrite every name that denotes a variable or an array.
///
/// Record field names and TYPE names are deliberately absent: a field's type
/// comes from its own `AS` clause, and a TYPE name is not a value at all.
fn rewrite_names(stmts: &mut [Stmt], table: &[DataType; 26], procs: &HashSet<String>) {
    fn expr(e: &mut Expr, table: &[DataType; 26], procs: &HashSet<String>) {
        match e {
            Expr::Variable(name) | Expr::ArrayAccess { name, .. } | Expr::FnCall { name, .. } => {
                if let Some(renamed) = defaulted(name, table, procs) {
                    *name = renamed;
                }
            }
            _ => {}
        }
        match e {
            Expr::Unary { operand, .. } => expr(operand, table, procs),
            Expr::Binary { left, right, .. } => {
                expr(left, table, procs);
                expr(right, table, procs);
            }
            Expr::Field { base, .. } => expr(base, table, procs),
            Expr::ArrayAccess { indices, .. } => {
                indices.iter_mut().for_each(|i| expr(i, table, procs))
            }
            Expr::FnCall { args, .. } => args.iter_mut().for_each(|a| expr(a, table, procs)),
            Expr::Literal(_) | Expr::Variable(_) => {}
        }
    }

    fn lvalue(lv: &mut LValue, table: &[DataType; 26], procs: &HashSet<String>) {
        if let Some(renamed) = defaulted(&lv.name, table, procs) {
            lv.name = renamed;
        }
    }

    for stmt in stmts.iter_mut() {
        // Every expression the statement holds, including those inside the
        // LValues it owns -- `for_each_expr_mut` already visits both.
        for_each_expr_mut(stmt, &mut |e| expr(e, table, procs));

        // The names a statement carries outside an expression, including the
        // LValues `for_each_expr_mut` does not reach.
        //
        // Deliberately without a wildcard, like `for_each_expr_mut` and
        // `gen_stmt`: a statement kind that carries a name and is missing here
        // keeps the name the program wrote, while every other mention of it is
        // renamed, so the two stop matching and the statement silently does
        // nothing. `ERASE` did exactly that -- it sat in the wildcard, went on
        // naming `A` after `DEFINT A-Z` had made the array `A%`, and erased
        // nothing. A new variant is now a compile error until it says which it
        // is.
        match &mut stmt.kind {
            StmtKind::Let { name, .. } | StmtKind::Const { name, .. } => {
                if let Some(renamed) = defaulted(name, table, procs) {
                    *name = renamed;
                }
            }
            StmtKind::For { var, .. } => {
                if let Some(renamed) = defaulted(var, table, procs) {
                    *var = renamed;
                }
            }
            StmtKind::Dim { decls, .. } | StmtKind::Redim { decls, .. } => {
                for d in decls.iter_mut() {
                    // A declarator with an `AS` clause states its own type.
                    if d.ty.is_none()
                        && let Some(renamed) = defaulted(&d.name, table, procs)
                    {
                        d.name = renamed;
                    }
                }
            }
            StmtKind::Shared(items) | StmtKind::Static(items) => {
                for item in items.iter_mut() {
                    if item.ty.is_none()
                        && let Some(renamed) = defaulted(&item.name, table, procs)
                    {
                        item.name = renamed;
                    }
                }
            }
            StmtKind::Sub { params, .. }
            | StmtKind::Function { params, .. }
            | StmtKind::Declare { params, .. } => {
                for p in params.iter_mut() {
                    if p.ty.is_none()
                        && let Some(renamed) = defaulted(&p.name, table, procs)
                    {
                        p.name = renamed;
                    }
                }
            }
            StmtKind::Erase(names) => {
                for name in names.iter_mut() {
                    if let Some(renamed) = defaulted(name, table, procs) {
                        *name = renamed;
                    }
                }
            }
            // The operand is an expression, which `for_each_expr_mut` reaches;
            // there is no name of its own to rename.
            StmtKind::RaiseError(_) => {}
            // A handler is a branch target, and a label is not a variable --
            // the same reason GOTO and GOSUB are left alone here.
            StmtKind::OnError(_) | StmtKind::Resume(_) => {}
            StmtKind::Input { vars, .. } | StmtKind::Read(vars) => {
                vars.iter_mut().for_each(|v| lvalue(v, table, procs))
            }
            StmtKind::LineInput { var, .. } => lvalue(var, table, procs),
            StmtKind::Swap(a, b) => {
                lvalue(a, table, procs);
                lvalue(b, table, procs);
            }
            StmtKind::MidAssign { target, .. }
            | StmtKind::FieldAssign { target, .. }
            | StmtKind::SetField { target, .. } => lvalue(target, table, procs),
            StmtKind::Field { fields, .. } => fields
                .iter_mut()
                .for_each(|f| lvalue(&mut f.target, table, procs)),

            // Nothing outside an expression to rename. A procedure call names
            // a procedure, a GOTO names a label, and neither is a variable.
            StmtKind::Label(_)
            | StmtKind::LabelName(_)
            | StmtKind::Print { .. }
            | StmtKind::If { .. }
            | StmtKind::While { .. }
            | StmtKind::DoLoop { .. }
            | StmtKind::Goto(_)
            | StmtKind::Gosub(_)
            | StmtKind::Return(_)
            | StmtKind::OnGoto { .. }
            | StmtKind::OnGosub { .. }
            | StmtKind::Call { .. }
            | StmtKind::ExitLoop { .. }
            | StmtKind::ExitProc
            | StmtKind::OptionBase(_)
            | StmtKind::TypeDef { .. }
            | StmtKind::Data(_)
            | StmtKind::DefType { .. }
            | StmtKind::Beep
            | StmtKind::Locate { .. }
            | StmtKind::Color { .. }
            | StmtKind::Randomize(_)
            | StmtKind::Restore(_)
            | StmtKind::Cls
            | StmtKind::SelectCase { .. }
            | StmtKind::End
            | StmtKind::Stop
            | StmtKind::Open { .. }
            | StmtKind::Close { .. }
            | StmtKind::Width { .. }
            | StmtKind::GetPut { .. }
            | StmtKind::Lock { .. } => {}
        }

        // Nested bodies.
        match &mut stmt.kind {
            StmtKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                rewrite_names(then_branch, table, procs);
                if let Some(eb) = else_branch {
                    rewrite_names(eb, table, procs);
                }
            }
            StmtKind::SelectCase { cases, .. } => {
                for (_, body) in cases.iter_mut() {
                    rewrite_names(body, table, procs);
                }
            }
            StmtKind::For { body, .. }
            | StmtKind::While { body, .. }
            | StmtKind::DoLoop { body, .. }
            | StmtKind::Sub { body, .. }
            | StmtKind::Function { body, .. } => rewrite_names(body, table, procs),
            _ => {}
        }
    }
}

/// Call `f` for every statement in `stmts`, nested bodies included.
pub(crate) fn walk_stmts(stmts: &[Stmt], f: &mut impl FnMut(&Stmt)) {
    for stmt in stmts {
        f(stmt);
        for body in child_bodies(stmt) {
            walk_stmts(body, f);
        }
    }
}

/// Apply `f` to every expression directly held by `stmt`, in place.
///
/// Deliberately exhaustive, with no wildcard arm: this is the single place that
/// knows where a statement keeps its expressions, so a new `StmtKind` that
/// carries one must fail to compile here rather than be silently skipped.
/// Nested statement bodies are *not* visited -- the caller walks those itself,
/// because it has to change scope on the way in.
pub(crate) fn for_each_expr_mut(stmt: &mut Stmt, f: &mut impl FnMut(&mut Expr)) {
    /// Every expression an assignment target can hold: its subscripts.
    fn lvalue(lv: &mut LValue, f: &mut impl FnMut(&mut Expr)) {
        if let Some(indices) = &mut lv.indices {
            indices.iter_mut().for_each(&mut *f);
        }
    }

    match &mut stmt.kind {
        StmtKind::Let { indices, value, .. } => {
            if let Some(indices) = indices {
                indices.iter_mut().for_each(&mut *f);
            }
            f(value);
        }
        StmtKind::Print {
            file_num,
            items,
            using,
            ..
        } => {
            file_num.iter_mut().for_each(&mut *f);
            using.iter_mut().for_each(&mut *f);
            for item in items {
                if let PrintItem::Expr(e) = item {
                    f(e);
                }
            }
        }
        StmtKind::Input { vars, file_num, .. } => {
            file_num.iter_mut().for_each(&mut *f);
            vars.iter_mut().for_each(|v| lvalue(v, f));
        }
        StmtKind::LineInput { var, file_num, .. } => {
            file_num.iter_mut().for_each(&mut *f);
            lvalue(var, f);
        }
        StmtKind::If { condition, .. } => f(condition),
        StmtKind::For {
            start, end, step, ..
        } => {
            f(start);
            f(end);
            step.iter_mut().for_each(&mut *f);
        }
        StmtKind::While { condition, .. } => f(condition),
        StmtKind::DoLoop { condition, .. } => condition.iter_mut().for_each(&mut *f),
        StmtKind::OnGoto { expr, .. } | StmtKind::OnGosub { expr, .. } => f(expr),
        StmtKind::Dim { decls, .. } | StmtKind::Redim { decls, .. } => {
            for d in decls {
                if let Some(dims) = &mut d.dimensions {
                    dims.iter_mut().for_each(&mut *f);
                }
                d.lowers.iter_mut().flatten().for_each(&mut *f);
            }
        }
        StmtKind::Call { args, .. } => args.iter_mut().for_each(&mut *f),
        StmtKind::MidAssign {
            target,
            start,
            len,
            value,
        } => {
            lvalue(target, f);
            f(start);
            len.iter_mut().for_each(&mut *f);
            f(value);
        }
        StmtKind::Swap(a, b) => {
            lvalue(a, f);
            lvalue(b, f);
        }
        StmtKind::Locate { row, col } => {
            row.iter_mut().for_each(&mut *f);
            col.iter_mut().for_each(&mut *f);
        }
        StmtKind::Color { fg, bg } => {
            fg.iter_mut().for_each(&mut *f);
            bg.iter_mut().for_each(&mut *f);
        }
        StmtKind::Randomize(seed) => seed.iter_mut().for_each(&mut *f),
        StmtKind::RaiseError(e) => f(e),
        StmtKind::OnError(_) | StmtKind::Resume(_) => {}
        StmtKind::Const { value, .. } => f(value),
        StmtKind::FieldAssign { target, value } => {
            lvalue(target, f);
            f(value);
        }
        StmtKind::Read(vars) => vars.iter_mut().for_each(|v| lvalue(v, f)),
        StmtKind::SelectCase { expr, cases } => {
            f(expr);
            for (clauses, _) in cases {
                for clause in clauses.iter_mut().flatten() {
                    match clause {
                        CaseClause::Value(e) | CaseClause::Compare(_, e) => f(e),
                        CaseClause::Range(lo, hi) => {
                            f(lo);
                            f(hi);
                        }
                    }
                }
            }
        }
        StmtKind::Open {
            filename,
            file_num,
            reclen,
            ..
        } => {
            f(filename);
            f(file_num);
            reclen.iter_mut().for_each(&mut *f);
        }
        StmtKind::Close(file_nums) => file_nums.iter_mut().for_each(&mut *f),
        StmtKind::Width { file_num, width } => {
            file_num.iter_mut().for_each(&mut *f);
            f(width);
        }
        StmtKind::Field { file_num, fields } => {
            f(file_num);
            for slice in fields {
                f(&mut slice.width);
                lvalue(&mut slice.target, f);
            }
        }
        StmtKind::SetField { target, value, .. } => {
            lvalue(target, f);
            f(value);
        }
        StmtKind::GetPut {
            file_num, record, ..
        } => {
            f(file_num);
            record.iter_mut().for_each(&mut *f);
        }
        StmtKind::Lock {
            file_num, range, ..
        } => {
            f(file_num);
            if let Some((start, end)) = range {
                f(start);
                end.iter_mut().for_each(&mut *f);
            }
        }
        // Statements that hold no expression of their own. Listed rather than
        // matched with a wildcard so that a new variant carrying one is a
        // compile error here.
        StmtKind::Label(_)
        | StmtKind::LabelName(_)
        | StmtKind::Goto(_)
        | StmtKind::Gosub(_)
        | StmtKind::Return(_)
        | StmtKind::Sub { .. }
        | StmtKind::Function { .. }
        | StmtKind::ExitLoop { .. }
        | StmtKind::ExitProc
        | StmtKind::OptionBase(_)
        | StmtKind::TypeDef { .. }
        | StmtKind::DefType { .. }
        | StmtKind::Beep
        | StmtKind::Erase(_)
        | StmtKind::Data(_)
        | StmtKind::Shared(_)
        | StmtKind::Static(_)
        | StmtKind::Declare { .. }
        | StmtKind::Restore(_)
        | StmtKind::Cls
        | StmtKind::End
        | StmtKind::Stop => {}
    }
}

#[derive(Default)]
struct Analyzer {
    symbols: Symbols,
    /// Arrays the program ERASEs somewhere, which may therefore be DIMed again.
    erased: HashSet<String>,
    diagnostics: Vec<Diagnostic>,
    /// Enclosing loops, innermost last: true for FOR, false for WHILE/DO.
    loops: Vec<bool>,
    /// Whether the walk is currently inside a procedure body.
    in_proc: bool,
    /// Whether runtime checks are compiled in (false under `--unsafe`).
    checks: bool,
    /// Whether an OPTION BASE has already been seen.
    seen_option_base: bool,
    /// Set by `const_eval` when a fold failed because the value overflowed its
    /// type, so the caller can say so rather than "not constant".
    fold_overflowed: std::cell::Cell<bool>,
    /// Array parameters, (procedure, name), that the body never subscripts,
    /// so whose number of dimensions no call can be checked against.
    unranked_array_params: HashSet<(String, String)>,
}

impl Analyzer {
    fn error(&mut self, line: u32, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            line,
            message: message.into(),
            note: None,
            warning: false,
        });
    }

    fn error_with_note(&mut self, line: u32, message: impl Into<String>, note: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            line,
            message: message.into(),
            note: Some(note.into()),
            warning: false,
        });
    }

    fn warning_with_note(
        &mut self,
        line: u32,
        message: impl Into<String>,
        note: impl Into<String>,
    ) {
        self.diagnostics.push(Diagnostic {
            line,
            message: message.into(),
            note: Some(note.into()),
            warning: true,
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
                    if self.symbols.lines.insert(*n, scope.clone()).is_some() {
                        self.error(stmt.line, format!("duplicate line number {}", n));
                    }
                }
                StmtKind::LabelName(name) => {
                    if self
                        .symbols
                        .labels
                        .insert(name.clone(), scope.clone())
                        .is_some()
                    {
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
                    self.fold_overflowed.set(false);
                    match self.const_eval(value) {
                        Some(lit) => {
                            self.symbols.consts.insert(upper, lit);
                        }
                        None if self.fold_overflowed.get() => self.error(
                            stmt.line,
                            format!("Overflow: CONST '{}' does not fit its type", name),
                        ),
                        None => self.error(
                            stmt.line,
                            format!("CONST '{}' must have a constant value", name),
                        ),
                    }
                }
                StmtKind::Shared(items) | StmtKind::Static(items) => {
                    let what = if matches!(stmt.kind, StmtKind::Shared(_)) {
                        "SHARED"
                    } else {
                        "STATIC"
                    };
                    let Scope::Proc(proc) = scope else {
                        self.error(
                            stmt.line,
                            format!("{} belongs inside a SUB or FUNCTION", what),
                        );
                        continue;
                    };
                    let sets = if what == "SHARED" {
                        self.symbols
                            .proc_shared
                            .entry(proc.to_uppercase())
                            .or_default()
                    } else {
                        self.symbols
                            .proc_static
                            .entry(proc.to_uppercase())
                            .or_default()
                    };
                    for item in items {
                        sets.insert(&item.name, item.array);
                    }
                    // A STATIC name with a type is a typed local like DIM AS.
                    if what == "STATIC" {
                        for item in items {
                            if let (Some(ty), false) = (&item.ty, item.array) {
                                self.symbols
                                    .typed_vars
                                    .insert((scope.clone(), item.name.to_uppercase()), ty.clone());
                            }
                        }
                    }
                }
                // REDIM may also introduce an array, so both are collected.
                StmtKind::Dim { decls, .. } | StmtKind::Redim { decls, .. } => {
                    let is_redim = matches!(stmt.kind, StmtKind::Redim { .. });
                    if let StmtKind::Dim { shared: true, .. } = &stmt.kind {
                        if scope == &Scope::Module {
                            for decl in decls {
                                self.symbols
                                    .shared_all
                                    .insert(&decl.name, decl.dimensions.is_some());
                            }
                        } else {
                            self.error(
                                stmt.line,
                                "DIM SHARED belongs at module level; inside a procedure, use SHARED",
                            );
                        }
                    }
                    for decl in decls {
                        let key = (scope.clone(), decl.name.to_uppercase());

                        if let Some(ty) = &decl.ty {
                            // A declared type and a type suffix must agree, or
                            // there is no telling which the program meant. The
                            // same check has always guarded a FUNCTION's result
                            // type; DIM accepted the contradiction silently.
                            if decl.name.ends_with(|c| "%&!#$".contains(c))
                                && !matches!(ty, TypeRef::Record(_))
                                && DataType::from_type_ref(ty) != DataType::from_suffix(&decl.name)
                            {
                                self.error(
                                    stmt.line,
                                    format!(
                                        "'{}' is declared AS a different type than its name's suffix",
                                        decl.name
                                    ),
                                );
                            }
                            if let TypeRef::Record(r) = ty
                                && !self.symbols.records.contains_key(normalized(r))
                            {
                                self.error(stmt.line, format!("undefined TYPE '{}'", r));
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
                            if !is_redim && !self.erased.contains(&key.1) {
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
                        let has_lower = decl.lowers.iter().any(Option::is_some)
                            || self.symbols.arrays.get(&key).is_some_and(|a| a.has_lower);
                        self.symbols.arrays.insert(
                            key,
                            ArrayInfo {
                                rank: dims.len(),
                                line: stmt.line,
                                has_lower,
                                implicit: false,
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
                    if matches!(
                        stmt.kind,
                        StmtKind::Sub {
                            is_static: true,
                            ..
                        } | StmtKind::Function {
                            is_static: true,
                            ..
                        }
                    ) {
                        self.symbols.static_procs.insert(name.to_uppercase());
                    }
                    if matches!(stmt.kind, StmtKind::Function { def_fn: true, .. }) {
                        self.symbols.def_fns.insert(name.to_uppercase());
                    }
                    // An array parameter is an array of the procedure's own,
                    // as many dimensions as the body subscripts it with -- the
                    // declaration (`A()`) does not say. Its bounds are the
                    // caller's, so it may have a lower bound.
                    for p in params.iter().filter(|p| p.array) {
                        let rank = subscript_count(body, &p.name);
                        self.symbols.arrays.insert(
                            (Scope::Proc(name.clone()), p.name.to_uppercase()),
                            ArrayInfo {
                                rank: rank.unwrap_or(1),
                                line: stmt.line,
                                has_lower: true,
                                implicit: false,
                            },
                        );
                        if rank.is_none() {
                            self.unranked_array_params
                                .insert((name.to_uppercase(), p.name.to_uppercase()));
                        }
                    }
                    // A parameter with an AS clause is a typed variable inside
                    // the body, so field access on it resolves.
                    for p in params.iter().filter(|p| !p.array) {
                        if let Some(ty) = &p.ty {
                            if let TypeRef::Record(r) = ty
                                && !self.symbols.records.contains_key(normalized(r))
                            {
                                self.error(
                                    stmt.line,
                                    format!("parameter '{}' uses undefined TYPE '{}'", p.name, r),
                                );
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

    /// Refuse a `RETURN` in a program that contains no `GOSUB`.
    ///
    /// codegen defines the GOSUB return stack only when it has seen a GOSUB, so
    /// a lone RETURN emitted a reference to `_gosub_sp` that nothing defined and
    /// the user was handed `ld: undefined reference to _gosub_sp`. Reaching the
    /// linker with a name the compiler invented is exactly what this pass exists
    /// to prevent.
    ///
    /// The check is deliberately whole-program rather than per-path: deciding
    /// whether a particular RETURN can be reached from a particular GOSUB needs
    /// the control flow, and GOTO makes that undecidable. "No GOSUB at all" is
    /// sound, and it is the shape a mistake actually takes.
    fn check_return_has_a_gosub(&mut self, stmts: &[Stmt]) {
        let mut has_gosub = false;
        let mut first_return = None;
        walk_stmts(stmts, &mut |stmt| match &stmt.kind {
            StmtKind::Gosub(_) | StmtKind::OnGosub { .. } => has_gosub = true,
            StmtKind::Return(_) if first_return.is_none() => first_return = Some(stmt.line),
            _ => {}
        });
        if let (false, Some(line)) = (has_gosub, first_return) {
            self.error_with_note(
                line,
                "RETURN without GOSUB".to_string(),
                "RETURN ends a GOSUB; use EXIT SUB or EXIT FUNCTION to leave a procedure"
                    .to_string(),
            );
        }
    }

    /// RESUME needs an ON ERROR to return from.
    ///
    /// Modelled on `check_return_has_a_gosub`, and for the same reason: on its
    /// own a RESUME compiles to a jump through a table that a non-trapping
    /// program never emits, so the failure would be a link error rather than a
    /// diagnosis.
    fn check_resume_has_a_handler(&mut self, stmts: &[Stmt]) {
        let mut has_handler = false;
        let mut first_resume = None;
        walk_stmts(stmts, &mut |stmt| match &stmt.kind {
            StmtKind::OnError(_) => has_handler = true,
            StmtKind::Resume(_) if first_resume.is_none() => first_resume = Some(stmt.line),
            _ => {}
        });
        if let (false, Some(line)) = (has_handler, first_resume) {
            self.error_with_note(
                line,
                "RESUME without an error handler".to_string(),
                "RESUME ends an ON ERROR handler; this program has no ON ERROR".to_string(),
            );
        }
    }

    /// Refuse a name declared as an array and also as a procedure or builtin.
    ///
    /// `A(1)` reaches the compiler as one shape and has to become one thing.
    /// Sema resolved such a clash in favour of the array and codegen in favour
    /// of the procedure, and the parser's DIM-order heuristic hid the
    /// disagreement for as long as it lasted. `DIM F(5)` alongside
    /// `FUNCTION F(X)` compiled silently; so did `DIM LEN(5)`, where codegen's
    /// builtin table would answer first and the array would never be read.
    ///
    /// Rather than pick an order and document it, refuse the program: nobody
    /// writes this on purpose, and either resolution surprises somebody.
    fn check_name_collisions(&mut self) {
        let arrays: Vec<(Scope, String, u32)> = self
            .symbols
            .arrays
            .iter()
            .map(|((scope, name), info)| (scope.clone(), name.clone(), info.line))
            .collect();

        for (_, name, line) in arrays {
            if let Some(proc_info) = self.symbols.procs.get(&name) {
                let kind = if proc_info.is_function {
                    "FUNCTION"
                } else {
                    "SUB"
                };
                let proc_line = proc_info.line;
                self.error_with_note(
                    line,
                    format!("'{}' is declared both as an array and as a {}", name, kind),
                    format!("the {} is on line {}", kind, proc_line),
                );
            } else if builtin(&name).is_some() {
                self.error(
                    line,
                    format!(
                        "'{}' is the name of a built-in function and cannot also be an array",
                        name
                    ),
                );
            }
        }
    }

    // Pass 2: resolve `A(1)` against the symbol table

    /// Turn every `FnCall` naming an array into an `ArrayAccess`.
    ///
    /// The parser emits `FnCall` for all of `name(args)`, because at that point
    /// nothing knows whether `name` is an array, a procedure or a builtin. Here
    /// the symbol table is complete and scoped, so the question has one answer
    /// regardless of where the `DIM` was written.
    ///
    /// Doing it as a rewrite keeps `ArrayAccess` in the AST that codegen sees,
    /// which matters for more than tidiness: `expr_is_call_free` treats every
    /// `FnCall` as opaque, so leaving array reads as calls would silently
    /// disable FOR-counter promotion, and `walk_array_uses` collects names only
    /// from `ArrayAccess`, so loop-invariant descriptor hoisting would stop
    /// firing. Both would have been invisible except as lost performance.
    fn resolve_array_accesses(&mut self, stmts: &mut [Stmt], scope: &Scope) {
        for stmt in stmts {
            for_each_expr_mut(stmt, &mut |e| Self::resolve_expr(&self.symbols, e, scope));

            // Nested bodies, entering procedure scope where there is one.
            match &mut stmt.kind {
                StmtKind::Sub { name, body, .. } | StmtKind::Function { name, body, .. } => {
                    // `clone`, not `to_uppercase`: the lexer already uppercased
                    // it, and `collect` and `check` build this scope the same
                    // way -- a scope key that normalized differently from
                    // theirs would simply fail to match.
                    let inner = Scope::Proc(name.clone());
                    self.resolve_array_accesses(body, &inner);
                }
                StmtKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.resolve_array_accesses(then_branch, scope);
                    if let Some(eb) = else_branch {
                        self.resolve_array_accesses(eb, scope);
                    }
                }
                StmtKind::SelectCase { cases, .. } => {
                    for (_, body) in cases {
                        self.resolve_array_accesses(body, scope);
                    }
                }
                StmtKind::For { body, .. }
                | StmtKind::While { body, .. }
                | StmtKind::DoLoop { body, .. } => self.resolve_array_accesses(body, scope),
                _ => {}
            }
        }
    }

    /// Rewrite one expression tree, innermost first.
    fn resolve_expr(symbols: &Symbols, expr: &mut Expr, scope: &Scope) {
        match expr {
            Expr::Literal(_) | Expr::Variable(_) => return,
            Expr::Unary { operand, .. } => Self::resolve_expr(symbols, operand, scope),
            Expr::Binary { left, right, .. } => {
                Self::resolve_expr(symbols, left, scope);
                Self::resolve_expr(symbols, right, scope);
            }
            Expr::Field { base, .. } => Self::resolve_expr(symbols, base, scope),
            Expr::ArrayAccess { indices, .. } => {
                for i in indices {
                    Self::resolve_expr(symbols, i, scope);
                }
            }
            Expr::FnCall { args, .. } => {
                for a in args.iter_mut() {
                    Self::resolve_expr(symbols, a, scope);
                }
            }
        }

        // A procedure wins over an array of the same name, matching what
        // codegen has always done; `check_name_collisions` refuses the program
        // that makes the two disagree, so the order is unobservable.
        if let Expr::FnCall { name, args } = expr {
            let upper = normalized(name);
            if !symbols.procs.contains_key(upper) && symbols.lookup_array(scope, upper).is_some() {
                *expr = Expr::ArrayAccess {
                    name: std::mem::take(name),
                    indices: std::mem::take(args),
                };
            }
        }
    }

    // Pass 3: check uses

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
                // `DATE$ = "01-01-2026"` is a GW-BASIC statement, not the
                // creation of a variable called DATE$.
                if let Some(why) = unsupported_reason(name) {
                    self.error_with_note(
                        line,
                        format!("'{}' is not supported", name.to_uppercase()),
                        why.to_string(),
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
                if let Some(ty) = self.check_field_path(&e, scope, line)
                    && matches!(ty, TypeRef::Record(_))
                {
                    self.error(line, "cannot assign to a whole record; assign its fields");
                }
            }
            StmtKind::Call { name, args } => {
                self.check_call(name, args, scope, line, true);
            }
            StmtKind::Declare {
                name,
                is_function,
                params,
                ret_ty,
            } => self.check_declare(name, *is_function, params, ret_ty.as_ref(), scope, line),
            StmtKind::Print {
                file_num,
                items,
                using,
                ..
            } => {
                if let Some(file_num) = file_num {
                    self.check_file_num(file_num, scope, line);
                }
                // The format is read at run time, so any string will do.
                if let Some(fmt) = using {
                    self.check_expr(fmt, scope, line);
                    if self.expr_is_string(fmt, scope) == Some(false) {
                        self.error(line, "PRINT USING needs a string format");
                    }
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
                // The console form checks a whole line against every variable
                // before storing any, in a table of this many fields.
                if vars.len() > MAX_INPUT_VARS {
                    self.error(
                        line,
                        format!("INPUT reads at most {} variables at once", MAX_INPUT_VARS),
                    );
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
            StmtKind::Goto(target) => self.check_target(target, scope, line, "GOTO"),
            StmtKind::Gosub(target) => self.check_target(target, scope, line, "GOSUB"),
            StmtKind::Return(Some(target)) => self.check_target(target, scope, line, "RETURN"),
            StmtKind::OnGoto { expr, targets } => {
                self.check_expr(expr, scope, line);
                for t in targets {
                    self.check_target(t, scope, line, "ON ... GOTO");
                }
            }
            StmtKind::OnGosub { expr, targets } => {
                self.check_expr(expr, scope, line);
                for t in targets {
                    self.check_target(t, scope, line, "ON ... GOSUB");
                }
            }
            StmtKind::Restore(Some(target)) => self.check_restore_target(target, line),
            StmtKind::Dim { decls, .. } => {
                for decl in decls {
                    for d in decl.dimensions.iter().flatten() {
                        self.check_expr(d, scope, line);
                    }
                    for d in decl.lowers.iter().flatten() {
                        self.check_expr(d, scope, line);
                    }
                }
            }
            StmtKind::Redim { decls, preserve } => {
                for decl in decls {
                    let dims = decl.dimensions.as_deref().unwrap_or(&[]);
                    for d in dims.iter().chain(decl.lowers.iter().flatten()) {
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
                // Compare what the operands *are*, not the suffix of the
                // variable they hang off: for `P.N` the base is a record and
                // carries no suffix, so a string field and a numeric one both
                // looked non-string and the mismatch reached codegen, which
                // stored a string pointer into an INTEGER slot and crashed.
                let a_str = self.expr_is_string(&lvalue_as_expr(a), scope);
                let b_str = self.expr_is_string(&lvalue_as_expr(b), scope);
                if a_str.is_some() && b_str.is_some() && a_str != b_str {
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
            StmtKind::Close(file_nums) => {
                for file_num in file_nums {
                    self.check_file_num(file_num, scope, line);
                }
            }
            StmtKind::Width { file_num, width } => {
                if let Some(file_num) = file_num {
                    self.check_file_num(file_num, scope, line);
                }
                self.check_expr(width, scope, line);
                self.require_numeric(width, scope, line, "a width");
            }
            StmtKind::Field { file_num, fields } => {
                self.check_file_num(file_num, scope, line);
                for f in fields {
                    self.check_expr(&f.width, scope, line);
                    // A field names a slice of the record buffer, so only a
                    // string variable can hold one.
                    if !f.target.name.ends_with('$') {
                        self.error(
                            line,
                            format!("FIELD target '{}' must be a string variable", f.target.name),
                        );
                    }
                    if let Some(indices) = &f.target.indices {
                        self.check_array_use(&f.target.name, indices.len(), scope, line);
                        for e in indices {
                            self.check_expr(e, scope, line);
                        }
                    }
                }
            }
            StmtKind::SetField {
                target,
                value,
                right,
            } => {
                self.check_expr(value, scope, line);
                let word = if *right { "RSET" } else { "LSET" };
                if !target.name.ends_with('$') {
                    self.error(
                        line,
                        format!("{} needs a string variable, not '{}'", word, target.name),
                    );
                }
                if self.expr_is_string(value, scope) == Some(false) {
                    self.error(line, format!("{} needs a string value", word));
                }
                if let Some(indices) = &target.indices {
                    self.check_array_use(&target.name, indices.len(), scope, line);
                    for e in indices {
                        self.check_expr(e, scope, line);
                    }
                }
            }
            StmtKind::GetPut {
                file_num, record, ..
            } => {
                self.check_file_num(file_num, scope, line);
                if let Some(r) = record {
                    self.check_expr(r, scope, line);
                }
            }
            StmtKind::Lock {
                file_num, range, ..
            } => {
                self.check_file_num(file_num, scope, line);
                if let Some((start, end)) = range {
                    self.check_expr(start, scope, line);
                    if let Some(e) = end {
                        self.check_expr(e, scope, line);
                    }
                }
            }
            // Codegen skips an ERASE name it cannot resolve, on the grounds
            // that sema has already complained. It had not: this arm did not
            // exist, so `ERASE TOTLA` for `ERASE TOTAL` compiled clean and
            // erased nothing. The lookup is the one every array use gets --
            // the enclosing procedure first, then the module.
            StmtKind::RaiseError(n) => self.check_numeric_operand(n, scope, line, "ERROR"),
            StmtKind::Resume(target) => {
                // A trapped error unwinds to main's frame, so the handler --
                // and the RESUME that ends it -- are module-level code.
                if let Scope::Proc(name) = scope {
                    self.error_with_note(
                        line,
                        format!("RESUME is not allowed inside '{}'", name),
                        "RESUME ends an ON ERROR handler, which is module-level code".to_string(),
                    );
                }
                if let ResumeTarget::At(t) = target {
                    self.check_target(t, scope, line, "RESUME");
                    let at_module_scope = match t {
                        GotoTarget::Line(n) => self.symbols.lines.get(n) == Some(&Scope::Module),
                        GotoTarget::Label(name) => {
                            self.symbols.labels.get(name) == Some(&Scope::Module)
                        }
                    };
                    if !at_module_scope {
                        self.error_with_note(
                            line,
                            "a RESUME target must be at module level".to_string(),
                            "it is reached after the error has unwound out of every procedure"
                                .to_string(),
                        );
                    }
                }
            }
            StmtKind::OnError(target) => {
                // `--unsafe` removes the checks that raise most of what a
                // handler exists to catch, so the pair would leave a handler
                // that looks right and never runs -- the silent wrong answer
                // this compiler refuses everywhere else.
                if !self.checks {
                    self.error_with_note(
                        line,
                        "ON ERROR cannot be used with --unsafe".to_string(),
                        "--unsafe removes the runtime checks the handler would trap".to_string(),
                    );
                }
                // A trapped error unwinds to main's frame before it jumps, so
                // both the statement and its handler have to belong to main.
                if let Scope::Proc(name) = scope {
                    self.error_with_note(
                        line,
                        format!("ON ERROR is not allowed inside '{}'", name),
                        "a trapped error unwinds to module level, so the handler belongs there"
                            .to_string(),
                    );
                }
                let Some(target) = target else {
                    return; // GOTO 0 names no line to check
                };
                self.check_target(target, scope, line, "ON ERROR GOTO");
                let at_module_scope = match target {
                    GotoTarget::Line(n) => self.symbols.lines.get(n) == Some(&Scope::Module),
                    GotoTarget::Label(name) => {
                        self.symbols.labels.get(name) == Some(&Scope::Module)
                    }
                };
                if !at_module_scope {
                    self.error_with_note(
                        line,
                        "an ON ERROR handler must be at module level".to_string(),
                        "the handler is entered after the error unwinds out of every procedure"
                            .to_string(),
                    );
                }
            }
            StmtKind::Locate { row, col } => {
                for e in row.iter().chain(col.iter()) {
                    self.check_numeric_operand(e, scope, line, "LOCATE");
                }
            }
            StmtKind::Color { fg, bg } => {
                for e in fg.iter().chain(bg.iter()) {
                    self.check_numeric_operand(e, scope, line, "COLOR");
                }
            }
            StmtKind::Randomize(seed) => {
                for e in seed.iter() {
                    self.check_numeric_operand(e, scope, line, "RANDOMIZE");
                }
            }
            StmtKind::Erase(names) => {
                for name in names {
                    if self.symbols.lookup_array(scope, name).is_none() {
                        self.error(
                            line,
                            format!("ERASE needs an array, and '{name}' is not a declared array"),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// Fold a constant expression, or return None when it is not constant.
    ///
    /// Small on purpose: literals, references to earlier CONSTs, and the
    /// arithmetic that appears in real constant declarations. It folds in the
    /// types the program's own arithmetic uses, so `300 * 300` is an INTEGER
    /// product and overflows, as it would at run time -- the fold used to
    /// widen it to 90000. An overflow sets `fold_overflowed` and yields None.
    fn const_eval(&self, e: &Expr) -> Option<Literal> {
        let overflow = || {
            self.fold_overflowed.set(true);
            None
        };
        match e {
            Expr::Literal(l) => Some(l.clone()),
            Expr::Variable(n) => self.symbols.consts.get(normalized(n)).cloned(),
            Expr::Unary {
                op: UnaryOp::Neg,
                operand,
            } => match self.const_eval(operand)? {
                // Typed by size, so -32768 comes back an INTEGER.
                Literal::Integer(n) => Some(Literal::Integer(-n)),
                Literal::Long(n) if n == i64::from(i32::MIN) => overflow(),
                Literal::Long(n) => Some(Literal::Long(-n)),
                Literal::Single(f) => Some(Literal::Single(-f)),
                Literal::Float(f) => Some(Literal::Float(-f)),
                Literal::String(_) => None,
            },
            Expr::Binary { op, left, right } => {
                let (l, r) = (self.const_eval(left)?, self.const_eval(right)?);
                let (lt, rt) = (l.numeric_type()?, r.numeric_type()?);
                let (a, b) = (l.as_f64()?, r.as_f64()?);
                let int_of = |x: &Literal| match x {
                    Literal::Integer(n) | Literal::Long(n) => Some(*n),
                    _ => None,
                };
                match op {
                    BinaryOp::Div => {
                        if b == 0.0 {
                            return None;
                        }
                        let q = a / b;
                        if q.is_finite() {
                            Some(Literal::Float(q))
                        } else {
                            overflow()
                        }
                    }
                    // `\` and MOD convert their operands to LONG, rounding.
                    BinaryOp::IntDiv | BinaryOp::Mod => {
                        let round = |x: f64| {
                            let r = x.round_ties_even();
                            (r >= f64::from(i32::MIN) && r <= f64::from(i32::MAX))
                                .then_some(r as i64)
                        };
                        let (Some(x), Some(y)) = (round(a), round(b)) else {
                            return overflow();
                        };
                        if y == 0 {
                            return None;
                        }
                        let v = if *op == BinaryOp::IntDiv {
                            x / y
                        } else {
                            x % y
                        };
                        if i32::try_from(v).is_err() {
                            return overflow();
                        }
                        Some(Literal::Long(v))
                    }
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                        let ty = [lt, rt]
                            .into_iter()
                            .max_by_key(|t| match t {
                                DataType::Integer => 0,
                                DataType::Long => 1,
                                DataType::Single => 2,
                                _ => 3,
                            })
                            .expect("two types");
                        match ty {
                            DataType::Integer | DataType::Long => {
                                let (x, y) = (int_of(&l)?, int_of(&r)?);
                                let v = match op {
                                    BinaryOp::Add => x + y,
                                    BinaryOp::Sub => x - y,
                                    _ => x * y,
                                };
                                if ty == DataType::Integer {
                                    if i16::try_from(v).is_err() {
                                        return overflow();
                                    }
                                    Some(Literal::Integer(v))
                                } else {
                                    if i32::try_from(v).is_err() {
                                        return overflow();
                                    }
                                    Some(Literal::Long(v))
                                }
                            }
                            _ => {
                                let v = match op {
                                    BinaryOp::Add => a + b,
                                    BinaryOp::Sub => a - b,
                                    _ => a * b,
                                };
                                if ty == DataType::Single {
                                    let v = v as f32;
                                    if !v.is_finite() {
                                        return overflow();
                                    }
                                    Some(Literal::Single(f64::from(v)))
                                } else if v.is_finite() {
                                    Some(Literal::Float(v))
                                } else {
                                    overflow()
                                }
                            }
                        }
                    }
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
            let Some(info) = self.symbols.lookup_array(scope, normalized(arr)) else {
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
            if let Some(Literal::Integer(n) | Literal::Long(n)) = self.const_eval(dim)
                && (n < 1 || n as usize > rank)
            {
                return Some(format!(
                    "'{}' has {} dimension{}, so dimension {} does not exist",
                    arr,
                    rank,
                    if rank == 1 { "" } else { "s" },
                    n
                ));
            }
            return None;
        }

        // Which arguments must be strings; every other one must be numeric.
        let string_args: &[usize] = match name {
            "LEN" | "ASC" | "VAL" | "LTRIM$" | "RTRIM$" | "UCASE$" | "LCASE$" => &[0],
            "LEFT$" | "RIGHT$" | "MID$" => &[0],
            // The CV* family decodes the bytes MK*$ produced.
            "CVI" | "CVL" | "CVS" | "CVD" => &[0],
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
                | "CVI"
                | "CVL"
                | "CVS"
                | "CVD"
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
                        let Some(info) = self.symbols.records.get(normalized(rec)) else {
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
        if let Some(n) = self
            .const_eval(e)
            .and_then(|l| l.as_f64())
            .map(|f| f.round_ties_even() as i64)
            && !(1..=MAX_FILE_NUM).contains(&n)
        {
            self.error(
                line,
                format!("file number {} is not between 1 and {}", n, MAX_FILE_NUM),
            );
        }
    }

    /// Check a branch target exists, and is in the scope branching to it.
    fn check_target(&mut self, target: &GotoTarget, scope: &Scope, line: u32, what: &str) {
        let (found, shown) = match target {
            GotoTarget::Line(n) => (self.symbols.lines.get(n).cloned(), format!("line {}", n)),
            GotoTarget::Label(name) => (
                self.symbols.labels.get(name).cloned(),
                format!("'{}'", name),
            ),
        };
        match found {
            Some(at) if &at == scope => {}
            Some(at) => {
                let place = |s: &Scope| match s {
                    Scope::Module => "module-level code".to_string(),
                    Scope::Proc(p) => format!("'{}'", p),
                };
                self.error_with_note(
                    line,
                    format!("{} target {} is in another scope", what, shown),
                    format!(
                        "it is in {}, and this is in {}; a branch cannot leave a SUB or FUNCTION",
                        place(&at),
                        place(scope)
                    ),
                );
            }
            None => match target {
                GotoTarget::Line(n) => {
                    self.error(line, format!("{} target line {} does not exist", what, n));
                }
                GotoTarget::Label(name) => {
                    let known: Vec<&str> = self.symbols.labels.keys().map(|s| s.as_str()).collect();
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
            },
        }
    }

    /// Check a RESTORE target exists. It names a place in the DATA, which is
    /// one list for the whole program, so it may be in any scope.
    fn check_restore_target(&mut self, target: &GotoTarget, line: u32) {
        let exists = match target {
            GotoTarget::Line(n) => self.symbols.lines.contains_key(n),
            GotoTarget::Label(name) => self.symbols.labels.contains_key(name),
        };
        if !exists {
            self.check_target(target, &Scope::Module, line, "RESTORE");
        }
    }

    /// Check a call, either as a statement (`is_stmt`) or in an expression.
    fn check_call(&mut self, name: &str, args: &[Expr], scope: &Scope, line: u32, is_stmt: bool) {
        let upper = name.to_uppercase();
        // An argument for an array parameter names a whole array, `A()`, and
        // is checked as one below rather than as an element reference.
        let array_params: Vec<bool> = self
            .symbols
            .procs
            .get(&upper)
            .map(|p| p.params.iter().map(|q| q.array).collect())
            .unwrap_or_default();
        for (i, a) in args.iter().enumerate() {
            if array_params.get(i) != Some(&true) {
                self.check_expr(a, scope, line);
            }
        }

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
            // An array parameter takes an array, written `A()` or `A`, with
            // the dimensions the procedure uses it with.
            let param_arrays: Vec<Option<(String, String)>> = proc
                .params
                .iter()
                .map(|p| p.array.then(|| (p.name.clone(), name.to_uppercase())))
                .collect();
            // A by-reference parameter given a variable shares that variable,
            // so the two must be the same type -- QuickBASIC's "Parameter type
            // mismatch". Anything else is evaluated into a temporary.
            let param_types: Vec<Option<DataType>> = proc
                .params
                .iter()
                .map(|p| {
                    (!p.byval && !p.array && !matches!(p.ty, Some(TypeRef::Record(_)))).then(|| {
                        match &p.ty {
                            Some(t) => DataType::from_type_ref(t),
                            None => DataType::from_suffix(&p.name),
                        }
                    })
                })
                .collect();

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
                if let Some(Some((pname, pproc))) = param_arrays.get(i) {
                    self.check_array_argument(arg, pname, pproc, scope, line, i + 1, name);
                    continue;
                }
                if let Some(Some(want)) = param_types.get(i)
                    && let Some(got) = self.lvalue_type(arg, scope)
                    && got != *want
                {
                    self.error_with_note(
                        line,
                        format!(
                            "argument {} of '{}' is passed by reference, so it must be {}, not {}",
                            i + 1,
                            name,
                            type_name(*want),
                            type_name(got)
                        ),
                        "pass an expression instead -- CINT(X), X + 0 -- or declare the parameter BYVAL"
                            .to_string(),
                    );
                }
            }

            for (i, arg) in args.iter().enumerate() {
                if matches!(param_records.get(i), Some(Some(_)))
                    || matches!(param_arrays.get(i), Some(Some(_)))
                {
                    continue;
                }
                let Some(want_string) = param_strings.get(i).copied() else {
                    break;
                };
                if let Some(is_string) = self.expr_is_string(arg, scope)
                    && is_string != want_string
                {
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

        // A GW-BASIC name this compiler knows but does not provide is worth
        // saying so about, rather than offering a spelling suggestion for a
        // word that is spelled perfectly well.
        if let Some(why) = unsupported_reason(&upper) {
            self.error_with_note(
                line,
                format!("'{}' is not supported", name.to_uppercase()),
                why.to_string(),
            );
            return;
        }

        // In an expression, `NAME(...)` that is neither a procedure nor a
        // builtin is an array -- created by this use if no DIM declares it,
        // as GW-BASIC does.
        if !is_stmt && !args.is_empty() {
            self.check_array_use(&upper, args.len(), scope, line);
            return;
        }

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

    /// A DECLARE must match the procedure it declares: the same kind, the
    /// same parameters -- each of the same type, passing and arrayness -- and
    /// the same result type.
    fn check_declare(
        &mut self,
        name: &str,
        is_function: bool,
        params: &[Param],
        ret_ty: Option<&TypeRef>,
        scope: &Scope,
        line: u32,
    ) {
        if scope != &Scope::Module {
            self.error(line, "DECLARE belongs at module level");
            return;
        }
        let Some(proc) = self.symbols.procs.get(&name.to_uppercase()) else {
            self.error(line, format!("'{}' is declared but never defined", name));
            return;
        };
        let def_line = proc.line;
        let mut problems = Vec::new();
        if proc.is_function != is_function {
            problems.push(format!(
                "it is defined as a {}",
                if proc.is_function { "FUNCTION" } else { "SUB" }
            ));
        }
        if proc.params.len() != params.len() {
            problems.push(format!(
                "it is defined with {} parameter{}",
                proc.params.len(),
                if proc.params.len() == 1 { "" } else { "s" }
            ));
        } else {
            let ty = |p: &Param| match &p.ty {
                Some(t) => DataType::from_type_ref(t),
                None => DataType::from_suffix(&p.name),
            };
            for (i, (d, p)) in params.iter().zip(&proc.params).enumerate() {
                if ty(d) != ty(p) || d.byval != p.byval || d.array != p.array {
                    problems.push(format!("parameter {} differs from its definition", i + 1));
                }
            }
        }
        let declared = ret_ty.map(DataType::from_type_ref);
        let defined = proc.ret_ty.as_ref().map(DataType::from_type_ref);
        if is_function && proc.is_function && declared.is_some() && declared != defined {
            problems.push("its result type differs".to_string());
        }
        for p in problems {
            self.error_with_note(
                line,
                format!("DECLARE of '{}' does not match: {}", name, p),
                format!("'{}' is defined on line {}", name, def_line),
            );
        }
    }

    /// The type of `arg` when it is a variable, array element or field --
    /// something a by-reference parameter would share -- or None for any
    /// other expression.
    fn lvalue_type(&self, arg: &Expr, scope: &Scope) -> Option<DataType> {
        match arg {
            Expr::Variable(n) => {
                if self.symbols.consts.contains_key(normalized(n)) || is_zero_arg_builtin(n) {
                    return None;
                }
                Some(match self.symbols.typed_var(scope, n) {
                    Some(TypeRef::Record(_)) => return None,
                    Some(t) => DataType::from_type_ref(t),
                    None => DataType::from_suffix(n),
                })
            }
            Expr::ArrayAccess { name, .. } => Some(DataType::from_suffix(name)),
            Expr::FnCall { name, args }
                if !args.is_empty()
                    && self
                        .symbols
                        .lookup_array(scope, &name.to_uppercase())
                        .is_some() =>
            {
                Some(DataType::from_suffix(name))
            }
            _ => None,
        }
    }

    /// Check an argument for array parameter `pname` of `proc`: it must name
    /// an array, as `A()` or `A`, used with the same number of dimensions.
    #[allow(clippy::too_many_arguments)]
    fn check_array_argument(
        &mut self,
        arg: &Expr,
        pname: &str,
        proc: &str,
        scope: &Scope,
        line: u32,
        position: usize,
        callee: &str,
    ) {
        let given = match arg {
            Expr::Variable(n) => Some(n.to_uppercase()),
            Expr::FnCall { name, args }
            | Expr::ArrayAccess {
                name,
                indices: args,
            } if args.is_empty() => Some(name.to_uppercase()),
            _ => None,
        };
        let Some(arr) = given else {
            self.error(
                line,
                format!(
                    "argument {} of '{}' must be an array, written {}()",
                    position, callee, pname
                ),
            );
            return;
        };
        // An array argument the caller never declared is created, as a use
        // of it would be.
        let want = self
            .symbols
            .arrays
            .get(&(Scope::Proc(proc.to_string()), pname.to_uppercase()))
            .map(|a| a.rank);
        if self.symbols.lookup_array(scope, &arr).is_none() {
            self.check_array_use(&arr, want.unwrap_or(1), scope, line);
        }
        let got = self.symbols.lookup_array(scope, &arr).map(|a| a.rank);
        if DataType::from_suffix(&arr) != DataType::from_suffix(pname) {
            self.error(
                line,
                format!(
                    "argument {} of '{}' is an array of the wrong type for {}()",
                    position, callee, pname
                ),
            );
        }
        let checkable = !self
            .unranked_array_params
            .contains(&(proc.to_string(), pname.to_uppercase()));
        if checkable
            && let (Some(want), Some(got)) = (want, got)
            && want != got
        {
            self.error(
                line,
                format!(
                    "argument {} of '{}' has {} dimension{}, but {}() is used with {}",
                    position,
                    callee,
                    got,
                    if got == 1 { "" } else { "s" },
                    pname,
                    want
                ),
            );
        }
    }

    /// Check an array is declared and indexed with the right number of
    /// subscripts.
    fn check_array_use(&mut self, name: &str, given: usize, scope: &Scope, line: u32) {
        let upper = name.to_uppercase();
        match self.symbols.lookup_array(scope, &upper) {
            Some(info) => {
                if info.rank != given {
                    let (rank, decl_line, implicit) = (info.rank, info.line, info.implicit);
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
                        if implicit {
                            format!(
                                "'{}' was created by its first use, on line {}",
                                name, decl_line
                            )
                        } else {
                            format!("'{}' is declared on line {}", name, decl_line)
                        },
                    );
                }
            }
            // An array no DIM declares is created by its first use, with as
            // many dimensions as that use has subscripts, each 0 (or the
            // OPTION BASE) to 10 -- GW-BASIC's rule. It belongs to the scope
            // that uses it. This used to be refused.
            //
            // So a mistyped function -- SQRT(4) -- is quietly an array, as it
            // is in GW-BASIC. A name close to a function or procedure is worth
            // a warning, though not a refusal.
            None => {
                let mut known: Vec<&str> = self.symbols.procs.keys().map(|s| s.as_str()).collect();
                known.extend(BUILTINS.iter().map(|(n, _, _)| *n));
                // Short names are left alone: any one-letter array is "close"
                // to any one-letter procedure.
                if upper.trim_end_matches(['$', '%', '&', '!', '#']).len() >= 3
                    && let Some(sug) = closest(&upper, &known).map(str::to_string)
                {
                    self.warning_with_note(
                        line,
                        format!(
                            "'{}' is not a function, so it is an array, created by this use",
                            name
                        ),
                        format!("did you mean '{}'?", sug),
                    );
                }
                self.symbols.arrays.insert(
                    (scope.clone(), upper),
                    ArrayInfo {
                        rank: given,
                        line,
                        has_lower: false,
                        implicit: true,
                    },
                );
            }
        }
    }

    /// Reject assigning a string to a numeric variable or vice versa. These
    /// used to reach codegen and abort it with a panic.
    fn check_assign_types(&mut self, name: &str, value: &Expr, scope: &Scope, line: u32) {
        let target_is_string = name.ends_with('$');
        if let Some(value_is_string) = self.expr_is_string(value, scope)
            && target_is_string != value_is_string
        {
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

    /// Check an expression a statement will use as a number.
    ///
    /// Two jobs, and both were missing for every statement that takes a bare
    /// numeric operand. Without `check_expr` the operand's names are never
    /// resolved, so `LOCATE NoSuchFn(1), 1` reached the codegen line that
    /// says "sema checked the array is declared" -- it had not -- and panicked.
    /// Without the string test, `LOCATE A$, 1` reached the implicit-conversion
    /// panic instead. A builtin's arguments were always checked (`SQR(A$)`
    /// says so properly); a statement's were not.
    fn check_numeric_operand(&mut self, expr: &Expr, scope: &Scope, line: u32, what: &str) {
        self.check_expr(expr, scope, line);
        if self.expr_is_string(expr, scope) == Some(true) {
            self.error(line, format!("{} needs a number, not a string", what));
        }
    }

    fn check_expr(&mut self, expr: &Expr, scope: &Scope, line: u32) {
        match expr {
            Expr::Literal(_) => {}
            // An unrecognised name is normally just a new variable, which is
            // what makes an unimplemented GW-BASIC name so dangerous here: it
            // would read as an empty string or a zero forever. Naming it is the
            // whole point of the check.
            Expr::Variable(name) => {
                if let Some(why) = unsupported_reason(name) {
                    self.error_with_note(
                        line,
                        format!("'{}' is not supported", name.to_uppercase()),
                        why.to_string(),
                    );
                }
            }
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
                .get(normalized(rec))?
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
            let info = self.symbols.records.get(normalized(rec))?;
            match info.field(field) {
                Some(f) => ty = f.ty.clone(),
                None => {
                    let known: Vec<&str> = info.fields.iter().map(|(n, _)| n.as_str()).collect();
                    match closest(normalized(field), &known) {
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
            // A `$` suffix is the usual way to be a string, but not the only
            // one: `DIM S AS STRING * 4` says so with no suffix at all, and
            // without consulting that, `LEN(S)` and `S = "xy"` were both
            // rejected as numeric.
            Expr::Variable(name) | Expr::ArrayAccess { name, .. } => {
                Some(match self.symbols.typed_var(scope, name) {
                    Some(t) => matches!(t, TypeRef::FixedString(_)),
                    None => name.ends_with('$'),
                })
            }
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
                    let info = self.symbols.records.get(normalized(rec))?;
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
        BinaryOp::Eqv => "EQV",
        BinaryOp::Imp => "IMP",
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

/// How many subscripts `body` gives `name` the first time it subscripts it.
///
/// An array parameter (`A()`) does not declare its dimensions, so its uses
/// say: QuickBASIC infers them the same way.
fn subscript_count(body: &[Stmt], name: &str) -> Option<usize> {
    fn in_expr(e: &Expr, name: &str) -> Option<usize> {
        match e {
            Expr::ArrayAccess { name: n, indices }
            | Expr::FnCall {
                name: n,
                args: indices,
            } if n.eq_ignore_ascii_case(name) && !indices.is_empty() => Some(indices.len()),
            Expr::ArrayAccess { indices, .. } | Expr::FnCall { args: indices, .. } => {
                indices.iter().find_map(|i| in_expr(i, name))
            }
            Expr::Binary { left, right, .. } => {
                in_expr(left, name).or_else(|| in_expr(right, name))
            }
            Expr::Unary { operand, .. } => in_expr(operand, name),
            Expr::Field { base, .. } => in_expr(base, name),
            Expr::Literal(_) | Expr::Variable(_) => None,
        }
    }
    fn in_lvalue(lv: &LValue, name: &str) -> Option<usize> {
        match &lv.indices {
            Some(ix) if lv.name.eq_ignore_ascii_case(name) => Some(ix.len()),
            _ => None,
        }
    }
    let mut found = None;
    walk_stmts(body, &mut |stmt| {
        if found.is_some() {
            return;
        }
        found = match &stmt.kind {
            StmtKind::Let {
                name: n,
                indices: Some(ix),
                ..
            } if n.eq_ignore_ascii_case(name) => Some(ix.len()),
            StmtKind::Input { vars, .. } | StmtKind::Read(vars) => {
                vars.iter().find_map(|v| in_lvalue(v, name))
            }
            StmtKind::LineInput { var, .. } => in_lvalue(var, name),
            StmtKind::Swap(a, b) => in_lvalue(a, name).or_else(|| in_lvalue(b, name)),
            _ => None,
        };
        if found.is_none() {
            let mut stmt = stmt.clone();
            for_each_expr_mut(&mut stmt, &mut |e| {
                if found.is_none() {
                    found = in_expr(e, name);
                }
            });
        }
    });
    found
}

/// The BASIC name of a numeric or string type, for diagnostics.
fn type_name(t: DataType) -> &'static str {
    match t {
        DataType::Integer => "an INTEGER",
        DataType::Long => "a LONG",
        DataType::Single => "a SINGLE",
        DataType::Double => "a DOUBLE",
        DataType::String => "a STRING",
    }
}
