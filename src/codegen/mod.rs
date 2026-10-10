//! Code generator: lowers the AST to Cranelift IR and emits an object file
//!
//! # Architecture
//!
//! There is still no IR of this compiler's own: each statement and
//! expression is lowered straight into Cranelift IR with a
//! `FunctionBuilder`, and Cranelift does instruction selection, register
//! allocation and the object file. The runtime library is a separate object,
//! prebuilt (see `rtbuild.rs`), reached through calls whose signatures come
//! from `rtsig`.
//!
//! One `CodeGen` holds what is module-wide: the `ObjectModule`, the
//! declared functions and data, and module-level storage. Each function --
//! `main`, every SUB and FUNCTION, and the error handler -- is lowered by an
//! `FnGen`, which owns that function's builder and its own names.
//!
//! # Storage
//!
//! A numeric scalar that only its own function can reach lives in a
//! Cranelift `Variable` (see `ssa.rs`). Everything else lives in memory:
//! module-level names in zero-filled data objects (`_var_X`, `_arr_X`, ...),
//! reachable from every procedure that shares them, and a procedure's own
//! names in stack slots, zeroed on entry so that each call starts at 0 and
//! "". No plain SSA value lives across a statement: GOTO, GOSUB and RESUME
//! may enter any statement from anywhere, and a value defined in one
//! statement would not dominate the next -- a `Variable` gets a block
//! parameter there instead.
//!
//! A string is two words, pointer then length; an array descriptor is the
//! element pointer, a count per dimension, then a lower bound per dimension.
//!
//! # Errors
//!
//! A runtime check branches to a cold block per (error, line, way out),
//! which calls `_rt_error`. Without ON ERROR that never returns. With it,
//! every statement has a start block (where RESUME retries it) and a next
//! block (where RESUME NEXT goes), and an error in it goes to its fault
//! block, which calls `_err_handler_fn` -- main's statements compiled a
//! second time -- and resumes as the handler says. See `trap.rs`.

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

mod builtins;
mod calls;
mod expr;
mod hoist;
mod io;
mod ssa;
mod stmt;
mod storage;
mod strings;
mod trap;

use crate::parser::*;
use crate::rtsig;
use crate::sema::Symbols;
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    AbiParam, Block, BlockArg, FuncRef, Function, GlobalValue, GlobalValueData, InstBuilder,
    JumpTableData, MemFlagsData, Signature, StackSlot, StackSlotData, StackSlotKind, TrapCode,
    Type, UserFuncName, Value, types,
};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::{BTreeMap, HashMap, HashSet};

use storage::Place;
pub use storage::mangle;

/// Code generation options.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Emit runtime safety checks (array bounds, division by zero, overflow, ...).
    pub checks: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { checks: true }
    }
}

/// What code generation produces.
pub struct Output {
    /// The program's object file, to be linked with the runtime's.
    pub object: Vec<u8>,
    /// Each function's Cranelift IR as text, when asked for (`-S`).
    pub clif: String,
}

/// Bit 63 of a string slot's length word: set when the slot owns its bytes,
/// which assignment then frees. A FIELD variable's bytes are the record
/// buffer's, an unassigned string has none, and both have it clear.
const OWNED: i64 = i64::MIN;

/// GOSUB stack size in bytes (64K entries * 8 bytes = 512KB)
const GOSUB_STACK_SIZE: i64 = 524288;

/// The console's file number. The runtime seeds handle 0 with standard
/// output, so PRINT and PRINT # are the same helpers with a different handle.
const CONSOLE: i64 = 0;

/// Record length of a `FOR RANDOM` open with no `LEN =` clause, as in GW-BASIC.
const DEFAULT_RECLEN: i64 = 128;

/// A runtime error the generated code can raise.
///
/// Each variant names a message constant in the runtime. The BASIC line number
/// is passed separately at the call site, so the set of messages stays small no
/// matter how many check sites exist.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
enum RtError {
    Subscript,
    DivideByZero,
    Domain,
    Overflow,
    /// DIM of an array that already has storage.
    DuplicateDefinition,
    OutOfMemory,
    GosubOverflow,
    BadFileNum,
    /// RETURN with no GOSUB of its own scope to go back to.
    ReturnWithoutGosub,
    /// RESUME with nothing to return from.
    ResumeNoError,
}

impl RtError {
    /// Runtime symbol holding the message text.
    fn symbol(self) -> &'static str {
        match self {
            RtError::Subscript => "_err_subscript",
            RtError::DivideByZero => "_err_div0",
            RtError::Domain => "_err_domain",
            RtError::Overflow => "_err_overflow",
            RtError::DuplicateDefinition => "_err_dupdef",
            RtError::OutOfMemory => "_err_memory",
            RtError::GosubOverflow => "_err_gosub",
            RtError::ReturnWithoutGosub => "_err_noreturn",
            RtError::BadFileNum => "_err_badfile",
            RtError::ResumeNoError => "_err_resnoerr",
        }
    }

    /// Whether an `ON ERROR` handler may catch this.
    ///
    /// RESUME's own failure may not: the handler would be re-entered by the
    /// very RESUME that failed, and would never stop.
    fn trappable(self) -> bool {
        !matches!(self, RtError::ResumeNoError)
    }
}

/// What kind of function is being generated, for error trapping.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FnKind {
    Main,
    Proc,
    /// `_err_handler_fn`: main's code compiled a second time, entered at an
    /// ON ERROR target when an error is trapped, returning to the fault site
    /// when it RESUMEs.
    Handler,
}

/// A value produced by an expression.
#[derive(Clone, Copy, Debug)]
enum Val {
    /// A number of the given type: an INTEGER or LONG as an `i32`, a SINGLE
    /// as an `f32`, a DOUBLE as an `f64`.
    Num(DataType, Value),
    /// A string's pointer and length.
    Str(Value, Value),
}

impl Val {
    fn ty(&self) -> DataType {
        match self {
            Val::Num(t, _) => *t,
            Val::Str(..) => DataType::String,
        }
    }

    fn num(&self) -> Value {
        match self {
            Val::Num(_, v) => *v,
            Val::Str(..) => unreachable!("sema rejects a string used as a number"),
        }
    }

    fn str(&self) -> (Value, Value) {
        match self {
            Val::Str(p, l) => (*p, *l),
            Val::Num(..) => unreachable!("sema rejects a number used as a string"),
        }
    }
}

/// Where a variable, a record or an array descriptor lives.
///
/// Storage is a run of 8-byte words: module-level names in data objects,
/// a procedure's own in its frame, and a by-reference parameter or an
/// array element wherever a pointer says.
#[derive(Clone, Copy, Debug)]
enum Loc {
    Data(DataId, i32),
    Stack(StackSlot, i32),
    /// A pointer that dominates every use: a function parameter, or an
    /// address computed within the statement that uses it.
    Ptr(Value, i32),
}

impl Loc {
    /// The location `bytes` further along.
    fn offset(self, bytes: i32) -> Loc {
        match self {
            Loc::Data(d, o) => Loc::Data(d, o + bytes),
            Loc::Stack(s, o) => Loc::Stack(s, o + bytes),
            Loc::Ptr(p, o) => Loc::Ptr(p, o + bytes),
        }
    }
}

/// A variable's storage and the type it holds.
#[derive(Clone, Copy, Debug)]
struct VarInfo {
    loc: Loc,
    data_type: DataType,
}

/// An array's descriptor and its rank.
#[derive(Clone, Copy, Debug)]
struct ArrayInfo {
    loc: Loc,
    ndims: usize,
}

impl ArrayInfo {
    /// The descriptor's last word: how many running calls hold an element
    /// of the array by reference. ERASE and REDIM refuse while it is not 0.
    fn lock(&self) -> Loc {
        self.loc.offset(8 * (1 + 2 * self.ndims as i32))
    }
}

/// Module-wide state: the object being built and what every function shares.
pub struct CodeGen {
    module: ObjectModule,
    /// The same target, with the cheap settings for huge functions.
    huge_isa: OwnedTargetIsa,
    call_conv: CallConv,
    symbols: Symbols,
    opts: Options,
    /// Whether the program contains `ON ERROR`, and so pays for trapping.
    traps: bool,
    /// Whether GOSUB is used, so the GOSUB stack exists.
    gosub_used: bool,
    clif: Option<String>,

    /// Functions declared so far, by symbol.
    funcs: HashMap<String, FuncId>,
    /// Data objects declared so far, by symbol, with their size in bytes.
    data: BTreeMap<String, (DataId, u32)>,
    /// String literals, by contents.
    strings: HashMap<String, DataId>,

    /// Module-level variables, records and arrays, by name.
    vars: HashMap<String, VarInfo>,
    arrays: HashMap<String, ArrayInfo>,
    record_vars: HashMap<String, Loc>,
    /// Element size in words for module-level arrays of records.
    record_elem_words: HashMap<String, i32>,

    /// DATA items, as written, and where each label sits in them.
    data_items: Vec<DataItem>,
    data_line_index: HashMap<u32, usize>,
    data_label_index: HashMap<String, usize>,

    /// GOSUB return sites numbered so far, program-wide.
    gosub_sites: usize,
    /// Main's range of return site numbers.
    main_gosub_scope: (usize, usize),
    /// ON ERROR targets in main, numbered from 1 by position.
    err_handlers: Vec<String>,
    /// Labels in main a RESUME <label> may go to, numbered from 0.
    resume_targets: Vec<String>,
    /// Statements numbered so far, for unique fault blocks.
    stmt_count: usize,
    /// Module-level scalars that must stay in memory (see `ssa.rs`).
    module_in_memory: HashSet<String>,
    /// In a trapping program, the limit and step of each FOR in main, by
    /// the address of its limit expression: shared by main and the handler,
    /// which is main compiled a second time and may GOTO into the loop.
    for_slots: HashMap<usize, DataId>,
}

/// Lower `program` to an object file.
pub fn generate(
    program: &Program,
    symbols: Symbols,
    opts: Options,
    want_clif: bool,
) -> Result<Output, String> {
    let isa = make_isa(false)?;
    let huge_isa = make_isa(true)?;
    let call_conv = isa.default_call_conv();
    let builder = ObjectBuilder::new(isa, "basic", cranelift_module::default_libcall_names())
        .map_err(|e| e.to_string())?;
    let module = ObjectModule::new(builder);

    let mut cg = CodeGen {
        module,
        huge_isa,
        call_conv,
        symbols,
        opts,
        traps: false,
        gosub_used: false,
        clif: want_clif.then(String::new),
        funcs: HashMap::new(),
        data: BTreeMap::new(),
        strings: HashMap::new(),
        vars: HashMap::new(),
        arrays: HashMap::new(),
        record_vars: HashMap::new(),
        record_elem_words: HashMap::new(),
        data_items: Vec::new(),
        data_line_index: HashMap::new(),
        data_label_index: HashMap::new(),
        gosub_sites: 0,
        main_gosub_scope: (0, 0),
        err_handlers: Vec::new(),
        resume_targets: Vec::new(),
        stmt_count: 0,
        module_in_memory: HashSet::new(),
        for_slots: HashMap::new(),
    };

    for stmt in &program.statements {
        cg.preprocess(stmt, true);
    }
    cg.module_in_memory = ssa::module_in_memory(program, &cg.symbols);
    cg.declare_procedures(program)?;

    // Main first, so every module-level name is known before a procedure
    // refers to it.
    cg.compile_function(FnKind::Main, None, program)?;
    if cg.traps {
        cg.compile_function(FnKind::Handler, None, program)?;
    }
    for stmt in &program.statements {
        if matches!(stmt.kind, StmtKind::Sub { .. } | StmtKind::Function { .. }) {
            cg.compile_function(FnKind::Proc, Some(stmt), program)?;
        }
    }

    cg.define_data()?;

    // CLIF names a function `u0:N` and a data object `u1:N`; the header says
    // which symbol each is.
    let clif = match cg.clif.take() {
        Some(body) => {
            let decls = cg.module.declarations();
            let mut header = String::from("; symbols\n");
            for (id, f) in decls.get_functions() {
                header.push_str(&format!(
                    "; u0:{} = {}\n",
                    id.as_u32(),
                    f.name.as_deref().unwrap_or("?")
                ));
            }
            for (id, d) in decls.get_data_objects() {
                header.push_str(&format!(
                    "; u1:{} = {}\n",
                    id.as_u32(),
                    d.name.as_deref().unwrap_or("?")
                ));
            }
            header.push('\n');
            header + &body
        }
        None => String::new(),
    };
    let product = cg.module.finish();
    let object = product.emit().map_err(|e| e.to_string())?;
    Ok(Output { object, clif })
}

/// Blocks past which a function is compiled cheaply: with the single-pass
/// register allocator, and without the optimizer.
///
/// Both grow faster than the function. With its variables in SSA values, a
/// generated 10,000-line main (60,000 blocks) spent 3.7 of its 4.3 seconds
/// in the backtracking allocator, and 40,000 lines took 45 seconds; compiled
/// cheaply they take 0.32 and 1.5. The code is slower -- a hot loop ran at
/// half the speed -- so this is kept for functions far larger than a program
/// anyone writes: mega.bas's main is 600 blocks.
const HUGE_FUNCTION_BLOCKS: usize = 20_000;

/// The target: this host, as the compiler is built for the machine it runs on.
/// `huge` selects the cheap settings for functions past
/// `HUGE_FUNCTION_BLOCKS`.
fn make_isa(huge: bool) -> Result<OwnedTargetIsa, String> {
    let mut flags = settings::builder();
    let set = |f: &mut settings::Builder, k: &str, v: &str| {
        f.set(k, v).map_err(|e| format!("{}={}: {}", k, v, e))
    };
    set(&mut flags, "opt_level", if huge { "none" } else { "speed" })?;
    set(&mut flags, "is_pic", "false")?;
    set(&mut flags, "enable_verifier", "true")?;
    if huge {
        set(&mut flags, "regalloc_algorithm", "single_pass")?;
    }
    if cfg!(windows) {
        // Windows commits stack a guard page at a time, so a frame larger
        // than a page must touch each one on the way down.
        set(&mut flags, "enable_probestack", "true")?;
        set(&mut flags, "probestack_strategy", "inline")?;
    }
    let mut isa = cranelift_codegen::isa::lookup(target_lexicon::Triple::host())
        .map_err(|e| e.to_string())?;
    isa.set("has_sse41", "true").map_err(|e| e.to_string())?;
    isa.finish(settings::Flags::new(flags))
        .map_err(|e| e.to_string())
}

/// A module error as text; a compilation error -- a verifier failure, say --
/// with its details.
fn describe_module_error(e: &cranelift_module::ModuleError) -> String {
    match e {
        cranelift_module::ModuleError::Compilation(ce) => format!("{:?}", ce),
        other => other.to_string(),
    }
}

/// The Cranelift type a number of type `ty` is held in.
fn num_type(ty: DataType) -> Type {
    match ty {
        DataType::Integer | DataType::Long => types::I32,
        DataType::Single => types::F32,
        DataType::Double => types::F64,
        DataType::String => unreachable!("a string is two values"),
    }
}

/// The AbiParam types of an rtsig helper argument or result.
fn rt_type(t: rtsig::Ty) -> Type {
    match t {
        rtsig::Ty::I32 => types::I32,
        rtsig::Ty::I64 => types::I64,
        rtsig::Ty::F64 => types::F64,
    }
}

/// The C library functions generated code calls, with their signatures.
fn libc_sig(name: &str) -> (&'static [Type], &'static [Type]) {
    const F: &[Type] = &[types::F64];
    match name {
        "pow" => (&[types::F64, types::F64], F),
        "sin" | "cos" | "tan" | "atan" | "exp" | "log" => (F, F),
        "calloc" | "realloc" => (&[types::I64, types::I64], &[types::I64]),
        "free" => (&[types::I64], &[]),
        "memset" => (&[types::I64, types::I32, types::I64], &[types::I64]),
        "memcpy" => (&[types::I64, types::I64, types::I64], &[types::I64]),
        _ => unreachable!("{} is not a libc function generated code calls", name),
    }
}

impl CodeGen {
    /// Note what the program needs before any code is generated: DATA
    /// items, whether GOSUB and ON ERROR are used, and main's ON ERROR and
    /// RESUME targets, which the handler and main's unwind dispatch on.
    fn preprocess(&mut self, stmt: &Stmt, in_main: bool) {
        match &stmt.kind {
            StmtKind::Data(values) => self.data_items.extend(values.clone()),
            StmtKind::Gosub(_) | StmtKind::OnGosub { .. } => self.gosub_used = true,
            StmtKind::OnError(t) => {
                self.traps = true;
                if let Some(t) = t {
                    let label = storage::main_label(t);
                    if !self.err_handlers.contains(&label) {
                        self.err_handlers.push(label);
                    }
                }
            }
            StmtKind::Resume(ResumeTarget::At(t)) | StmtKind::Return(Some(t)) if in_main => {
                let label = storage::main_label(t);
                if !self.resume_targets.contains(&label) {
                    self.resume_targets.push(label);
                }
            }
            StmtKind::Label(n) => {
                self.data_line_index.insert(*n, self.data_items.len());
            }
            StmtKind::LabelName(name) => {
                self.data_label_index
                    .insert(name.to_uppercase(), self.data_items.len());
            }
            _ => {}
        }
        let in_main =
            in_main && !matches!(stmt.kind, StmtKind::Sub { .. } | StmtKind::Function { .. });
        for body in child_bodies(stmt) {
            for s in body {
                self.preprocess(s, in_main);
            }
        }
    }

    /// The signature of a user procedure, from its declared parameters.
    ///
    /// An array, a record and anything by reference arrives as an address;
    /// a string by value as its pointer and length; any other number by
    /// value as a Double, which the procedure narrows to the parameter's
    /// type -- so an argument rounds and range checks as an assignment
    /// would, and in the procedure, where QuickBASIC raises it.
    fn proc_signature(&self, params: &[Param], ret: Option<DataType>) -> Signature {
        let mut sig = Signature::new(self.call_conv);
        for p in params {
            match calls::param_pass(p) {
                calls::Pass::Pointer => sig.params.push(AbiParam::new(types::I64)),
                calls::Pass::Str => {
                    sig.params.push(AbiParam::new(types::I64));
                    sig.params.push(AbiParam::new(types::I64));
                }
                calls::Pass::Number(_) => sig.params.push(AbiParam::new(types::F64)),
            }
        }
        match ret {
            None => {}
            Some(DataType::String) => {
                sig.returns.push(AbiParam::new(types::I64));
                sig.returns.push(AbiParam::new(types::I64));
            }
            Some(t) => sig.returns.push(AbiParam::new(num_type(t))),
        }
        sig
    }

    /// Declare `main`, every procedure and the handler, so that any function
    /// may call any other whatever order they are compiled in.
    fn declare_procedures(&mut self, program: &Program) -> Result<(), String> {
        let mut sig = Signature::new(self.call_conv);
        sig.returns.push(AbiParam::new(types::I32));
        let id = self
            .module
            .declare_function("main", Linkage::Export, &sig)
            .map_err(|e| e.to_string())?;
        self.funcs.insert("main".to_string(), id);

        if self.traps {
            let id = self
                .module
                .declare_function("_err_handler_fn", Linkage::Local, &sig)
                .map_err(|e| e.to_string())?;
            self.funcs.insert("_err_handler_fn".to_string(), id);
        }

        for stmt in &program.statements {
            let (name, params, is_function) = match &stmt.kind {
                StmtKind::Sub { name, params, .. } => (name, params, false),
                StmtKind::Function { name, params, .. } => (name, params, true),
                _ => continue,
            };
            let ret = is_function.then(|| self.proc_return_type(name));
            let sig = self.proc_signature(params, ret);
            let sym = format!("_proc_{}", mangle(&name.to_uppercase()));
            let id = self
                .module
                .declare_function(&sym, Linkage::Local, &sig)
                .map_err(|e| e.to_string())?;
            self.funcs.insert(sym, id);
        }
        Ok(())
    }

    /// Result type of a user-defined FUNCTION: its `AS` clause, or its name.
    fn proc_return_type(&self, name: &str) -> DataType {
        match self
            .symbols
            .procs
            .get(&name.to_uppercase())
            .and_then(|p| p.ret_ty.as_ref())
        {
            Some(ty) => DataType::from_type_ref(ty),
            None => DataType::from_suffix(name),
        }
    }

    /// A zero-filled, writable data object of at least `bytes` bytes.
    fn global(&mut self, sym: &str, bytes: u32) -> DataId {
        if let Some((id, size)) = self.data.get_mut(sym) {
            *size = (*size).max(bytes);
            return *id;
        }
        let id = self
            .module
            .declare_data(sym, Linkage::Local, true, false)
            .expect("a fresh local data symbol");
        self.data.insert(sym.to_string(), (id, bytes));
        id
    }

    /// A data object the runtime defines.
    fn runtime_data(&mut self, sym: &str) -> DataId {
        if let Some((id, _)) = self.data.get(sym) {
            return *id;
        }
        let id = self
            .module
            .declare_data(sym, Linkage::Import, true, false)
            .expect("a runtime data symbol");
        // Size 0 marks it as defined elsewhere.
        self.data.insert(sym.to_string(), (id, 0));
        id
    }

    /// The data object holding string literal `s`, NUL-terminated: the
    /// runtime measures a DATA string with strlen.
    fn string_literal(&mut self, s: &str) -> DataId {
        if let Some(id) = self.strings.get(s) {
            return *id;
        }
        let sym = format!("_str_{}", self.strings.len());
        let id = self
            .module
            .declare_data(&sym, Linkage::Local, false, false)
            .expect("a fresh string symbol");
        let mut desc = DataDescription::new();
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        desc.define(bytes.into_boxed_slice());
        self.module
            .define_data(id, &desc)
            .expect("a string literal defines once");
        self.strings.insert(s.to_string(), id);
        id
    }

    /// A function generated code calls by name: a runtime helper, from its
    /// rtsig signature, or a C library function.
    fn import_function(&mut self, name: &str) -> FuncId {
        if let Some(id) = self.funcs.get(name) {
            return *id;
        }
        let mut sig = Signature::new(self.call_conv);
        if name.starts_with("_rt_") {
            let h = rtsig::lookup(name);
            sig.params
                .extend(h.params.iter().map(|t| AbiParam::new(rt_type(*t))));
            sig.returns
                .extend(h.rets.iter().map(|t| AbiParam::new(rt_type(*t))));
        } else {
            let (params, rets) = libc_sig(name);
            sig.params.extend(params.iter().map(|t| AbiParam::new(*t)));
            sig.returns.extend(rets.iter().map(|t| AbiParam::new(*t)));
        }
        let id = self
            .module
            .declare_function(name, Linkage::Import, &sig)
            .expect("an imported function");
        self.funcs.insert(name.to_string(), id);
        id
    }

    /// Define every zero-filled data object, and the DATA table.
    fn define_data(&mut self) -> Result<(), String> {
        // The DATA table: (quoted, text) pairs, which the runtime's READ
        // helpers walk. Always defined, even empty, since they name it.
        let items = std::mem::take(&mut self.data_items);
        let texts: Vec<DataId> = items.iter().map(|i| self.string_literal(&i.text)).collect();
        let table = self
            .module
            .declare_data("_data_table", Linkage::Export, true, false)
            .map_err(|e| e.to_string())?;
        let mut desc = DataDescription::new();
        let mut bytes = vec![0u8; 16 * items.len().max(1)];
        for (i, item) in items.iter().enumerate() {
            bytes[16 * i..16 * i + 8].copy_from_slice(&u64::from(item.quoted).to_le_bytes());
        }
        desc.define(bytes.into_boxed_slice());
        desc.set_align(8);
        for (i, id) in texts.iter().enumerate() {
            let gv = self.module.declare_data_in_data(*id, &mut desc);
            desc.write_data_addr(16 * i as u32 + 8, gv, 0);
        }
        self.module
            .define_data(table, &desc)
            .map_err(|e| e.to_string())?;

        let count = self
            .module
            .declare_data("_data_count", Linkage::Export, true, false)
            .map_err(|e| e.to_string())?;
        let mut desc = DataDescription::new();
        desc.define(Box::new((items.len() as u64).to_le_bytes()));
        desc.set_align(8);
        self.module
            .define_data(count, &desc)
            .map_err(|e| e.to_string())?;

        let ptr = self
            .module
            .declare_data("_data_ptr", Linkage::Export, true, false)
            .map_err(|e| e.to_string())?;
        let mut desc = DataDescription::new();
        desc.define_zeroinit(8);
        desc.set_align(8);
        self.module
            .define_data(ptr, &desc)
            .map_err(|e| e.to_string())?;

        for (id, size) in self.data.values() {
            if *size == 0 {
                continue; // the runtime's
            }
            let mut desc = DataDescription::new();
            desc.define_zeroinit(*size as usize);
            desc.set_align(16);
            self.module
                .define_data(*id, &desc)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Compile a huge function with `huge_isa` and define its bytes.
    fn define_huge(
        &mut self,
        id: FuncId,
        ctx: &mut cranelift_codegen::Context,
    ) -> Result<(), String> {
        ctx.compile(&*self.huge_isa, &mut Default::default())
            .map_err(|e| format!("{:?}", e.inner))?;
        let code = ctx.compiled_code().expect("just compiled");
        let relocs: Vec<cranelift_module::ModuleReloc> = code
            .buffer
            .relocs()
            .iter()
            .map(|r| cranelift_module::ModuleReloc::from_mach_reloc(r, &ctx.func, id))
            .collect();
        let alignment = u64::from(code.buffer.alignment);
        let bytes = code.code_buffer().to_vec();
        self.module
            .define_function_bytes(id, alignment, &bytes, &relocs)
            .map_err(|e| describe_module_error(&e))
    }

    /// Lower one function -- main, the handler, or the procedure `proc` --
    /// and define it in the module.
    fn compile_function(
        &mut self,
        kind: FnKind,
        proc: Option<&Stmt>,
        program: &Program,
    ) -> Result<(), String> {
        let sym = match (kind, proc.map(|s| &s.kind)) {
            (FnKind::Main, _) => "main".to_string(),
            (FnKind::Handler, _) => "_err_handler_fn".to_string(),
            (FnKind::Proc, Some(StmtKind::Sub { name, .. } | StmtKind::Function { name, .. })) => {
                format!("_proc_{}", mangle(&name.to_uppercase()))
            }
            _ => unreachable!("a procedure is a SUB or FUNCTION"),
        };
        let id = self.funcs[&sym];
        let sig = self
            .module
            .declarations()
            .get_function_decl(id)
            .signature
            .clone();
        let mut func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), sig);
        let mut fctx = FunctionBuilderContext::new();
        {
            let b = FunctionBuilder::new(&mut func, &mut fctx);
            let mut f = FnGen::new(self, b, kind);
            match (kind, proc) {
                (FnKind::Main, _) => f.gen_main(program),
                (FnKind::Handler, _) => f.gen_handler(program),
                (FnKind::Proc, Some(stmt)) => f.gen_procedure(stmt),
                _ => unreachable!(),
            }
            f.finish();
        }

        if let Some(clif) = &mut self.clif {
            // The text names an external by its index in this function's own
            // table, `userextnameM`; the header's `uN:M` names are global.
            let mut text = func.display().to_string();
            let names: Vec<(usize, String)> = func
                .params
                .user_named_funcs()
                .iter()
                .map(|(r, n)| (r.as_u32() as usize, format!("u{}:{}", n.namespace, n.index)))
                .collect();
            for (r, name) in names.iter().rev() {
                text = text.replace(&format!("userextname{}", r), name);
            }
            clif.push_str(&format!("; {}\n{}\n", sym, text));
        }
        let mut ctx = cranelift_codegen::Context::for_function(func);
        let r = if ctx.func.layout.blocks().count() > HUGE_FUNCTION_BLOCKS {
            self.define_huge(id, &mut ctx)
        } else {
            self.module
                .define_function(id, &mut ctx)
                .map_err(|e| describe_module_error(&e))
        };
        r.map_err(|e| format!("code generation failed in {}: {}", sym, e))
    }
}

/// One function being lowered.
struct FnGen<'a> {
    cg: &'a mut CodeGen,
    b: FunctionBuilder<'a>,
    kind: FnKind,
    /// The procedure being generated, upper case; None in main and the handler.
    current_proc: Option<String>,

    /// The current procedure's own names.
    proc_vars: HashMap<String, VarInfo>,
    proc_arrays: HashMap<String, ArrayInfo>,
    proc_record_vars: HashMap<String, Loc>,
    /// Declared types of the current procedure's typed parameters.
    proc_types: HashMap<String, TypeRef>,
    proc_record_elem_words: HashMap<String, i32>,
    /// Stack slots to zero on entry, with their sizes in bytes.
    zeroed_slots: Vec<(StackSlot, u32)>,
    /// Fresh strings this statement made and nothing has taken yet; freed
    /// when it is done. See `strings.rs`.
    temps: Vec<Value>,
    /// Whether this statement calls a user FUNCTION, which could free a
    /// string value already loaded: string loads are then copies.
    copy_loads: bool,
    /// Scalars held in SSA variables, by name, and whether this function
    /// may hold any; names this function passes to procedures may not.
    ssa: HashMap<String, (Variable, DataType)>,
    ssa_enabled: bool,
    ssa_excluded: HashSet<String>,
    /// Labels something branches to; only these get a block.
    branch_targets: HashSet<String>,
    /// Descriptors of arrays the loops being compiled read before their
    /// heads, by upper-case name. See `hoist.rs`.
    hoisted: HashMap<String, hoist::Hoisted>,

    /// Blocks of this function's labels, by symbol.
    labels: HashMap<String, Block>,
    /// This scope's GOSUB return sites: the first number, and a block per
    /// site, taken in order as GOSUBs are compiled.
    gosub_first: usize,
    gosub_blocks: Vec<Block>,
    gosub_next: usize,
    /// Enclosing loops, innermost last: (is FOR, exit block).
    loop_stack: Vec<(bool, Block)>,
    /// A procedure's epilogue, which EXIT SUB and an unwind go to.
    exit_block: Option<Block>,

    /// Error blocks, by (error, line, where a trapped error goes next).
    error_blocks: HashMap<(RtError, u32, Option<Block>), Block>,
    /// The statement being generated when trapping: (number, start, next).
    cur_stmt: Option<(usize, Block, Block)>,
    /// Fault blocks wanted so far: (fault, start, next).
    faults: HashMap<usize, (Block, Block, Block)>,
    /// Where this function goes to be left on the way to a RESUME label.
    unwind_block: Option<Block>,
    /// The handler's return, taking the code it returns.
    handler_ret: Option<Block>,
    /// In the handler: its entry, which dispatches on `_err_handler`; the
    /// statement that faulted in the handler's own code, plus one, or 0; and
    /// the blocks RESUME and RESUME NEXT go to when that is set.
    handler_dispatch: Option<Block>,
    local_fault: Option<Variable>,
    local_resume: Option<(Block, Block)>,

    func_refs: HashMap<FuncId, FuncRef>,
    data_gvs: HashMap<DataId, GlobalValue>,
    /// Physical source line of the statement being compiled.
    current_line: u32,
    /// The BASIC line number most recently reached, or 0 before the first.
    basic_line: u32,
    /// In a program that does not trap, the block where this statement
    /// stored `_err_line` for a fallible helper: a later one in the same
    /// block -- or in one reached only from it -- needs no store of its own.
    /// Forgotten when a statement begins or ends and after a procedure call,
    /// each of which may store another line.
    err_line_block: Option<Block>,
}

impl<'a> FnGen<'a> {
    fn new(cg: &'a mut CodeGen, mut b: FunctionBuilder<'a>, kind: FnKind) -> Self {
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        FnGen {
            cg,
            b,
            kind,
            current_proc: None,
            proc_vars: HashMap::new(),
            proc_arrays: HashMap::new(),
            proc_record_vars: HashMap::new(),
            proc_types: HashMap::new(),
            proc_record_elem_words: HashMap::new(),
            zeroed_slots: Vec::new(),
            temps: Vec::new(),
            copy_loads: false,
            ssa: HashMap::new(),
            ssa_enabled: false,
            ssa_excluded: HashSet::new(),
            branch_targets: HashSet::new(),
            hoisted: HashMap::new(),
            labels: HashMap::new(),
            gosub_first: 0,
            gosub_blocks: Vec::new(),
            gosub_next: 0,
            loop_stack: Vec::new(),
            exit_block: None,
            error_blocks: HashMap::new(),
            cur_stmt: None,
            faults: HashMap::new(),
            unwind_block: None,
            handler_ret: None,
            handler_dispatch: None,
            local_fault: None,
            local_resume: None,
            func_refs: HashMap::new(),
            data_gvs: HashMap::new(),
            current_line: 0,
            basic_line: 0,
            err_line_block: None,
        }
    }

    /// Fill the cold blocks collected while the body was compiled, then
    /// close the function.
    fn finish(mut self) {
        self.fill_error_blocks();
        self.fill_fault_blocks();
        self.b.seal_all_blocks();
        let config = self.cg.module.isa().frontend_config();
        self.b.finalize(config);
    }

    // ---- blocks ----

    /// End the current block with a jump to `block` and continue there.
    /// The block stays unsealed: more predecessors may come.
    fn begin_block(&mut self, block: Block) {
        self.b.ins().jump(block, &[]);
        self.b.switch_to_block(block);
    }

    /// The same, for a block whose predecessors all exist once this jump
    /// does.
    ///
    /// Sealing a block as early as possible is what keeps SSA construction
    /// linear: a variable read in a sealed block is looked up through its
    /// known predecessors, while in an unsealed one it costs a block
    /// parameter, resolved only at the end. Unsealed until the end, every
    /// block of a large program gets one per variable.
    fn begin_sealed(&mut self, block: Block) {
        self.begin_block(block);
        self.b.seal_block(block);
    }

    /// Continue in `block`, all of whose predecessors already exist.
    fn switch_sealed(&mut self, block: Block) {
        self.b.switch_to_block(block);
        self.b.seal_block(block);
    }

    /// Continue in a fresh block after a terminator. Code that follows --
    /// statements after a GOTO -- is unreachable, but still has to be
    /// somewhere; with no predecessors, it is sealed at once.
    fn after_terminator(&mut self) {
        let next = self.b.create_block();
        self.switch_sealed(next);
    }

    /// Jump to `block`; what follows is unreachable.
    fn jump(&mut self, block: Block) {
        self.b.ins().jump(block, &[]);
        self.after_terminator();
    }

    /// End the program here: what follows a call that never returns.
    fn unreachable(&mut self) {
        self.b.ins().trap(TrapCode::unwrap_user(1));
        self.after_terminator();
    }

    /// Branch on `cond` (an integer, true when nonzero): to `then` and to
    /// `else_`.
    fn brif(&mut self, cond: Value, then: Block, else_: Block) {
        self.b.ins().brif(cond, then, &[], else_, &[]);
    }

    /// Dispatch on `index` (an i64 or narrower integer) through `targets`,
    /// to `default` when it is out of range.
    fn br_table(&mut self, index: Value, targets: &[Block], default: Block) {
        let index = self.index_i32(index);
        let pool = &mut self.b.func.dfg;
        let def = pool.block_call(default, &[]);
        let calls: Vec<_> = targets.iter().map(|t| pool.block_call(*t, &[])).collect();
        let jt = self.b.create_jump_table(JumpTableData::new(def, &calls));
        self.b.ins().br_table(index, jt);
        self.after_terminator();
    }

    /// An integer value narrowed or widened to i32 for a jump table index.
    fn index_i32(&mut self, v: Value) -> Value {
        match self.b.func.dfg.value_type(v) {
            types::I64 => self.b.ins().ireduce(types::I32, v),
            types::I32 => v,
            _ => self.b.ins().sextend(types::I32, v),
        }
    }

    // ---- constants and addresses ----

    fn iconst(&mut self, ty: Type, n: i64) -> Value {
        self.b.ins().iconst(ty, n)
    }

    fn f64const(&mut self, v: f64) -> Value {
        self.b.ins().f64const(v)
    }

    /// The address of a data object, as an i64.
    fn data_addr(&mut self, id: DataId) -> Value {
        let gv = match self.data_gvs.get(&id) {
            Some(gv) => *gv,
            None => {
                let gv = self.cg.module.declare_data_in_func(id, self.b.func);
                // Everything is linked into one executable, so a direct,
                // PC-relative reference reaches it.
                if let GlobalValueData::Symbol { colocated, .. } =
                    &mut self.b.func.global_values[gv]
                {
                    *colocated = true;
                }
                self.data_gvs.insert(id, gv);
                gv
            }
        };
        self.b.ins().symbol_value(types::I64, gv)
    }

    /// The address of a runtime data symbol.
    fn rt_data_addr(&mut self, sym: &str) -> Value {
        let id = self.cg.runtime_data(sym);
        self.data_addr(id)
    }

    /// The address of a module-level data object of this program.
    fn global_addr(&mut self, sym: &str, bytes: u32) -> Value {
        let id = self.cg.global(sym, bytes);
        self.data_addr(id)
    }

    /// A string literal's (pointer, length).
    fn str_literal(&mut self, s: &str) -> (Value, Value) {
        let id = self.cg.string_literal(s);
        let p = self.data_addr(id);
        let l = self.iconst(types::I64, s.len() as i64);
        (p, l)
    }

    /// The address `loc` names.
    fn addr(&mut self, loc: Loc) -> Value {
        match loc {
            Loc::Data(id, off) => {
                let base = self.data_addr(id);
                if off == 0 {
                    base
                } else {
                    self.b.ins().iadd_imm_s(base, i64::from(off))
                }
            }
            Loc::Stack(slot, off) => self.b.ins().stack_addr(types::I64, slot, off),
            Loc::Ptr(p, off) => {
                if off == 0 {
                    p
                } else {
                    self.b.ins().iadd_imm_s(p, i64::from(off))
                }
            }
        }
    }

    fn load(&mut self, ty: Type, loc: Loc) -> Value {
        match loc {
            Loc::Stack(slot, off) => self.b.ins().stack_load(types::I64, ty, slot, off),
            Loc::Data(id, off) => {
                let base = self.data_addr(id);
                self.b.ins().load(ty, MemFlagsData::trusted(), base, off)
            }
            Loc::Ptr(p, off) => self.b.ins().load(ty, MemFlagsData::trusted(), p, off),
        }
    }

    fn store(&mut self, v: Value, loc: Loc) {
        match loc {
            Loc::Stack(slot, off) => {
                self.b.ins().stack_store(types::I64, v, slot, off);
            }
            Loc::Data(id, off) => {
                let base = self.data_addr(id);
                self.b.ins().store(MemFlagsData::trusted(), v, base, off);
            }
            Loc::Ptr(p, off) => {
                self.b.ins().store(MemFlagsData::trusted(), v, p, off);
            }
        }
    }

    /// A word of the runtime's error state, or of this program's.
    fn load_rt_word(&mut self, sym: &str) -> Value {
        let a = self.rt_data_addr(sym);
        self.b.ins().load(types::I64, MemFlagsData::trusted(), a, 0)
    }

    fn store_rt_word(&mut self, sym: &str, v: i64) {
        let a = self.rt_data_addr(sym);
        let v = self.iconst(types::I64, v);
        self.b.ins().store(MemFlagsData::trusted(), v, a, 0);
    }

    /// A fresh stack slot of `bytes`, 8-aligned.
    fn stack_slot(&mut self, bytes: u32) -> StackSlot {
        self.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            bytes.max(8),
            3,
        ))
    }

    /// A stack slot that starts each call as zeros: a procedure's own
    /// variable. Main and the handler use only data objects for variables.
    fn local_slot(&mut self, bytes: u32) -> StackSlot {
        let slot = self.stack_slot(bytes);
        self.zeroed_slots.push((slot, bytes.max(8)));
        slot
    }

    /// Load a number of type `ty` stored at its own width, widened to the
    /// type it is computed in: an INTEGER's 16 bits sign-extended to 32.
    fn load_num(&mut self, ty: DataType, loc: Loc) -> Value {
        match ty {
            DataType::Integer => {
                let v = self.load(types::I16, loc);
                self.b.ins().sextend(types::I32, v)
            }
            t => self.load(num_type(t), loc),
        }
    }

    /// Store a number of type `ty` at its own width.
    fn store_num(&mut self, ty: DataType, v: Value, loc: Loc) {
        match ty {
            DataType::Integer => {
                let n = self.b.ins().ireduce(types::I16, v);
                self.store(n, loc);
            }
            _ => self.store(v, loc),
        }
    }

    /// A string slot's (pointer, length), the owned bit masked off.
    fn load_str(&mut self, loc: Loc) -> (Value, Value) {
        let (p, raw) = self.load_str_raw(loc);
        let l = self.b.ins().band_imm_s(raw, i64::MAX);
        (p, l)
    }

    /// A string slot's two words as stored, owned bit and all.
    fn load_str_raw(&mut self, loc: Loc) -> (Value, Value) {
        let p = self.load(types::I64, loc);
        let l = self.load(types::I64, loc.offset(8));
        (p, l)
    }

    /// Store a string slot's two words as given.
    fn store_str_raw(&mut self, (p, l): (Value, Value), loc: Loc) {
        self.store(p, loc);
        self.store(l, loc.offset(8));
    }

    /// Load a value of type `ty` from `loc`.
    fn load_val(&mut self, ty: DataType, loc: Loc) -> Val {
        if ty == DataType::String {
            let (p, l) = self.load_str(loc);
            Val::Str(p, l)
        } else {
            Val::Num(ty, self.load_num(ty, loc))
        }
    }

    // ---- calls ----

    fn func_ref(&mut self, id: FuncId) -> FuncRef {
        if let Some(r) = self.func_refs.get(&id) {
            return *r;
        }
        let r = self.cg.module.declare_func_in_func(id, self.b.func);
        self.b.func.dfg.ext_funcs[r].colocated = true;
        self.func_refs.insert(id, r);
        r
    }

    /// Call `name` -- a runtime helper or a C library function -- with
    /// `args`, an integer argument widened to the helper's 64 bits. A
    /// helper that can raise an error is followed by the trap check.
    fn call(&mut self, name: &str, args: &[Value]) -> Vec<Value> {
        let id = self.cg.import_function(name);
        let fref = self.func_ref(id);
        let sig = self.b.func.dfg.ext_funcs[fref].signature;
        let want: Vec<Type> = self.b.func.dfg.signatures[sig]
            .params
            .iter()
            .map(|p| p.value_type)
            .collect();
        assert_eq!(want.len(), args.len(), "arguments to {}", name);
        let args: Vec<Value> = args
            .iter()
            .zip(&want)
            .map(|(a, t)| {
                let have = self.b.func.dfg.value_type(*a);
                if have == *t {
                    *a
                } else if have.is_int() && t.is_int() && have.bits() < t.bits() {
                    self.b.ins().sextend(*t, *a)
                } else if have == types::F32 && *t == types::F64 {
                    self.b.ins().fpromote(types::F64, *a)
                } else {
                    panic!("{}: argument {} where {} is wanted", name, have, t)
                }
            })
            .collect();
        // The line a helper's error reports. Most helpers are given none and
        // pass 0, so the runtime prints _err_line instead; a program that
        // traps has stored it already, per statement, for ERL. Stored once
        // where it still holds: see `err_line_block`.
        if !self.cg.traps && name.starts_with("_rt_") && rtsig::lookup(name).fallible {
            let here = self.b.current_block();
            if here.is_none() || self.err_line_block != here {
                let line = i64::from(self.report_line());
                self.store_rt_word("_err_line", line);
                self.err_line_block = here;
            }
        }
        let inst = self.b.ins().call(fref, &args);
        let rets = self.b.inst_results(inst).to_vec();
        if name.starts_with("_rt_") {
            let h = rtsig::lookup(name);
            if h.fresh {
                self.temps.push(rets[0]);
            }
            if h.fallible {
                self.error_check();
            }
            if h.noreturn {
                self.unreachable();
            }
        }
        rets
    }

    /// Call a runtime helper whose arguments are an i64 each.
    fn call_rt(&mut self, name: &str, args: &[Value]) -> Vec<Value> {
        self.call(name, args)
    }

    // ---- checks ----

    /// The line a runtime diagnostic quotes: the BASIC line number once one
    /// has been reached, otherwise the physical source line.
    fn report_line(&self) -> u32 {
        if self.basic_line != 0 {
            self.basic_line
        } else {
            self.current_line
        }
    }

    /// Branch to the error block for `kind` when `cond` holds -- unless
    /// checks are off.
    fn check(&mut self, cond: Value, kind: RtError) {
        if self.cg.opts.checks {
            self.raise_if(cond, kind);
        }
    }

    /// Branch to the error block for `kind` when `cond` holds, whatever the
    /// options say.
    fn raise_if(&mut self, cond: Value, kind: RtError) {
        let eb = self.error_block(kind);
        let cont = self.b.create_block();
        let from = self.b.current_block();
        self.brif(cond, eb, cont);
        self.switch_sealed(cont);
        self.carry_err_line(from, cont);
    }

    /// `to` is reached only through `from`: a line stored there holds.
    fn carry_err_line(&mut self, from: Option<Block>, to: Block) {
        if from.is_some() && self.err_line_block == from {
            self.err_line_block = Some(to);
        }
    }

    /// Raise `kind` unconditionally: what follows is unreachable.
    fn raise(&mut self, kind: RtError) {
        let eb = self.error_block(kind);
        self.jump(eb);
    }

    /// The cold block that raises `kind` at the current line, creating it
    /// the first time a site needs it.
    fn error_block(&mut self, kind: RtError) -> Block {
        let line = self.report_line();
        let exit = if self.cg.traps && kind.trappable() {
            Some(self.error_exit())
        } else {
            None
        };
        let key = (kind, line, exit);
        if let Some(b) = self.error_blocks.get(&key) {
            return *b;
        }
        let block = self.b.create_block();
        self.b.set_cold_block(block);
        self.error_blocks.insert(key, block);
        block
    }

    fn fill_error_blocks(&mut self) {
        let mut blocks: Vec<_> = self.error_blocks.iter().map(|(k, b)| (*k, *b)).collect();
        blocks.sort_by_key(|(_, b)| *b);
        for ((kind, line, exit), block) in blocks {
            self.b.switch_to_block(block);
            let msg = self.rt_data_addr(kind.symbol());
            let line = self.iconst(types::I64, i64::from(line));
            let entry = if kind.trappable() {
                "_rt_error"
            } else {
                "_rt_fatal"
            };
            let id = self.cg.import_function(entry);
            let fref = self.func_ref(id);
            self.b.ins().call(fref, &[msg, line]);
            // _rt_error returns only when a handler traps the error.
            match exit {
                Some(e) => {
                    self.b.ins().jump(e, &[]);
                }
                None => {
                    self.b.ins().trap(TrapCode::unwrap_user(1));
                }
            }
        }
    }

    /// In a program that traps, leave for the error exit if the call just
    /// made raised an error a handler trapped.
    fn error_check(&mut self) {
        if !self.cg.traps {
            return;
        }
        let exit = self.error_exit();
        let p = self.load_rt_word("_err_pending");
        let cont = self.b.create_block();
        self.brif(p, exit, cont);
        self.switch_sealed(cont);
    }

    // ---- numeric helpers shared by expressions and statements ----

    /// An i8 condition true when the 32-bit `v` does not fit 16 bits.
    fn outside_i16(&mut self, v: Value) -> Value {
        let n = self.b.ins().ireduce(types::I16, v);
        let w = self.b.ins().sextend(types::I32, n);
        self.b.ins().icmp(IntCC::NotEqual, w, v)
    }

    /// Overflow unless the 32-bit `v` fits an INTEGER.
    fn check_i16(&mut self, v: Value) {
        if self.cg.opts.checks {
            let bad = self.outside_i16(v);
            self.raise_if(bad, RtError::Overflow);
        }
    }

    /// Overflow if the float `v` is infinite or NaN: a result too large for
    /// its type.
    ///
    /// `v - v` is NaN exactly then, so one subtract and an unordered compare
    /// of the difference with itself say so, with no constant to load: the
    /// `|v| >= inf` form this replaced rebuilt both its mask and infinity in
    /// every iteration of a loop.
    fn check_float(&mut self, v: Value, _ty: DataType) {
        if !self.cg.opts.checks {
            return;
        }
        let d = self.b.ins().fsub(v, v);
        let bad = self.b.ins().fcmp(FloatCC::Unordered, d, d);
        self.raise_if(bad, RtError::Overflow);
    }

    /// Raise `kind` when the i64 `v` is outside `min..=max`.
    fn check_range(&mut self, v: Value, min: i64, max: Option<i64>, kind: RtError) {
        if !self.cg.opts.checks {
            return;
        }
        let lo = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, v, min);
        self.raise_if(lo, kind);
        if let Some(max) = max {
            let hi = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, v, max);
            self.raise_if(hi, kind);
        }
    }

    /// The i8 BASIC truth -1/0 of an i8 condition, as a LONG.
    fn bool_val(&mut self, c: Value) -> Val {
        let v = self.b.ins().bmask(types::I32, c);
        Val::Num(DataType::Long, v)
    }
}
