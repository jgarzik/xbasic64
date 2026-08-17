//! BASIC parser - produces AST from tokens

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::lexer::{DataTypeWord, Token};
use std::collections::{HashSet, VecDeque};

/// Binary operator precedence levels (higher = tighter binding)
///
/// These are LANGREF's table read upside down -- it numbers 1 as tightest, this
/// numbers 1 as loosest. `XOR` used to sit alone at 3, binding tighter than
/// `AND`, which contradicted both LANGREF and GW-BASIC and made
/// `-1 OR 0 XOR -1` give -1 instead of 0.
///
/// Returns (precedence, BinaryOp) or None if not a binary operator
fn binary_op_info(token: &Token) -> Option<(u8, BinaryOp)> {
    match token {
        // Precedence 1: implication (lowest)
        Token::Imp => Some((1, BinaryOp::Imp)),
        // Precedence 2: equivalence
        Token::Eqv => Some((2, BinaryOp::Eqv)),
        // Precedence 3: logical OR and XOR, which share a level
        Token::Or => Some((3, BinaryOp::Or)),
        Token::Xor => Some((3, BinaryOp::Xor)),
        // Precedence 4: logical AND
        Token::And => Some((4, BinaryOp::And)),
        // Precedence 5 is NOT, a prefix operator; see `parse_prec_inner`.
        // Precedence 6: comparison
        Token::Eq => Some((CMP_PREC, BinaryOp::Eq)),
        Token::Ne => Some((CMP_PREC, BinaryOp::Ne)),
        Token::Lt => Some((CMP_PREC, BinaryOp::Lt)),
        Token::Gt => Some((CMP_PREC, BinaryOp::Gt)),
        Token::Le => Some((CMP_PREC, BinaryOp::Le)),
        Token::Ge => Some((CMP_PREC, BinaryOp::Ge)),
        // Precedence 7: additive
        Token::Plus => Some((7, BinaryOp::Add)),
        Token::Minus => Some((7, BinaryOp::Sub)),
        // Precedence 8: multiplicative
        Token::Star => Some((8, BinaryOp::Mul)),
        Token::Slash => Some((8, BinaryOp::Div)),
        Token::Backslash => Some((8, BinaryOp::IntDiv)),
        Token::Mod => Some((8, BinaryOp::Mod)),
        // Precedence 9: power
        Token::Caret => Some((POWER_PREC, BinaryOp::Pow)),
        _ => None,
    }
}

/// Precedence of the comparison operators.
///
/// Named because `NOT` sits directly below it: `NOT` takes an operand at this
/// level, which is what makes `NOT A = B` group as `NOT (A = B)` while
/// `NOT A AND B` groups as `(NOT A) AND B`.
const CMP_PREC: u8 = 6;

/// Precedence of `^`, the tightest-binding binary operator.
const POWER_PREC: u8 = 9;

/// How deeply expressions and blocks may nest before the parser gives up.
///
/// This is a recursive-descent parser, so nesting costs stack. Without a bound
/// it did not refuse deep input, it *died* on it: 50,000 nested parentheses --
/// or the same number of unary minuses or `NOT`s, which descend through the
/// same path -- aborted the process with "fatal runtime error: stack overflow"
/// and exit code 134, which is outside anything a caller can interpret.
///
/// The limit is far above what a person writes and far below what the stack
/// can take, so the only programs it rejects are ones that were going to crash.
const MAX_DEPTH: u32 = 256;

// AST Definitions

#[derive(Debug, Clone)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

/// A statement together with the source line it started on.
///
/// The line is recorded once, at the statement level: BASIC is line-oriented,
/// so that is the resolution diagnostics actually need, and it avoids threading
/// spans through every token and expression.
#[derive(Debug, Clone)]
pub struct Stmt {
    pub line: u32,
    pub kind: StmtKind,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Label(u32), // Line number label
    /// A named label definition (`Retry:`), usable as a GOTO/GOSUB target.
    LabelName(String),
    Let {
        name: String,
        indices: Option<Vec<Expr>>, // For array assignment
        value: Expr,
    },
    Print {
        /// `PRINT #n` writes to a file; `None` is the console, which the
        /// runtime reaches as file handle 0.
        file_num: Option<Expr>,
        items: Vec<PrintItem>,
        newline: bool,
        /// PRINT USING format string, when one was given.
        using: Option<Expr>,
        /// True for WRITE: values are comma-separated and strings quoted.
        write: bool,
    },
    Input {
        prompt: Option<String>,
        /// Whether to print `? ` after the prompt.
        ///
        /// GW-BASIC decides this by the separator: `INPUT "p"; A` prints `p? `
        /// and `INPUT "p", A` prints `p` alone, while a promptless `INPUT A`
        /// prints just `? `. The parser used to accept either separator and
        /// discard which, so no form ever printed a question mark.
        query: bool,
        vars: Vec<LValue>,
        /// `INPUT #n` reads from a file; `None` is the console.
        file_num: Option<Expr>,
    },
    LineInput {
        prompt: Option<String>,
        var: LValue,
        /// `LINE INPUT #n, Var$` reads a whole line from a file.
        file_num: Option<Expr>,
    },
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    For {
        var: String,
        start: Expr,
        end: Expr,
        step: Option<Expr>,
        body: Vec<Stmt>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    DoLoop {
        condition: Option<Expr>,
        cond_at_start: bool,
        is_until: bool,
        body: Vec<Stmt>,
    },
    Goto(GotoTarget),
    Gosub(GotoTarget),
    Return,
    OnGoto {
        expr: Expr,
        targets: Vec<GotoTarget>,
    },
    /// `ON expr GOSUB t1, t2, ...` -- call the nth subroutine.
    ///
    /// Separate from `OnGoto` rather than a flag on it, because the generated
    /// code differs in more than the jump: there is a return address to push,
    /// and it must *not* be pushed when the selector matches nothing.
    OnGosub {
        expr: Expr,
        targets: Vec<GotoTarget>,
    },
    Dim {
        decls: Vec<Declarator>,
    },
    /// `REDIM [PRESERVE] A(bounds)` -- resize an existing array.
    Redim {
        decls: Vec<Declarator>,
        /// Keep the existing contents (only the last dimension may change).
        preserve: bool,
    },
    Sub {
        name: String,
        params: Vec<Param>,
        body: Vec<Stmt>,
    },
    Function {
        name: String,
        params: Vec<Param>,
        /// Result type from an `AS` clause, when one was given; otherwise the
        /// type comes from the name's suffix.
        ret_ty: Option<TypeRef>,
        body: Vec<Stmt>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    /// `MID$(s, start [, len]) = value` -- overwrite characters in place.
    MidAssign {
        target: LValue,
        start: Expr,
        len: Option<Expr>,
        value: Expr,
    },
    /// `SWAP a, b` -- exchange two values of the same type class.
    Swap(LValue, LValue),
    /// `CONST N = expr` -- a compile-time constant.
    Const {
        name: String,
        value: Expr,
    },
    /// `EXIT FOR` / `EXIT DO` -- leave the innermost matching loop.
    ExitLoop {
        is_for: bool,
    },
    /// `EXIT SUB` / `EXIT FUNCTION` -- return from the current procedure.
    ExitProc,
    /// `OPTION BASE 0|1` -- lowest subscript for arrays declared after it.
    OptionBase(i64),
    /// `TYPE name ... END TYPE` -- a user-defined record type.
    TypeDef {
        name: String,
        fields: Vec<FieldDecl>,
    },
    /// `v.field = value` -- assign to a record field.
    FieldAssign {
        target: LValue,
        value: Expr,
    },
    Data(Vec<Literal>),
    Read(Vec<LValue>),
    /// `DEFINT A-Z` and friends -- the default type for unsuffixed names whose
    /// first letter falls in one of the ranges.
    ///
    /// Held as (first, last) inclusive letter pairs, already upper-cased.
    DefType {
        ty: DataType,
        ranges: Vec<(char, char)>,
    },
    /// `BEEP` -- ring the terminal bell.
    Beep,
    /// `ERASE a, b` -- release arrays so they can be dimensioned again.
    Erase(Vec<String>),
    /// `ERROR n` -- raise the error GW-BASIC numbers `n`.
    RaiseError(Expr),
    /// `ON ERROR GOTO n` -- install an error handler; `None` is `GOTO 0`,
    /// which removes it and puts the fatal path back.
    OnError(Option<GotoTarget>),
    /// `RESUME`, `RESUME NEXT`, `RESUME n` -- return from a handler.
    Resume(ResumeTarget),
    /// `LOCATE [row][, col]` -- move the cursor.
    ///
    /// Either part may be omitted, in which case that coordinate is left where
    /// it is; GW-BASIC also takes cursor-shape arguments, which have no meaning
    /// on a terminal and are refused.
    Locate {
        row: Option<Expr>,
        col: Option<Expr>,
    },
    /// `COLOR fg[, bg]` -- set the text colours.
    Color {
        fg: Option<Expr>,
        bg: Option<Expr>,
    },
    /// `RANDOMIZE [expr]` -- reseed the random number generator.
    ///
    /// GW-BASIC prompts for a seed when none is given; a compiled program has
    /// nobody to prompt, so `None` means "seed from the clock".
    Randomize(Option<Expr>),
    Restore(Option<GotoTarget>),
    Cls,
    SelectCase {
        expr: Expr,
        /// `None` marks `CASE ELSE`; otherwise the alternatives, any of which
        /// may match.
        cases: Vec<(Option<Vec<CaseClause>>, Vec<Stmt>)>,
    },
    End,
    Stop,
    // File I/O
    Open {
        filename: Expr,
        mode: FileMode,
        file_num: Expr,
        /// `LEN = n` on a `FOR RANDOM` open; the record length in bytes.
        /// `None` takes GW-BASIC's default of 128.
        reclen: Option<Expr>,
    },
    /// `CLOSE #n`, or bare `CLOSE` to close every open file.
    Close {
        file_num: Option<Expr>,
    },
    /// `FIELD #n, w AS v$, ...` -- name slices of a random file's record buffer.
    ///
    /// Each target is bound to (buffer + offset, width) rather than to a copy,
    /// so `GET` refreshes every field variable at once and `LSET`/`RSET` write
    /// straight into the buffer that `PUT` will emit.
    Field {
        file_num: Expr,
        fields: Vec<FieldSlice>,
    },
    /// `LSET v$ = expr` / `RSET v$ = expr` -- overwrite a field in place,
    /// padding with spaces to the field's width.
    SetField {
        target: LValue,
        value: Expr,
        /// `RSET` right-justifies; `LSET` left-justifies.
        right: bool,
    },
    /// `GET #n[, rec]` / `PUT #n[, rec]` -- move one record between the file
    /// and its buffer. Without a record number the next one is used.
    GetPut {
        file_num: Expr,
        record: Option<Expr>,
        is_put: bool,
    },
    /// `LOCK`/`UNLOCK #n[, start [TO end]]` -- advisory record locking.
    Lock {
        file_num: Expr,
        /// `None` locks the whole file.
        range: Option<(Expr, Option<Expr>)>,
        is_unlock: bool,
    },
}

/// One `w AS v$` clause of a FIELD statement.
#[derive(Debug, Clone)]
pub struct FieldSlice {
    pub width: Expr,
    pub target: LValue,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileMode {
    Input,
    Output,
    Append,
    Random,
}

#[derive(Debug, Clone)]
pub enum PrintItem {
    Expr(Expr),
    Tab,   // comma = tab to next zone
    Empty, // semicolon = no separator
}

/// A declared type: one of the built-ins, or a user-defined record.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeRef {
    Integer,
    Long,
    Single,
    Double,
    /// `STRING * n` -- a fixed-length string field.
    FixedString(usize),
    /// A user-defined TYPE, by name.
    Record(String),
}

/// One alternative within a `CASE`.
#[derive(Debug, Clone)]
pub enum CaseClause {
    /// `CASE 1`
    Value(Expr),
    /// `CASE 1 TO 10`, inclusive at both ends
    Range(Expr, Expr),
    /// `CASE IS > 100`
    Compare(BinaryOp, Expr),
}

/// One declarator in a `DIM` or `REDIM` list.
///
/// A single statement may mix forms -- `DIM A(3), B AS INTEGER` -- so the
/// list is heterogeneous rather than the parser bailing out to a different
/// statement the moment it sees `AS`.
#[derive(Debug, Clone)]
pub struct Declarator {
    pub name: String,
    /// Array bounds, when this declares an array.
    pub dimensions: Option<Vec<Expr>>,
    /// Declared type from an `AS` clause.
    pub ty: Option<TypeRef>,
}

/// One procedure parameter.
#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    /// Declared type from an `AS` clause; `None` means the type comes from the
    /// name's suffix, as it always has.
    pub ty: Option<TypeRef>,
}

/// One field of a user-defined TYPE.
#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub name: String,
    pub ty: TypeRef,
}

/// A target that can be assigned to: a variable, or one array element.
///
/// Introduced so that statements which read into a variable -- INPUT, LINE
/// INPUT, INPUT #, READ, and now SWAP -- can also target an array element.
/// They previously held a bare `String`, so `INPUT A(3)` and `READ A(I)` were
/// impossible to express.
#[derive(Debug, Clone)]
pub struct LValue {
    pub name: String,
    /// Subscripts, when the target is an array element.
    pub indices: Option<Vec<Expr>>,
    /// Record field path, e.g. `p.origin.x` gives ["ORIGIN", "X"].
    pub fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ArrayDecl {
    pub name: String,
    pub dimensions: Vec<Expr>,
}

/// Where a `RESUME` goes back to.
#[derive(Debug, Clone)]
pub enum ResumeTarget {
    /// Bare `RESUME`, and `RESUME 0`: retry the statement that failed.
    Same,
    /// `RESUME NEXT`: carry on at the statement after it.
    Next,
    /// `RESUME n`: carry on somewhere else entirely.
    At(GotoTarget),
}

#[derive(Debug, Clone)]
pub enum GotoTarget {
    Line(u32),
    Label(String),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Literal),
    Variable(String),
    ArrayAccess {
        name: String,
        indices: Vec<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    FnCall {
        name: String,
        args: Vec<Expr>,
    },
    /// Record field access: `v.field`
    Field {
        base: Box<Expr>,
        field: String,
    },
}

#[derive(Debug, Clone)]
pub enum Literal {
    Integer(i64),
    Float(f64),
    String(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    IntDiv,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    Xor,
    /// Bitwise equivalence: `NOT (a XOR b)`.
    Eqv,
    /// Bitwise implication: `(NOT a) OR b`.
    Imp,
}

/// BASIC data types following GW-BASIC/QuickBASIC conventions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataType {
    Integer, // % - 16-bit signed (i16)
    Long,    // & - 32-bit signed (i32)
    Single,  // ! - 32-bit float (f32)
    Double,  // # - 64-bit float (f64) - DEFAULT for unsuffixed
    String,  // $ - heap-allocated string
}

impl DataType {
    /// Determine type from variable name suffix
    pub fn from_suffix(name: &str) -> DataType {
        match name.chars().last() {
            Some('%') => DataType::Integer,
            Some('&') => DataType::Long,
            Some('!') => DataType::Single,
            Some('#') => DataType::Double,
            Some('$') => DataType::String,
            _ => DataType::Double, // DEFAULT for unsuffixed variables
        }
    }

    /// The storage class of a declared type.
    ///
    /// A record has no scalar class of its own; callers that can encounter one
    /// check for `TypeRef::Record` before asking.
    pub fn from_type_ref(ty: &TypeRef) -> DataType {
        match ty {
            TypeRef::Integer => DataType::Integer,
            TypeRef::Long => DataType::Long,
            TypeRef::Single => DataType::Single,
            TypeRef::Double => DataType::Double,
            TypeRef::FixedString(_) => DataType::String,
            TypeRef::Record(_) => DataType::Double,
        }
    }

    /// Check if this is an integer type (Integer or Long)
    pub fn is_integer(&self) -> bool {
        matches!(self, DataType::Integer | DataType::Long)
    }
}

/// Every nested statement body directly contained by `stmt`.
///
/// This is the single place that knows which `Stmt` variants carry child
/// statements. Any pass that walks the AST must go through it, so that adding a
/// block-bearing statement cannot silently leave a walker behind: omitting
/// `SelectCase` here previously caused `GOSUB` inside a `SELECT CASE` to fail to
/// link and `DATA` inside one to be dropped.
pub fn child_bodies(stmt: &Stmt) -> Vec<&[Stmt]> {
    match &stmt.kind {
        StmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            let mut v = vec![then_branch.as_slice()];
            if let Some(eb) = else_branch {
                v.push(eb.as_slice());
            }
            v
        }
        StmtKind::SelectCase { cases, .. } => {
            cases.iter().map(|(_, body)| body.as_slice()).collect()
        }
        StmtKind::For { body, .. }
        | StmtKind::While { body, .. }
        | StmtKind::DoLoop { body, .. }
        | StmtKind::Sub { body, .. }
        | StmtKind::Function { body, .. } => vec![body.as_slice()],
        _ => vec![],
    }
}

// Parse errors and block terminators

/// A block-closing keyword, consumed by `parse_statement` on behalf of the
/// enclosing block parser.
///
/// BASIC's block terminators are statements syntactically, but they belong to
/// the construct that opened the block, so `parse_statement` hands the matching
/// one back to the enclosing parser (`parse_if_body`, `parse_for`, ...) as a
/// normal end-of-body. Any terminator that reaches the top level without a
/// matching opener becomes a diagnostic there.
///
/// This used to travel through the error channel as `ParseError::Block`, which
/// worked but meant `?` could not be trusted: every `?` in the parser might be
/// propagating an ordinary end-of-block rather than a failure, and a stray
/// terminator would be caught by whichever enclosing block parser matched it
/// first. It is now part of [`Parsed`], so `?` carries only real errors.
///
/// Conditions travel as payloads rather than through parser fields, so a
/// terminator cannot be separated from its expression.
#[derive(Debug, Clone)]
pub enum BlockEnd {
    EndIf,
    EndSub,
    EndFunction,
    EndSelect,
    /// `NEXT [var [, var]...]`. The names are carried rather than discarded so
    /// that `FOR I ... NEXT J` can be refused and `NEXT J, I` can close both
    /// loops. Empty means a bare `NEXT`, which closes the innermost loop.
    Next(Vec<String>),
    Wend,
    Loop,
    LoopWhile(Expr),
    LoopUntil(Expr),
    Else,
    ElseIf(Expr),
}

impl BlockEnd {
    /// The source keyword, for diagnostics about an unmatched terminator.
    fn keyword(&self) -> &'static str {
        match self {
            BlockEnd::EndIf => "END IF",
            BlockEnd::EndSub => "END SUB",
            BlockEnd::EndFunction => "END FUNCTION",
            BlockEnd::EndSelect => "END SELECT",
            BlockEnd::Next(_) => "NEXT",
            BlockEnd::Wend => "WEND",
            BlockEnd::Loop | BlockEnd::LoopWhile(_) | BlockEnd::LoopUntil(_) => "LOOP",
            BlockEnd::Else => "ELSE",
            BlockEnd::ElseIf(_) => "ELSEIF",
        }
    }

    /// The construct this terminator closes, for the "without matching X" hint.
    fn opener(&self) -> &'static str {
        match self {
            BlockEnd::EndIf | BlockEnd::Else | BlockEnd::ElseIf(_) => "IF",
            BlockEnd::EndSub => "SUB",
            BlockEnd::EndFunction => "FUNCTION",
            BlockEnd::EndSelect => "SELECT CASE",
            BlockEnd::Next(_) => "FOR",
            BlockEnd::Wend => "WHILE",
            BlockEnd::Loop | BlockEnd::LoopWhile(_) | BlockEnd::LoopUntil(_) => "DO",
        }
    }
}

/// The result of parsing one statement: the statement, or the terminator that
/// closed the block it was in.
///
/// Generic over the item so that `parse_statement_kind` (which yields a
/// `StmtKind`) and `parse_statement` (which tags it with a line to make a
/// `Stmt`) can share one type.
#[derive(Debug, Clone)]
pub enum Parsed<T> {
    Item(T),
    End(BlockEnd),
}

/// Why parsing of a statement stopped.
#[derive(Debug, Clone)]
pub enum ParseError {
    /// A syntax error at the parser's current position.
    Error(String),
    /// A syntax error belonging to an earlier line -- the opener of a block
    /// that was never closed. Without this the diagnostic lands on end of file,
    /// which is where the parser noticed rather than where the mistake is.
    ErrorAt(u32, String),
}

impl ParseError {
    /// The line this error belongs to, if it names one of its own.
    fn line(&self) -> Option<u32> {
        match self {
            ParseError::Error(_) => None,
            ParseError::ErrorAt(line, _) => Some(*line),
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Error(msg) | ParseError::ErrorAt(_, msg) => write!(f, "{}", msg),
        }
    }
}

/// A parse error together with the source line it was found on.
#[derive(Debug, Clone)]
pub struct LocatedParseError {
    /// Source line, or 0 when the line is unknown.
    pub line: u32,
    pub error: ParseError,
}

impl std::fmt::Display for LocatedParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error)
    }
}

/// How a token is written in BASIC source, for diagnostics.
///
/// Every fixed token has a spelling, so a diagnostic can quote what the
/// programmer would have typed. Errors used to fall back to `{:?}` and show
/// Rust variant names instead -- "Expected To, got Integer(2)" rather than
/// "expected TO, got 2", and `EndSelect`, `LParen` and `Ne` at people who had
/// written `END SELECT`, `(` and `<>`.
fn token_spelling(tok: &Token) -> Option<&'static str> {
    Some(match tok {
        Token::Print => "PRINT",
        Token::Input => "INPUT",
        Token::Line => "LINE",
        Token::Let => "LET",
        Token::Dim => "DIM",
        Token::If => "IF",
        Token::Then => "THEN",
        Token::Else => "ELSE",
        Token::ElseIf => "ELSEIF",
        Token::EndIf => "ENDIF",
        Token::For => "FOR",
        Token::To => "TO",
        Token::Step => "STEP",
        Token::Next => "NEXT",
        Token::While => "WHILE",
        Token::Wend => "WEND",
        Token::Do => "DO",
        Token::Loop => "LOOP",
        Token::Until => "UNTIL",
        Token::Goto => "GOTO",
        Token::Gosub => "GOSUB",
        Token::Return => "RETURN",
        Token::On => "ON",
        Token::Sub => "SUB",
        Token::EndSub => "ENDSUB",
        Token::Function => "FUNCTION",
        Token::EndFunction => "ENDFUNCTION",
        Token::Select => "SELECT",
        Token::Case => "CASE",
        Token::EndSelect => "ENDSELECT",
        Token::End => "END",
        Token::Stop => "STOP",
        Token::DataText(_) => "DATA",
        Token::Read => "READ",
        Token::DefType(w) => match w {
            DataTypeWord::Integer => "DEFINT",
            DataTypeWord::Long => "DEFLNG",
            DataTypeWord::Single => "DEFSNG",
            DataTypeWord::Double => "DEFDBL",
            DataTypeWord::String => "DEFSTR",
        },
        Token::Beep => "BEEP",
        Token::System => "SYSTEM",
        Token::Locate => "LOCATE",
        Token::Color => "COLOR",
        Token::Randomize => "RANDOMIZE",
        Token::Resume => "RESUME",
        Token::Restore => "RESTORE",
        Token::Cls => "CLS",
        Token::Open => "OPEN",
        Token::Close => "CLOSE",
        Token::As => "AS",
        Token::Output => "OUTPUT",
        Token::Append => "APPEND",
        Token::And => "AND",
        Token::Or => "OR",
        Token::Not => "NOT",
        Token::Xor => "XOR",
        Token::Eqv => "EQV",
        Token::Imp => "IMP",
        Token::Mod => "MOD",
        Token::Using => "USING",
        Token::Swap => "SWAP",
        Token::Const => "CONST",
        Token::Write => "WRITE",
        Token::Exit => "EXIT",
        Token::Def => "DEF",
        Token::Option => "OPTION",
        Token::Base => "BASE",
        Token::Redim => "REDIM",
        Token::Preserve => "PRESERVE",
        Token::Type => "TYPE",
        Token::EndType => "ENDTYPE",
        Token::Plus => "+",
        Token::Minus => "-",
        Token::Star => "*",
        Token::Slash => "/",
        Token::Backslash => "\\",
        Token::Caret => "^",
        Token::Eq => "=",
        Token::Ne => "<>",
        Token::Lt => "<",
        Token::Gt => ">",
        Token::Le => "<=",
        Token::Ge => ">=",
        Token::LParen => "(",
        Token::RParen => ")",
        Token::Comma => ",",
        Token::Semicolon => ";",
        Token::Colon => ":",
        Token::Hash => "#",
        Token::Dot => ".",
        _ => return None,
    })
}

/// Human-readable name for a token, for diagnostics.
fn describe_token(tok: &Token) -> String {
    match tok {
        Token::Ident(n) => format!("identifier '{}'", n),
        Token::Integer(n) => format!("{}", n),
        Token::Float(f) => format!("{}", f),
        Token::String(s) => format!("string \"{}\"", s),
        Token::Newline => "end of line".to_string(),
        Token::Eof => "end of file".to_string(),
        Token::LineNumber(n) => format!("line number {}", n),
        other => token_spelling(other)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{:?}", other)),
    }
}

/// One item of a `DATA` statement, as written.
struct DataItem {
    text: String,
    /// Whether it was written in quotes, which decides both whether the
    /// surrounding spaces were significant and whether it is a string.
    quoted: bool,
}

impl DataItem {
    /// The value this item denotes.
    ///
    /// A quoted item is always a string. An unquoted one is whatever it looks
    /// like: GW-BASIC decides the type when the item is READ, but the table this
    /// compiles to is tagged per item, and the runtime already converts a string
    /// entry to a number with `strtod` -- so classifying here loses nothing and
    /// keeps numeric DATA on the fast path.
    ///
    /// An omitted item is the empty string, which reads as 0 or as "".
    fn literal(&self) -> Literal {
        if self.quoted {
            return Literal::String(self.text.clone());
        }
        if let Ok(n) = self.text.parse::<i32>() {
            return Literal::Integer(n as i64);
        }
        if let Ok(f) = self.text.parse::<f64>() {
            if f.is_finite() {
                return Literal::Float(f);
            }
        }
        Literal::String(self.text.clone())
    }
}

/// Split a `DATA` operand into its items.
///
/// Items are separated by commas outside quotes. An unquoted item has its
/// surrounding spaces trimmed; a quoted one keeps everything between the quotes,
/// which is why quotes are needed for an item containing a comma, a colon, or
/// spaces that matter. A doubled `""` inside quotes is one quote character, as
/// everywhere else in the language.
fn split_data_items(text: &str) -> Vec<DataItem> {
    // `DATA` with nothing after it declares no items at all, as against
    // `DATA ,` which declares two empty ones.
    if text.trim().is_empty() {
        return Vec::new();
    }

    let mut items = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.peek() == Some(&' ') || chars.peek() == Some(&'\t') {
            chars.next();
        }

        let item = if chars.peek() == Some(&'"') {
            chars.next();
            let mut body = String::new();
            while let Some(c) = chars.next() {
                if c == '"' {
                    // A doubled quote is one quote character, as everywhere
                    // else in the language.
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        body.push('"');
                        continue;
                    }
                    break;
                }
                body.push(c);
            }
            // Whatever separates the closing quote from the comma is not data.
            while chars.peek().is_some_and(|c| *c != ',') {
                chars.next();
            }
            DataItem {
                text: body,
                quoted: true,
            }
        } else {
            let mut body = String::new();
            while chars.peek().is_some_and(|c| *c != ',') {
                body.push(chars.next().unwrap());
            }
            DataItem {
                text: body.trim().to_string(),
                quoted: false,
            }
        };
        items.push(item);

        match chars.next() {
            Some(',') => continue,
            _ => break,
        }
    }
    items
}

/// Shorthand for the parser's result type.
type PResult<T> = Result<T, ParseError>;

/// Build a syntax error.
fn err<T>(msg: impl Into<String>) -> PResult<T> {
    Err(ParseError::Error(msg.into()))
}

// Parser

#[derive(Default)]
pub struct Parser {
    tokens: Vec<Token>,
    /// Source line of each token, parallel to `tokens`. Empty when unknown.
    lines: Vec<u32>,
    pos: usize,
    /// SUB/FUNCTION names, collected before parsing so that `Name:` at the
    /// start of a line is not mistaken for a label definition.
    declared_procs: HashSet<String>,
    /// How deep the recursive descent currently is, so that pathological input
    /// is refused rather than overflowing the stack. See [`MAX_DEPTH`].
    depth: u32,
    /// Errors found so far. Parsing continues past each one, so a program with
    /// several mistakes reports them all rather than one per compile.
    errors: Vec<LocatedParseError>,
    /// Loop names from a `NEXT I, J` still waiting to close their loops.
    ///
    /// One NEXT may close several nested loops. The innermost `parse_for` takes
    /// the first name and leaves the rest here; each enclosing block body picks
    /// the next one up before reading another token, so the terminator reaches
    /// every loop it names as the recursion unwinds outward.
    pending_next: VecDeque<String>,
}

impl Parser {
    /// Build a parser. `lines` gives each token's source line, and may be
    /// empty when position information is unavailable.
    pub fn new(tokens: Vec<Token>, lines: Vec<u32>) -> Self {
        let declared_procs = Self::scan_proc_names(&tokens);
        Parser {
            tokens,
            lines,
            declared_procs,
            ..Default::default()
        }
    }

    /// Collect the names following SUB and FUNCTION, before parsing.
    ///
    /// Needed up front because a procedure may be defined after the line that
    /// calls it, and `Name:` has to be classified as a label or a call at the
    /// point it is parsed.
    fn scan_proc_names(tokens: &[Token]) -> HashSet<String> {
        let mut names = HashSet::new();
        for pair in tokens.windows(2) {
            if matches!(pair[0], Token::Sub | Token::Function) {
                if let Token::Ident(n) = &pair[1] {
                    names.insert(n.to_uppercase());
                }
            }
        }
        names
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    /// Look `n` tokens past the current one without consuming anything.
    fn peek_at(&self, n: usize) -> &Token {
        self.tokens.get(self.pos + n).unwrap_or(&Token::Eof)
    }

    /// True if the token after the current one is `tok`.
    ///
    /// Used to tell the random-access statements apart from ordinary names:
    /// `FIELD` is only the statement when a `#` follows it.
    fn next_is(&self, tok: Token) -> bool {
        *self.peek_at(1) == tok
    }

    /// True if the token after the current one is any identifier.
    fn next_is_ident(&self) -> bool {
        matches!(self.peek_at(1), Token::Ident(_))
    }

    /// True if something follows the current token for it to take as an operand.
    ///
    /// `ERROR` is a statement only when a number follows it. Left contextual
    /// rather than reserved so that a bare `ERROR` still reaches the
    /// UNSUPPORTED table, which is what keeps `ON ERROR GOTO` refused with a
    /// reason until it is written.
    ///
    /// Phrased as "the statement has not ended" rather than as a list of the
    /// tokens an expression may start with. That list was written out once and
    /// was already missing `NOT` and a string literal, so `ERROR NOT 0` --
    /// a perfectly ordinary GW-BASIC expression -- was refused as though the
    /// statement did not exist. This way anything else is handed to the
    /// expression parser, which either accepts it or says what is wrong with
    /// it, and a token added later needs no edit here.
    fn next_is_an_operand(&self) -> bool {
        !matches!(
            self.peek_at(1),
            Token::Newline | Token::Colon | Token::Eof | Token::Eq
        )
    }

    /// Source line of the current token, or 0 when unknown (no line map).
    fn cur_line(&self) -> u32 {
        self.lines.get(self.pos).copied().unwrap_or(0)
    }

    /// True if the current token begins a logical line, so that `Name:` there
    /// can be a label definition.
    fn at_line_start(&self) -> bool {
        match self.pos.checked_sub(1) {
            None => true,
            Some(prev) => matches!(
                self.tokens.get(prev),
                Some(Token::Newline) | Some(Token::LineNumber(_)) | None
            ),
        }
    }

    /// True if `Name :` at the current position is a label definition.
    ///
    /// `Name:` is ambiguous in principle -- it could be a label, or a call to a
    /// parameterless SUB followed by the `:` statement separator. BASIC resolves
    /// this by position (a label starts a line), and we additionally refuse to
    /// read a declared procedure's name as a label, so `MySub : PRINT "x"`
    /// still calls the procedure.
    fn at_label_definition(&self) -> bool {
        let Token::Ident(name) = self.peek() else {
            return false;
        };
        matches!(self.peek_at(1), Token::Colon)
            && self.at_line_start()
            && !self.declared_procs.contains(&name.to_uppercase())
    }

    /// True if the upcoming tokens close a SELECT CASE.
    ///
    /// Two-token lookahead matters: a bare `END` inside a case body is the
    /// program-termination statement, not the start of `END SELECT`.
    fn at_end_select(&self) -> bool {
        matches!(self.peek(), Token::EndSelect)
            || (matches!(self.peek(), Token::End) && matches!(self.peek_at(1), Token::Select))
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        tok
    }

    /// Consume the next token, which must be `expected`.
    ///
    /// Matching is by variant, so a payload would be ignored -- every caller
    /// passes a payload-free token, and `token_spelling` returning `Some` for
    /// exactly those is what keeps that honest: a payload-carrying token has no
    /// fixed spelling to name in the diagnostic, and is refused here.
    fn expect(&mut self, expected: Token) -> PResult<()> {
        let Some(wanted) = token_spelling(&expected) else {
            unreachable!("expect takes a token with a fixed spelling")
        };
        let tok = self.advance();
        if std::mem::discriminant(&tok) == std::mem::discriminant(&expected) {
            Ok(())
        } else {
            err(format!("expected {}, got {}", wanted, describe_token(&tok)))
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline) {
            self.advance();
        }
    }

    /// Parse the whole program, reporting every syntax error it contains.
    ///
    /// Sema has always returned a `Vec<Diagnostic>`, so a program with five
    /// undefined names is told about all five. The parser stopped at the first
    /// error, so five typos meant five compiles. It now recovers in the same
    /// way and reports the same way.
    pub fn parse(&mut self) -> Result<Program, Vec<LocatedParseError>> {
        let program = match self.parse_program() {
            Ok(p) => p,
            // A hard error -- one recovery could not get past, such as an
            // unterminated block -- ends the parse, but whatever was already
            // collected is still worth reporting alongside it.
            Err(error) => {
                self.record(error);
                return Err(std::mem::take(&mut self.errors));
            }
        };
        if self.errors.is_empty() {
            Ok(program)
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }

    /// Note an error, giving it a line if it does not name one of its own.
    fn record(&mut self, error: ParseError) {
        let line = error.line().unwrap_or_else(|| self.cur_line());
        self.errors.push(LocatedParseError { line, error });
    }

    /// Skip to the next place a statement could begin.
    ///
    /// Panic-mode recovery. BASIC is line-oriented, which makes this unusually
    /// reliable: a newline or a colon ends a statement no matter what went
    /// wrong before it, so the parser can pick up with the next one instead of
    /// abandoning the file. Stopping *before* the boundary rather than past it
    /// leaves any block terminator on that line to be read normally, so one bad
    /// statement inside a SUB does not also cost the END SUB.
    fn synchronize(&mut self) {
        // Whatever a half-parsed statement left queued is meaningless now, and
        // a stale name would surface as a terminator somewhere unrelated.
        self.pending_next.clear();

        let start = self.pos;
        while !matches!(self.peek(), Token::Newline | Token::Colon | Token::Eof) {
            self.advance();
        }
        // If the error was already at a boundary, step over it: the caller's
        // loop would otherwise see the same token again and spin.
        if self.pos == start && !matches!(self.peek(), Token::Eof) {
            self.advance();
        }
    }

    fn parse_program(&mut self) -> PResult<Program> {
        let mut statements = Vec::new();
        self.skip_newlines();

        while !matches!(self.peek(), Token::Eof) || !self.pending_next.is_empty() {
            // A name left over from `NEXT I, J` has no loop to close: more
            // loops were named than were open. Draining it here rather than
            // only inside the loop condition matters, because the commonest
            // shape -- `FOR I .. NEXT I, J` as the last statement -- leaves the
            // token stream at Eof with the name still queued.
            if let Some(name) = self.pending_next.pop_front() {
                let e = ParseError::Error(format!("NEXT {} without matching FOR", name));
                self.record(e);
                continue;
            }
            match self.parse_statement() {
                Ok(Parsed::Item(stmt)) => statements.push(stmt),
                // A terminator here closed nothing: there is no enclosing block
                // for it to belong to.
                Ok(Parsed::End(end)) => {
                    // Once anything has gone wrong, a stray terminator says
                    // nothing useful: it is usually the perfectly good closer
                    // of a block whose *header* failed, so reporting it blames
                    // a FOR that is sitting right there. Suppress after the
                    // first error, report it in a clean program.
                    if self.errors.is_empty() {
                        let e = ParseError::Error(format!(
                            "{} without matching {}",
                            end.keyword(),
                            end.opener()
                        ));
                        self.record(e);
                    }
                    self.synchronize();
                }
                Err(e) => {
                    self.record(e);
                    self.synchronize();
                }
            }
            self.skip_newlines();
        }

        Ok(Program { statements })
    }

    /// Parse the statements of a block, up to and including its terminator.
    ///
    /// Every block-bearing construct shares this. `opener` names the construct
    /// and `closer` the keyword it needs, for the diagnostic when end of file
    /// arrives first -- none of the seven hand-written loops this replaces
    /// checked for EOF at all. They terminated only because an unrecognised
    /// token became "Unexpected token: Eof", so an unterminated SUB blamed the
    /// last line of the file and named nothing.
    fn parse_block_body(
        &mut self,
        opener: &str,
        closer: &str,
        opener_line: u32,
    ) -> PResult<(Vec<Stmt>, BlockEnd)> {
        let mut body = Vec::new();
        loop {
            // A `NEXT I, J` left names here for the enclosing loops. Take one
            // before looking at the token stream at all -- the NEXT has already
            // been consumed, so the next real token is whatever followed it,
            // and at the end of a program that is Eof. Checking after the guard
            // below would report "FOR is missing its NEXT" with the terminator
            // sitting right there.
            if let Some(name) = self.pending_next.pop_front() {
                return Ok((body, BlockEnd::Next(vec![name])));
            }
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::ErrorAt(
                    opener_line,
                    format!("{} is missing its {}", opener, closer),
                ));
            }
            match self.parse_statement() {
                Ok(Parsed::Item(stmt)) => body.push(stmt),
                Ok(Parsed::End(end)) => return Ok((body, end)),
                // Recover here as well as at the top level, so a mistake inside
                // a block costs that statement rather than the whole block --
                // otherwise the error would propagate out, the block's own
                // terminator would be left stranded, and the reader would get a
                // cascade of complaints about a SUB that was perfectly closed.
                Err(e) => {
                    self.record(e);
                    self.synchronize();
                }
            }
            self.skip_newlines();
        }
    }

    /// A block body that must end with exactly one terminator, named by `want`.
    ///
    /// `want` is matched by variant, so a payload-free value stands in for the
    /// whole family; it also supplies the keyword for both diagnostics.
    fn parse_block(
        &mut self,
        want: BlockEnd,
        opener: &str,
        opener_line: u32,
    ) -> PResult<Vec<Stmt>> {
        let (body, end) = self.parse_block_body(opener, want.keyword(), opener_line)?;
        if std::mem::discriminant(&end) != std::mem::discriminant(&want) {
            return Err(ParseError::ErrorAt(
                opener_line,
                format!(
                    "{} needs {} to close it, but {} came first",
                    opener,
                    want.keyword(),
                    end.keyword()
                ),
            ));
        }
        Ok(body)
    }

    /// Parse one statement, tagging it with the line it started on.
    fn parse_statement(&mut self) -> PResult<Parsed<Stmt>> {
        let line = self.cur_line();
        Ok(match self.parse_statement_kind()? {
            Parsed::Item(kind) => Parsed::Item(Stmt { line, kind }),
            Parsed::End(end) => Parsed::End(end),
        })
    }

    /// Parse one statement that is known not to close a block.
    ///
    /// Used where a terminator would be meaningless -- the branches of a
    /// single-line IF -- so the caller does not have to invent a diagnostic.
    fn parse_inner_statement(&mut self) -> PResult<Stmt> {
        match self.parse_statement()? {
            Parsed::Item(stmt) => Ok(stmt),
            Parsed::End(end) => err(format!(
                "{} without matching {}",
                end.keyword(),
                end.opener()
            )),
        }
    }

    fn parse_statement_kind(&mut self) -> PResult<Parsed<StmtKind>> {
        self.depth += 1;
        let r = self.parse_statement_kind_inner();
        self.depth -= 1;
        r
    }

    fn parse_statement_kind_inner(&mut self) -> PResult<Parsed<StmtKind>> {
        if self.depth > MAX_DEPTH {
            return err(format!("nesting is too deep (limit {} levels)", MAX_DEPTH));
        }

        // Skip any run of separators and blank lines before the statement
        // proper. Each of these used to recurse. That is a tail call, so a
        // release build optimized it away and only a debug build overflowed on
        // a long run of colons -- the worst way to hold a bug, since CI runs
        // `cargo test --release`. A loop costs no stack in any profile.
        while matches!(self.peek(), Token::Colon | Token::Newline) {
            self.advance();
        }

        // A block-closing keyword belongs to the construct that opened the
        // block, so hand it back rather than parsing it as a statement.
        if let Some(end) = self.try_block_end()? {
            return Ok(Parsed::End(end));
        }

        // Handle line numbers as labels
        if let Token::LineNumber(n) = self.peek().clone() {
            self.advance();
            return Ok(Parsed::Item(StmtKind::Label(n)));
        }

        // A named label definition: `Retry:` at the start of a line.
        if self.at_label_definition() {
            let Token::Ident(name) = self.advance() else {
                unreachable!("at_label_definition checked for an identifier")
            };
            self.advance(); // consume ':'
            return Ok(Parsed::Item(StmtKind::LabelName(name)));
        }

        let kind = match self.peek().clone() {
            Token::Print => self.parse_print(false),
            Token::Write => self.parse_print(true),
            Token::Swap => self.parse_swap(),
            Token::Const => self.parse_const(),
            Token::Exit => self.parse_exit(),
            Token::Def => self.parse_def_fn(),
            Token::Option => self.parse_option_base(),
            Token::Input => self.parse_input(),
            Token::Line => self.parse_line_input(),
            Token::Let => self.parse_let(),
            Token::If => self.parse_if(),
            Token::For => self.parse_for(),
            Token::While => self.parse_while(),
            Token::Do => self.parse_do_loop(),
            Token::Goto => self.parse_goto(),
            Token::Gosub => self.parse_gosub(),
            Token::Return => {
                self.advance();
                Ok(StmtKind::Return)
            }
            Token::On => self.parse_on_goto(),
            Token::Dim => self.parse_dim(),
            Token::Redim => self.parse_redim(),
            Token::Type => self.parse_type_def(),
            Token::Sub => self.parse_sub(),
            Token::Function => self.parse_function(),
            Token::DataText(text) => self.parse_data(&text),
            Token::Read => self.parse_read(),
            Token::DefType(w) => self.parse_def_type(w),
            Token::Beep => {
                self.advance();
                Ok(StmtKind::Beep)
            }
            // SYSTEM ends the program, which is what END already means.
            Token::System => {
                self.advance();
                Ok(StmtKind::End)
            }
            Token::Locate => self.parse_locate(),
            Token::Color => self.parse_color(),
            Token::Randomize => self.parse_randomize(),
            Token::Resume => self.parse_resume(),
            Token::Restore => self.parse_restore(),
            Token::Cls => {
                self.advance();
                Ok(StmtKind::Cls)
            }
            Token::Open => self.parse_open(),
            Token::Close => self.parse_close(),
            // `try_block_end` has already taken END followed by IF, SUB,
            // FUNCTION or SELECT, so a bare END is the statement.
            Token::End => {
                self.advance();
                Ok(StmtKind::End)
            }
            Token::Stop => {
                self.advance();
                Ok(StmtKind::Stop)
            }
            Token::Select => self.parse_select_case(),
            // `parse_select_case` consumes CASE itself, so a CASE reaching here
            // is always outside any SELECT CASE.
            Token::Case => err("CASE without matching SELECT CASE"),
            Token::Ident(ref n) => {
                // The random-access statements lead with a name rather than a
                // reserved word. Each is recognised only in a shape that an
                // assignment or a call could not take -- `FIELD #`, `LSET v =`
                // -- so programs may still use these names for their own
                // variables and procedures.
                match n.to_uppercase().as_str() {
                    "FIELD" if self.next_is(Token::Hash) => self.parse_field(),
                    "GET" if self.next_is(Token::Hash) => self.parse_get_put(false),
                    "PUT" if self.next_is(Token::Hash) => self.parse_get_put(true),
                    "LOCK" if self.next_is(Token::Hash) => self.parse_lock(false),
                    "UNLOCK" if self.next_is(Token::Hash) => self.parse_lock(true),
                    "CALL" if self.next_is_ident() => self.parse_call(),
                    "ERASE" if self.next_is_ident() => self.parse_erase(),
                    "ERROR" if self.next_is_an_operand() => self.parse_raise_error(),
                    "LSET" if self.next_is_ident() => self.parse_set_field(false),
                    "RSET" if self.next_is_ident() => self.parse_set_field(true),
                    _ => self.parse_assignment_or_call(),
                }
            }
            // Newline and Colon are consumed by the skip loop above, so
            // reaching here means the statement itself is unrecognised.
            _ => err(format!(
                "unexpected {} at the start of a statement",
                describe_token(self.peek())
            )),
        }?;
        Ok(Parsed::Item(kind))
    }

    /// Consume a block-closing keyword if one is next, leaving the position
    /// untouched otherwise.
    ///
    /// `END` is the awkward one: alone it terminates the program, and only the
    /// token after it decides. `NEXT` swallows its optional control variable,
    /// and `LOOP`/`ELSEIF` carry the condition they were written with, so that
    /// a terminator can never be separated from its expression.
    fn try_block_end(&mut self) -> PResult<Option<BlockEnd>> {
        let end = match self.peek().clone() {
            Token::End => {
                let closes = match self.peek_at(1) {
                    Token::If => BlockEnd::EndIf,
                    Token::Sub => BlockEnd::EndSub,
                    Token::Function => BlockEnd::EndFunction,
                    Token::Select => BlockEnd::EndSelect,
                    // A bare END is the program-termination statement.
                    _ => return Ok(None),
                };
                self.advance();
                self.advance();
                closes
            }
            Token::EndIf => {
                self.advance();
                BlockEnd::EndIf
            }
            Token::EndSub => {
                self.advance();
                BlockEnd::EndSub
            }
            Token::EndFunction => {
                self.advance();
                BlockEnd::EndFunction
            }
            Token::EndSelect => {
                self.advance();
                BlockEnd::EndSelect
            }
            Token::Next => {
                self.advance();
                // `NEXT`, `NEXT I` or `NEXT I, J, ...`. The names used to be
                // swallowed and discarded, so a NEXT could close a loop it did
                // not name and a list was a syntax error.
                let mut names = Vec::new();
                while let Token::Ident(name) = self.peek().clone() {
                    self.advance();
                    names.push(name);
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                BlockEnd::Next(names)
            }
            Token::Wend => {
                self.advance();
                BlockEnd::Wend
            }
            Token::Loop => {
                self.advance();
                match self.peek() {
                    Token::While => {
                        self.advance();
                        BlockEnd::LoopWhile(self.parse_expression()?)
                    }
                    Token::Until => {
                        self.advance();
                        BlockEnd::LoopUntil(self.parse_expression()?)
                    }
                    _ => BlockEnd::Loop,
                }
            }
            Token::Else => {
                self.advance();
                BlockEnd::Else
            }
            Token::ElseIf => {
                self.advance();
                let cond = self.parse_expression()?;
                self.expect(Token::Then)?;
                BlockEnd::ElseIf(cond)
            }
            _ => return Ok(None),
        };
        Ok(Some(end))
    }

    fn parse_print(&mut self, write: bool) -> PResult<StmtKind> {
        self.advance(); // consume PRINT or WRITE

        // Check for PRINT #n (file output)
        let file_num = if matches!(self.peek(), Token::Hash) {
            let num = self.parse_file_number()?;
            if matches!(self.peek(), Token::Comma) {
                self.advance(); // consume comma after file number
            }
            Some(num)
        } else {
            None
        };

        // PRINT USING "fmt"; items
        let using = if matches!(self.peek(), Token::Using) {
            self.advance();
            let fmt = self.parse_expression()?;
            // GW-BASIC separates the format from the values with ; or ,
            if matches!(self.peek(), Token::Semicolon | Token::Comma) {
                self.advance();
            }
            Some(fmt)
        } else {
            None
        };

        let mut items = Vec::new();
        let mut newline = true;

        while !matches!(
            self.peek(),
            Token::Newline | Token::Colon | Token::Eof | Token::Else
        ) {
            if matches!(self.peek(), Token::Semicolon) {
                self.advance();
                items.push(PrintItem::Empty);
                newline = false;
            } else if matches!(self.peek(), Token::Comma) {
                self.advance();
                items.push(PrintItem::Tab);
                newline = false;
            } else {
                let expr = self.parse_expression()?;
                items.push(PrintItem::Expr(expr));
                newline = true;
            }
        }

        // WRITE always ends its line, so a trailing separator does not
        // suppress the newline the way it does for PRINT.
        let newline = newline || write;

        Ok(StmtKind::Print {
            file_num,
            items,
            newline,
            using,
            write,
        })
    }

    /// `SWAP a, b`
    fn parse_swap(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume SWAP
        let a = self.parse_lvalue()?;
        self.expect(Token::Comma)?;
        let b = self.parse_lvalue()?;
        Ok(StmtKind::Swap(a, b))
    }

    /// `CONST NAME = expr`
    fn parse_const(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume CONST
        let name = if let Token::Ident(n) = self.advance() {
            n
        } else {
            return err("Expected a name after CONST");
        };
        self.expect(Token::Eq)?;
        let value = self.parse_expression()?;
        Ok(StmtKind::Const { name, value })
    }

    /// `EXIT FOR` / `EXIT DO` / `EXIT SUB` / `EXIT FUNCTION`
    fn parse_exit(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume EXIT
        match self.advance() {
            Token::For => Ok(StmtKind::ExitLoop { is_for: true }),
            Token::Do => Ok(StmtKind::ExitLoop { is_for: false }),
            Token::Sub | Token::Function => Ok(StmtKind::ExitProc),
            tok => err(format!(
                "Expected FOR, DO, SUB or FUNCTION after EXIT, got {}",
                describe_token(&tok)
            )),
        }
    }

    /// `DEF FNname(params) = expr`
    ///
    /// Desugared straight into an ordinary FUNCTION whose body assigns the
    /// expression to the function name. That reuses the whole procedure path
    /// and, unlike macro substitution, evaluates each argument exactly once.
    fn parse_def_fn(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume DEF
        let name = if let Token::Ident(n) = self.advance() {
            n
        } else {
            return err("Expected a function name after DEF");
        };
        if !name.to_uppercase().starts_with("FN") {
            return err(format!("DEF function name '{}' must begin with FN", name));
        }

        // parse_param_list expects the parenthesis to be consumed already.
        let params = if matches!(self.peek(), Token::LParen) {
            self.advance();
            let p = self.parse_param_list()?;
            self.expect(Token::RParen)?;
            p
        } else {
            Vec::new()
        };

        self.expect(Token::Eq)?;
        let line = self.cur_line();
        let value = self.parse_expression()?;

        Ok(StmtKind::Function {
            name: name.clone(),
            params,
            ret_ty: None,
            body: vec![Stmt {
                line,
                kind: StmtKind::Let {
                    name,
                    indices: None,
                    value,
                },
            }],
        })
    }

    /// `OPTION BASE 0` or `OPTION BASE 1`
    fn parse_option_base(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume OPTION
        self.expect(Token::Base)?;
        match self.advance() {
            Token::Integer(n @ (0 | 1)) => Ok(StmtKind::OptionBase(n)),
            tok => err(format!(
                "OPTION BASE takes 0 or 1, got {}",
                describe_token(&tok)
            )),
        }
    }

    fn parse_input(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume INPUT
        self.skip_input_suppressor();

        // Check for INPUT #n (file input)
        if matches!(self.peek(), Token::Hash) {
            let file_num = self.parse_file_number()?;
            if matches!(self.peek(), Token::Comma) {
                self.advance(); // consume comma after file number
            }

            let mut vars = Vec::new();
            while matches!(self.peek(), Token::Ident(_)) {
                vars.push(self.parse_lvalue()?);
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }

            return Ok(StmtKind::Input {
                prompt: None,
                // A file read prompts for nothing.
                query: false,
                vars,
                file_num: Some(file_num),
            });
        }

        let mut prompt = None;
        let mut vars = Vec::new();
        // A promptless INPUT still asks: GW-BASIC prints a bare `? `.
        let mut query = true;

        // Check for prompt string
        if let Token::String(s) = self.peek().clone() {
            self.advance();
            prompt = Some(s);
            // The separator decides whether a question mark follows the
            // prompt: `;` adds one, `,` suppresses it. Both were accepted and
            // the choice thrown away, so neither form ever printed one.
            match self.peek() {
                Token::Semicolon => {
                    self.advance();
                }
                Token::Comma => {
                    self.advance();
                    query = false;
                }
                _ => {}
            }
        }

        // Read variable names
        while matches!(self.peek(), Token::Ident(_)) {
            vars.push(self.parse_lvalue()?);
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(StmtKind::Input {
            prompt,
            query,
            vars,
            file_num: None,
        })
    }

    /// Consume the optional `;` that may precede an INPUT prompt.
    ///
    /// In GW-BASIC this suppresses the newline echoed when the operator presses
    /// Return, so the next PRINT continues the same line. That newline is the
    /// terminal's echo here -- neither runtime prints one -- so there is nothing
    /// on our side to suppress and the form is accepted and ignored. Rejecting
    /// it outright would refuse a program for asking about a difference it
    /// cannot observe. LANGREF says so.
    fn skip_input_suppressor(&mut self) {
        if matches!(self.peek(), Token::Semicolon) {
            self.advance();
        }
    }

    fn parse_line_input(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume LINE
        self.expect(Token::Input)?;
        self.skip_input_suppressor();

        // LINE INPUT #n, Var$ -- documented in LANGREF but never parsed.
        let file_num = if matches!(self.peek(), Token::Hash) {
            let num = self.parse_file_number()?;
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            }
            Some(num)
        } else {
            None
        };

        let mut prompt = None;

        // Check for prompt string. Unlike INPUT, LINE INPUT never adds a
        // question mark, so the separator carries no meaning here.
        if let Token::String(s) = self.peek().clone() {
            self.advance();
            prompt = Some(s);
            if matches!(self.peek(), Token::Comma | Token::Semicolon) {
                self.advance();
            }
        }

        if !matches!(self.peek(), Token::Ident(_)) {
            return err("Expected variable name after LINE INPUT");
        }
        let var = self.parse_lvalue()?;

        Ok(StmtKind::LineInput {
            prompt,
            var,
            file_num,
        })
    }

    /// Parse an assignment target: a name, optionally subscripted.
    fn parse_lvalue(&mut self) -> PResult<LValue> {
        let name = if let Token::Ident(n) = self.advance() {
            n
        } else {
            return err("Expected variable name");
        };
        let indices = if matches!(self.peek(), Token::LParen) {
            self.advance();
            let args = self.parse_expr_list()?;
            self.expect(Token::RParen)?;
            Some(args)
        } else {
            None
        };
        // A record field path may follow: v.field.sub
        let fields = self.parse_field_path()?;
        Ok(LValue {
            name,
            indices,
            fields,
        })
    }

    fn parse_let(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume LET

        // LET introduces exactly the assignments that may also be written
        // without it, so it shares their parser rather than reimplementing a
        // subset -- which is why `LET Q.X = 3` and `LET MID$(S$,1,2) = "HE"`
        // used to be rejected.
        let stmt = self.parse_assignment_or_call()?;
        if let StmtKind::Call { name, .. } = &stmt {
            return err(format!("LET needs an assignment, but '{}' is a call", name));
        }
        Ok(stmt)
    }

    fn parse_assignment_or_call(&mut self) -> PResult<StmtKind> {
        let name = if let Token::Ident(n) = self.advance() {
            n
        } else {
            return err("Expected identifier");
        };

        // A record field assignment: v.field... = value
        if matches!(self.peek(), Token::Dot) {
            let fields = self.parse_field_path()?;
            self.expect(Token::Eq)?;
            let value = self.parse_expression()?;
            return Ok(StmtKind::FieldAssign {
                target: LValue {
                    name,
                    indices: None,
                    fields,
                },
                value,
            });
        }

        // Check for array subscript or function call
        if matches!(self.peek(), Token::LParen) {
            self.advance();

            // Could be array assignment or subroutine call
            // Look ahead to see if there's an = after )
            let args = self.parse_expr_list()?;
            self.expect(Token::RParen)?;

            // arr(i).field... = value
            if matches!(self.peek(), Token::Dot) {
                let fields = self.parse_field_path()?;
                self.expect(Token::Eq)?;
                let value = self.parse_expression()?;
                return Ok(StmtKind::FieldAssign {
                    target: LValue {
                        name,
                        indices: Some(args),
                        fields,
                    },
                    value,
                });
            }

            if matches!(self.peek(), Token::Eq) {
                self.advance();
                let value = self.parse_expression()?;

                // MID$(s, start [, len]) = value overwrites characters of an
                // existing string rather than assigning an array element.
                if name.to_uppercase() == "MID$" {
                    return self.mid_assign(args, value);
                }

                Ok(StmtKind::Let {
                    name,
                    indices: Some(args),
                    value,
                })
            } else {
                // Subroutine call
                Ok(StmtKind::Call { name, args })
            }
        } else if matches!(self.peek(), Token::Eq) {
            // Simple assignment
            self.advance();
            let value = self.parse_expression()?;
            Ok(StmtKind::Let {
                name,
                indices: None,
                value,
            })
        } else {
            // Subroutine call without parens
            let mut args = Vec::new();
            while !matches!(
                self.peek(),
                Token::Newline | Token::Colon | Token::Eof | Token::Else
            ) {
                args.push(self.parse_expression()?);
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            Ok(StmtKind::Call { name, args })
        }
    }

    /// Build a `MID$(...) = value` statement from its parsed argument list.
    fn mid_assign(&mut self, mut args: Vec<Expr>, value: Expr) -> PResult<StmtKind> {
        if args.len() < 2 || args.len() > 3 {
            return err("MID$ assignment takes a string, a start, and an optional length");
        }
        let len = if args.len() == 3 { args.pop() } else { None };
        let start = args.pop().expect("checked above");
        let target = match args.pop().expect("checked above") {
            Expr::Variable(name) => LValue {
                name,
                indices: None,
                fields: Vec::new(),
            },
            // A subscripted target arrives as FnCall now that the parser no
            // longer guesses which one it is; sema turns the surviving calls
            // into array accesses, but MID$ needs the LValue here.
            Expr::ArrayAccess { name, indices }
            | Expr::FnCall {
                name,
                args: indices,
            } => LValue {
                name,
                indices: Some(indices),
                fields: Vec::new(),
            },
            _ => return err("MID$ assignment requires a string variable"),
        };
        Ok(StmtKind::MidAssign {
            target,
            start,
            len,
            value,
        })
    }

    fn parse_if(&mut self) -> PResult<StmtKind> {
        let if_line = self.cur_line();
        self.advance(); // consume IF
        let condition = self.parse_expression()?;
        self.expect(Token::Then)?;

        // A run of colons after THEN separates no statements, so the line ends
        // there and this is a block IF. Treating them as the start of a
        // statement -- as any non-newline token used to be -- made
        // `IF X = 1 THEN :` a one-line IF whose END IF was then unmatched.
        let mut ahead = 0;
        while matches!(self.peek_at(ahead), Token::Colon) {
            ahead += 1;
        }
        if matches!(self.peek_at(ahead), Token::Newline | Token::Eof) {
            for _ in 0..ahead {
                self.advance();
            }
        }

        // Check for single-line IF
        if !matches!(self.peek(), Token::Newline | Token::Eof) {
            // Single-line IF
            let then_branch = self.parse_single_line_branch()?;

            let else_branch = if matches!(self.peek(), Token::Else) {
                self.advance();
                Some(self.parse_single_line_branch()?)
            } else {
                None
            };

            return Ok(StmtKind::If {
                condition,
                then_branch,
                else_branch,
            });
        }

        // Block IF - parse body, handling ELSEIF as nested IF
        self.skip_newlines();
        let (then_branch, else_branch) = self.parse_if_body(if_line)?;

        Ok(StmtKind::If {
            condition,
            then_branch,
            else_branch,
        })
    }

    /// Parse one branch of a single-line IF: a colon-separated statement list.
    ///
    /// Everything after `THEN` up to `ELSE` or the end of the line is the THEN
    /// clause, and everything after `ELSE` is the ELSE clause. Taking only the
    /// first statement -- as this used to -- let the rest of the line escape
    /// the conditional and run unconditionally, so `IF X = 1 THEN PRINT "A" :
    /// PRINT "B"` printed `B` when X was 0. It also broke the ELSE form
    /// outright: the second statement became a sibling of the IF, leaving the
    /// `ELSE` to reach the top level as "ELSE without matching IF".
    ///
    /// The check for a terminator after the separator is what keeps a trailing
    /// colon from pulling in the next line: `parse_statement` skips a leading
    /// newline, so without it `IF C THEN PRINT "x" :` would swallow the
    /// statement below it.
    fn parse_single_line_branch(&mut self) -> PResult<Vec<Stmt>> {
        let mut body = vec![self.parse_branch_statement()?];
        while matches!(self.peek(), Token::Colon) {
            self.advance();
            if matches!(self.peek(), Token::Else | Token::Newline | Token::Eof) {
                break;
            }
            body.push(self.parse_branch_statement()?);
        }
        Ok(body)
    }

    /// One statement of a single-line IF branch, where a bare line number is
    /// an implied GOTO.
    ///
    /// `IF X < 0 THEN 900` is how GW-BASIC spells its commonest branch, and it
    /// is unambiguous: no other statement may begin with a number, so this used
    /// to be rejected as "Unexpected token: Integer(900)".
    fn parse_branch_statement(&mut self) -> PResult<Stmt> {
        let line = self.cur_line();
        if let Token::Integer(_) | Token::LineNumber(_) = self.peek() {
            let target = self.parse_goto_target()?;
            return Ok(Stmt {
                line,
                kind: StmtKind::Goto(target),
            });
        }
        self.parse_inner_statement()
    }

    /// Parse the body of an IF block, returning (then_branch, else_branch).
    /// Handles ELSEIF by constructing nested IF statements in else_branch.
    ///
    /// `if_line` is the line of the IF or ELSEIF that opened this body, used to
    /// blame the right line when END IF never arrives.
    fn parse_if_body(&mut self, if_line: u32) -> PResult<(Vec<Stmt>, Option<Vec<Stmt>>)> {
        let (body, end) = self.parse_block_body("IF", "END IF", if_line)?;
        // Captured before the newline is skipped, so an ELSEIF's synthesized
        // nested IF is attributed to the ELSEIF's own line: a terminator is
        // followed by the newline that ends the line it was written on.
        let elseif_line = self.cur_line();

        match end {
            BlockEnd::EndIf => Ok((body, None)),
            BlockEnd::Else => {
                self.skip_newlines();
                let else_body = self.parse_block(BlockEnd::EndIf, "IF", if_line)?;
                Ok((body, Some(else_body)))
            }
            BlockEnd::ElseIf(condition) => {
                // The rest of the chain is an IF nested in this one's ELSE.
                self.skip_newlines();
                let (nested_then, nested_else) = self.parse_if_body(if_line)?;
                let nested_if = Stmt {
                    line: elseif_line,
                    kind: StmtKind::If {
                        condition,
                        then_branch: nested_then,
                        else_branch: nested_else,
                    },
                };
                Ok((body, Some(vec![nested_if])))
            }
            other => Err(ParseError::ErrorAt(
                if_line,
                format!(
                    "IF needs END IF to close it, but {} came first",
                    other.keyword()
                ),
            )),
        }
    }

    fn parse_for(&mut self) -> PResult<StmtKind> {
        let for_line = self.cur_line();
        self.advance(); // consume FOR
        let var = if let Token::Ident(n) = self.advance() {
            n
        } else {
            return err("Expected variable name after FOR");
        };

        self.expect(Token::Eq)?;
        let start = self.parse_expression()?;
        self.expect(Token::To)?;
        let end = self.parse_expression()?;

        let step = if matches!(self.peek(), Token::Step) {
            self.advance();
            Some(self.parse_expression()?)
        } else {
            None
        };

        self.skip_newlines();

        // Inspect the terminator rather than letting `parse_block` check only
        // its variant, so the control variable can be matched against this
        // loop's. `parse_do_loop` and `parse_if_body` take the same route.
        let (body, terminator) = self.parse_block_body("FOR", "NEXT", for_line)?;
        let BlockEnd::Next(mut names) = terminator else {
            return Err(ParseError::ErrorAt(
                for_line,
                format!(
                    "FOR needs NEXT to close it, but {} came first",
                    terminator.keyword()
                ),
            ));
        };

        // A bare NEXT closes the innermost loop, whichever it is. A named one
        // must name *this* loop: `FOR I ... NEXT J` used to compile, and with
        // two loops open it silently produced a nesting nobody wrote.
        if !names.is_empty() {
            let closes = names.remove(0);
            if closes != var {
                return Err(ParseError::ErrorAt(
                    for_line,
                    format!("FOR {} is closed by NEXT {}", var, closes),
                ));
            }
            // The rest belong to the loops enclosing this one.
            for name in names.into_iter().rev() {
                self.pending_next.push_front(name);
            }
        }

        Ok(StmtKind::For {
            var,
            start,
            end,
            step,
            body,
        })
    }

    fn parse_while(&mut self) -> PResult<StmtKind> {
        let while_line = self.cur_line();
        self.advance(); // consume WHILE
        let condition = self.parse_expression()?;
        self.skip_newlines();

        let body = self.parse_block(BlockEnd::Wend, "WHILE", while_line)?;

        Ok(StmtKind::While { condition, body })
    }

    fn parse_do_loop(&mut self) -> PResult<StmtKind> {
        let do_line = self.cur_line();
        self.advance(); // consume DO

        // Check for DO WHILE/UNTIL at start
        let (cond_at_start, is_until, condition) = match self.peek() {
            Token::While => {
                self.advance();
                (true, false, Some(self.parse_expression()?))
            }
            Token::Until => {
                self.advance();
                (true, true, Some(self.parse_expression()?))
            }
            _ => (false, false, None),
        };

        self.skip_newlines();

        let (body, end) = self.parse_block_body("DO", "LOOP", do_line)?;
        let (end_condition, end_is_until) = match end {
            BlockEnd::Loop => (None, false),
            BlockEnd::LoopWhile(cond) => (Some(cond), false),
            BlockEnd::LoopUntil(cond) => (Some(cond), true),
            other => {
                return Err(ParseError::ErrorAt(
                    do_line,
                    format!(
                        "DO needs LOOP to close it, but {} came first",
                        other.keyword()
                    ),
                ));
            }
        };

        // A loop tests at one end or the other. These used to be merged with
        // `condition.or(end_condition)`, which silently discarded the one on
        // the LOOP: `DO WHILE I < 3 ... LOOP UNTIL I > 100` ran on the WHILE
        // alone, with the UNTIL having no effect at all. Writing both is a
        // mistake about which test applies, so say so rather than pick one.
        if condition.is_some() && end_condition.is_some() {
            return err(
                "a DO loop may test its condition at only one end, not on both DO and LOOP",
            );
        }

        Ok(StmtKind::DoLoop {
            condition: condition.or(end_condition),
            cond_at_start,
            is_until: if cond_at_start {
                is_until
            } else {
                end_is_until
            },
            body,
        })
    }

    fn parse_select_case(&mut self) -> PResult<StmtKind> {
        let select_line = self.cur_line();
        self.advance(); // consume SELECT
        self.expect(Token::Case)?;
        let expr = self.parse_expression()?;
        self.skip_newlines();

        let mut cases: Vec<(Option<Vec<CaseClause>>, Vec<Stmt>)> = Vec::new();

        // Parse CASE blocks until END SELECT
        loop {
            // The body loop below stops at end of file rather than spinning, so
            // this is where an unterminated SELECT CASE is caught. It used to
            // fall through to `expect(Token::Case)` and report "Expected Case,
            // got Eof" against the last line of the file.
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::ErrorAt(
                    select_line,
                    "SELECT CASE is missing its END SELECT".to_string(),
                ));
            }

            // Check for END SELECT
            if self.at_end_select() {
                // Consume END SELECT
                if matches!(self.peek(), Token::End) {
                    self.advance();
                    self.expect(Token::Select)?;
                } else {
                    self.advance(); // consume ENDSELECT
                }
                break;
            }

            // Expect CASE keyword
            self.expect(Token::Case)?;

            // Check for CASE ELSE
            let case_value = if matches!(self.peek(), Token::Else) {
                self.advance();
                None
            } else {
                Some(self.parse_case_clauses()?)
            };

            self.skip_newlines();

            // Parse case body until next CASE or END SELECT
            let mut body = Vec::new();
            loop {
                // Check for terminators before parsing statement. A bare END is
                // a statement, not a terminator: only END SELECT ends the body.
                if self.at_end_select() || matches!(self.peek(), Token::Case | Token::Eof) {
                    break;
                }

                body.push(self.parse_inner_statement()?);
                self.skip_newlines();
            }

            cases.push((case_value, body));
        }

        Ok(StmtKind::SelectCase { expr, cases })
    }

    /// Parse the alternatives of one `CASE`, which may be values, ranges or
    /// `IS` comparisons, separated by commas.
    fn parse_case_clauses(&mut self) -> PResult<Vec<CaseClause>> {
        let mut clauses = Vec::new();
        loop {
            // `CASE IS > 100` compares the selector against a value.
            let is_comparison = matches!(self.peek(), Token::Ident(n) if n == "IS");
            if is_comparison {
                self.advance();
                let op = match self.advance() {
                    Token::Eq => BinaryOp::Eq,
                    Token::Ne => BinaryOp::Ne,
                    Token::Lt => BinaryOp::Lt,
                    Token::Gt => BinaryOp::Gt,
                    Token::Le => BinaryOp::Le,
                    Token::Ge => BinaryOp::Ge,
                    tok => {
                        return err(format!(
                            "Expected a comparison after CASE IS, got {}",
                            describe_token(&tok)
                        ));
                    }
                };
                clauses.push(CaseClause::Compare(op, self.parse_expression()?));
            } else {
                let first = self.parse_expression()?;
                // `CASE 1 TO 10` is an inclusive range.
                if matches!(self.peek(), Token::To) {
                    self.advance();
                    let last = self.parse_expression()?;
                    clauses.push(CaseClause::Range(first, last));
                } else {
                    clauses.push(CaseClause::Value(first));
                }
            }

            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(clauses)
    }

    fn parse_goto(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume GOTO
        let target = self.parse_goto_target()?;
        Ok(StmtKind::Goto(target))
    }

    fn parse_gosub(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume GOSUB
        let target = self.parse_goto_target()?;
        Ok(StmtKind::Gosub(target))
    }

    fn parse_goto_target(&mut self) -> PResult<GotoTarget> {
        match self.advance() {
            Token::Integer(n) => Ok(GotoTarget::Line(n as u32)),
            Token::LineNumber(n) => Ok(GotoTarget::Line(n)),
            Token::Ident(name) => Ok(GotoTarget::Label(name)),
            tok => err(format!(
                "expected a line number or label, got {}",
                describe_token(&tok)
            )),
        }
    }

    /// `ON expr GOTO t, ...` and `ON expr GOSUB t, ...`, which differ only in
    /// the one keyword and in whether the subroutine comes back.
    fn parse_on_goto(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume ON

        // `ON ERROR GOTO n` is a different statement that happens to share a
        // first word. Recognised positionally, like ERASE and LSET, so ERROR
        // stays available to the UNSUPPORTED table everywhere else.
        if matches!(self.peek(), Token::Ident(n) if n.eq_ignore_ascii_case("ERROR")) {
            self.advance(); // consume ERROR
            if !matches!(self.advance(), Token::Goto) {
                return err("ON ERROR must be followed by GOTO");
            }
            // GW-BASIC spells "stop trapping" as GOTO 0, and there is no line
            // 0 to check against, so it is folded away here.
            if matches!(self.peek(), Token::Integer(0)) {
                self.advance();
                return Ok(StmtKind::OnError(None));
            }
            return Ok(StmtKind::OnError(Some(self.parse_goto_target()?)));
        }

        let expr = self.parse_expression()?;
        let is_gosub = match self.advance() {
            Token::Goto => false,
            Token::Gosub => true,
            tok => {
                return err(format!(
                    "Expected GOTO or GOSUB after ON, got {}",
                    describe_token(&tok)
                ));
            }
        };

        let mut targets = Vec::new();
        loop {
            targets.push(self.parse_goto_target()?);
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(if is_gosub {
            StmtKind::OnGosub { expr, targets }
        } else {
            StmtKind::OnGoto { expr, targets }
        })
    }

    fn parse_dim(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume DIM
        Ok(StmtKind::Dim {
            decls: self.parse_dim_list()?,
        })
    }

    /// Parse the `name(bounds), name(bounds), ...` part shared by DIM and REDIM.
    /// Parse the declarator list shared by DIM and REDIM.
    ///
    /// Each declarator is `name`, `name(bounds)`, or either of those followed
    /// by `AS type`, and they may be mixed in one statement.
    fn parse_dim_list(&mut self) -> PResult<Vec<Declarator>> {
        let mut decls = Vec::new();

        loop {
            let name = if let Token::Ident(n) = self.advance() {
                n
            } else {
                return err("Expected a variable or array name");
            };

            let dimensions = if matches!(self.peek(), Token::LParen) {
                self.advance();
                let dims = self.parse_expr_list()?;
                self.expect(Token::RParen)?;
                Some(dims)
            } else {
                None
            };

            let ty = if matches!(self.peek(), Token::As) {
                self.advance();
                Some(self.parse_type_ref()?)
            } else {
                None
            };

            if dimensions.is_none() && ty.is_none() {
                return err(format!(
                    "'{}' needs either array bounds or an AS clause",
                    name
                ));
            }

            decls.push(Declarator {
                name,
                dimensions,
                ty,
            });

            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(decls)
    }

    /// `REDIM [PRESERVE] A(bounds), B(bounds), ...`
    fn parse_redim(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume REDIM
        let preserve = if matches!(self.peek(), Token::Preserve) {
            self.advance();
            true
        } else {
            false
        };
        let decls = self.parse_dim_list()?;
        Ok(StmtKind::Redim { decls, preserve })
    }

    /// Parse a type name in an `AS` clause.
    ///
    /// The built-in type names are matched here rather than made keywords: as
    /// keywords they would collide with ordinary uses of the same words, and
    /// both `FUNCTION Double(X)` and `STRING$(...)` are real programs.
    fn parse_type_ref(&mut self) -> PResult<TypeRef> {
        let Token::Ident(name) = self.advance() else {
            return err("Expected a type name");
        };
        match name.to_uppercase().as_str() {
            "INTEGER" => Ok(TypeRef::Integer),
            "LONG" => Ok(TypeRef::Long),
            "SINGLE" => Ok(TypeRef::Single),
            "DOUBLE" => Ok(TypeRef::Double),
            "STRING" => {
                // STRING * n declares a fixed-length field.
                if matches!(self.peek(), Token::Star) {
                    self.advance();
                    match self.advance() {
                        Token::Integer(n) if n > 0 => Ok(TypeRef::FixedString(n as usize)),
                        tok => err(format!(
                            "Expected a positive length after STRING *, got {}",
                            describe_token(&tok)
                        )),
                    }
                } else {
                    err("A STRING field needs a fixed length, as in STRING * 20")
                }
            }
            _ => Ok(TypeRef::Record(name)),
        }
    }

    /// `TYPE name` ... `END TYPE`
    fn parse_type_def(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume TYPE
        let name = if let Token::Ident(n) = self.advance() {
            n
        } else {
            return err("Expected a type name after TYPE");
        };
        self.skip_newlines();

        let mut fields = Vec::new();
        loop {
            // END TYPE, in either spelling, closes the definition.
            if matches!(self.peek(), Token::EndType) {
                self.advance();
                break;
            }
            if matches!(self.peek(), Token::End) && matches!(self.peek_at(1), Token::Type) {
                self.advance();
                self.advance();
                break;
            }
            if matches!(self.peek(), Token::Eof) {
                return err(format!("TYPE '{}' is missing its END TYPE", name));
            }

            let field = if let Token::Ident(f) = self.advance() {
                f
            } else {
                return err("Expected a field name");
            };
            self.expect(Token::As)?;
            let ty = self.parse_type_ref()?;
            fields.push(FieldDecl { name: field, ty });
            self.skip_newlines();
        }

        Ok(StmtKind::TypeDef { name, fields })
    }

    fn parse_sub(&mut self) -> PResult<StmtKind> {
        let sub_line = self.cur_line();
        self.advance(); // consume SUB
        let name = if let Token::Ident(n) = self.advance() {
            n
        } else {
            return err("Expected subroutine name");
        };

        let params = if matches!(self.peek(), Token::LParen) {
            self.advance();
            let params = self.parse_param_list()?;
            self.expect(Token::RParen)?;
            params
        } else {
            Vec::new()
        };

        // A SUB has no result, so an `AS` clause here has nothing to describe.
        if matches!(self.peek(), Token::As) {
            return err(format!(
                "a SUB has no return value, so '{}' cannot be declared AS a type; use a FUNCTION",
                name
            ));
        }

        self.skip_newlines();

        let body = self.parse_block(BlockEnd::EndSub, &format!("SUB '{}'", name), sub_line)?;

        Ok(StmtKind::Sub { name, params, body })
    }

    /// `FUNCTION name(params)` ... `END FUNCTION`
    fn parse_function(&mut self) -> PResult<StmtKind> {
        let fn_line = self.cur_line();
        self.advance(); // consume FUNCTION
        let name = if let Token::Ident(n) = self.advance() {
            n
        } else {
            return err("Expected function name");
        };

        let params = if matches!(self.peek(), Token::LParen) {
            self.advance();
            let params = self.parse_param_list()?;
            self.expect(Token::RParen)?;
            params
        } else {
            Vec::new()
        };

        // `FUNCTION f AS T` gives the result a declared type. This used to be
        // recorded in a parser-local map that nothing ever read, so the result
        // silently fell back to the name's suffix -- Double, in most cases.
        let mut ret_ty = None;
        if matches!(self.peek(), Token::As) {
            self.advance();
            let ty = self.parse_type_ref()?;
            if let TypeRef::Record(r) = &ty {
                return err(format!(
                    "a FUNCTION cannot return the record type '{}'; use a SUB with a record parameter",
                    r
                ));
            }
            ret_ty = Some(ty);
        }

        self.skip_newlines();

        let body = self.parse_block(
            BlockEnd::EndFunction,
            &format!("FUNCTION '{}'", name),
            fn_line,
        )?;

        Ok(StmtKind::Function {
            name,
            params,
            ret_ty,
            body,
        })
    }

    fn parse_param_list(&mut self) -> PResult<Vec<Param>> {
        let mut params = Vec::new();
        while let Token::Ident(name) = self.peek().clone() {
            self.advance();
            // `name AS Type` declares a typed parameter, including a record.
            let ty = if matches!(self.peek(), Token::As) {
                self.advance();
                Some(self.parse_type_ref()?)
            } else {
                None
            };
            params.push(Param { name, ty });
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(params)
    }

    /// `DATA item, item, ...`, where the items arrived as raw source text.
    ///
    /// The lexer hands over the whole operand verbatim (see
    /// `Lexer::read_data_text`), because a DATA item is not an expression: it is
    /// a literal run of characters, and tokenizing it would uppercase words and
    /// renormalize numbers. This used to accept only Integer, Float, String and
    /// a leading minus, so `DATA hello, world` ended the list at `hello` -- and
    /// silently, since the loop simply broke, leaving the word to be parsed as a
    /// fresh statement and produce an error about the word rather than the DATA.
    fn parse_data(&mut self, text: &str) -> PResult<StmtKind> {
        self.advance(); // consume the DATA token
        Ok(StmtKind::Data(
            split_data_items(text)
                .iter()
                .map(|it| it.literal())
                .collect(),
        ))
    }

    fn parse_read(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume READ
        let mut vars = Vec::new();

        while matches!(self.peek(), Token::Ident(_)) {
            vars.push(self.parse_lvalue()?);
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(StmtKind::Read(vars))
    }

    /// `ERASE A, B` -- one or more array names.
    ///
    /// Contextual rather than reserved, like CALL above it: LANGREF's own
    /// `ON ... GOSUB Draw, Erase` example uses the word as a label, and a
    /// reserved ERASE would take that name away from every program.
    /// `RESUME`, `RESUME NEXT`, `RESUME 0`, `RESUME <line|label>`.
    fn parse_resume(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume RESUME
        // GW-BASIC spells "retry the failing statement" as either a bare
        // RESUME or RESUME 0, so the two are folded together here.
        if matches!(self.peek(), Token::Newline | Token::Colon | Token::Eof) {
            return Ok(StmtKind::Resume(ResumeTarget::Same));
        }
        if matches!(self.peek(), Token::Integer(0)) {
            self.advance();
            return Ok(StmtKind::Resume(ResumeTarget::Same));
        }
        if matches!(self.peek(), Token::Next) {
            self.advance();
            return Ok(StmtKind::Resume(ResumeTarget::Next));
        }
        Ok(StmtKind::Resume(ResumeTarget::At(
            self.parse_goto_target()?,
        )))
    }

    /// `ERROR n` -- raise an error by GW-BASIC's number for it.
    fn parse_raise_error(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume ERROR
        Ok(StmtKind::RaiseError(self.parse_expression()?))
    }

    fn parse_erase(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume ERASE
        let mut names = Vec::new();
        loop {
            let Token::Ident(name) = self.advance() else {
                return err("ERASE needs an array name");
            };
            names.push(name);
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(StmtKind::Erase(names))
    }

    /// `LOCATE row, col`, `LOCATE row`, `LOCATE , col`.
    fn parse_locate(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume LOCATE
        let (row, col) = self.parse_two_optional_args()?;
        if row.is_none() && col.is_none() {
            return err("LOCATE needs a row, a column, or both");
        }
        Ok(StmtKind::Locate { row, col })
    }

    /// `COLOR fg, bg`, `COLOR fg`, `COLOR , bg`.
    fn parse_color(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume COLOR
        let (fg, bg) = self.parse_two_optional_args()?;
        if fg.is_none() && bg.is_none() {
            return err("COLOR needs a foreground, a background, or both");
        }
        // GW-BASIC's third argument is the border colour, which a terminal has
        // no equivalent for.
        if matches!(self.peek(), Token::Comma) {
            return err("COLOR takes a foreground and a background; a terminal has no border");
        }
        Ok(StmtKind::Color { fg, bg })
    }

    /// `a, b` where either side may be left out -- the shape LOCATE and COLOR
    /// share.
    fn parse_two_optional_args(&mut self) -> PResult<(Option<Expr>, Option<Expr>)> {
        let ends = |t: &Token| matches!(t, Token::Newline | Token::Colon | Token::Eof);
        let first = if matches!(self.peek(), Token::Comma) || ends(self.peek()) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        let second = if matches!(self.peek(), Token::Comma) {
            self.advance();
            if ends(self.peek()) {
                None
            } else {
                Some(self.parse_expression()?)
            }
        } else {
            None
        };
        Ok((first, second))
    }

    /// `DEFINT A-Z`, `DEFSTR S`, `DEFINT A, C-E`.
    ///
    /// Each clause is a single letter or an inclusive range of them. The lexer
    /// has already upper-cased the identifiers, so `a-z` and `A-Z` arrive the
    /// same.
    fn parse_def_type(&mut self, word: DataTypeWord) -> PResult<StmtKind> {
        self.advance(); // consume DEFINT/DEFLNG/...
        let ty = match word {
            DataTypeWord::Integer => DataType::Integer,
            DataTypeWord::Long => DataType::Long,
            DataTypeWord::Single => DataType::Single,
            DataTypeWord::Double => DataType::Double,
            DataTypeWord::String => DataType::String,
        };

        let mut ranges = Vec::new();
        loop {
            let first = self.def_type_letter()?;
            let last = if matches!(self.peek(), Token::Minus) {
                self.advance();
                self.def_type_letter()?
            } else {
                first
            };
            if last < first {
                return err(format!(
                    "the letter range {}-{} runs backwards",
                    first, last
                ));
            }
            ranges.push((first, last));
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(StmtKind::DefType { ty, ranges })
    }

    /// One letter of a `DEF*` range.
    ///
    /// A single letter lexes as an identifier, so this checks the length here
    /// rather than trusting the token.
    fn def_type_letter(&mut self) -> PResult<char> {
        let tok = self.advance();
        if let Token::Ident(name) = &tok {
            let mut chars = name.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                if c.is_ascii_alphabetic() {
                    return Ok(c);
                }
            }
        }
        err(format!(
            "expected a single letter in the range, got {}",
            describe_token(&tok)
        ))
    }

    /// `RANDOMIZE`, `RANDOMIZE n`, `RANDOMIZE TIMER`.
    ///
    /// `TIMER` needs no special case: it is an ordinary builtin, so the general
    /// expression form covers it.
    fn parse_randomize(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume RANDOMIZE
        let seed = if matches!(self.peek(), Token::Newline | Token::Colon | Token::Eof) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        Ok(StmtKind::Randomize(seed))
    }

    fn parse_restore(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume RESTORE
        let target = if matches!(self.peek(), Token::Integer(_) | Token::Ident(_)) {
            Some(self.parse_goto_target()?)
        } else {
            None
        };
        Ok(StmtKind::Restore(target))
    }

    /// Parse a file number: `#` followed by any numeric expression.
    ///
    /// GW-BASIC allows an expression here; only a literal used to be accepted,
    /// so `OPEN ... AS #F%` was a parse error.
    fn parse_file_number(&mut self) -> PResult<Expr> {
        self.expect(Token::Hash)?;
        self.parse_expression()
    }

    fn parse_open(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume OPEN

        // Parse filename expression
        let filename = self.parse_expression()?;

        // Expect FOR
        self.expect(Token::For)?;

        // Parse mode (INPUT, OUTPUT, APPEND, RANDOM)
        let mode = match self.peek() {
            Token::Input => {
                self.advance();
                FileMode::Input
            }
            Token::Output => {
                self.advance();
                FileMode::Output
            }
            Token::Append => {
                self.advance();
                FileMode::Append
            }
            // RANDOM is not a reserved word: only this position gives it a
            // meaning, so a program may still use the name elsewhere.
            Token::Ident(n) if n.eq_ignore_ascii_case("RANDOM") => {
                self.advance();
                FileMode::Random
            }
            tok => {
                let tok = tok.clone();
                return err(format!(
                    "expected INPUT, OUTPUT, APPEND or RANDOM, got {}",
                    describe_token(&tok)
                ));
            }
        };

        // Expect AS
        self.expect(Token::As)?;

        let file_num = self.parse_file_number()?;

        // `LEN = n` sets the record length. LEN is the string function's name
        // everywhere else, so it is matched here as an identifier rather than
        // reserved.
        let reclen = match self.peek() {
            Token::Ident(n) if n.eq_ignore_ascii_case("LEN") => {
                self.advance();
                self.expect(Token::Eq)?;
                Some(self.parse_expression()?)
            }
            _ => None,
        };

        if reclen.is_some() && mode != FileMode::Random {
            return err("LEN = applies only to OPEN ... FOR RANDOM");
        }

        Ok(StmtKind::Open {
            filename,
            mode,
            file_num,
            reclen,
        })
    }

    /// `FIELD #n, width AS var$ [, width AS var$]...`
    fn parse_field(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume FIELD
        let file_num = self.parse_file_number()?;
        self.expect(Token::Comma)?;

        let mut fields = Vec::new();
        loop {
            let width = self.parse_expression()?;
            self.expect(Token::As)?;
            let target = self.parse_lvalue()?;
            fields.push(FieldSlice { width, target });
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(StmtKind::Field { file_num, fields })
    }

    /// `CALL Name(args)` or `CALL Name` -- the explicit form of a procedure
    /// call, which GW-BASIC and QuickBASIC both accept.
    ///
    /// Recognised only in statement position before a name, like the
    /// random-access statement names, so a program may still use CALL for a
    /// variable of its own. Without this the word parsed as a paren-less call
    /// to a subroutine named CALL, and the diagnostic complained about the
    /// callee rather than about CALL.
    fn parse_call(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume CALL
        let Token::Ident(name) = self.advance() else {
            return err("Expected a procedure name after CALL");
        };
        let args = if matches!(self.peek(), Token::LParen) {
            self.advance();
            let args = self.parse_expr_list()?;
            self.expect(Token::RParen)?;
            args
        } else {
            Vec::new()
        };
        Ok(StmtKind::Call { name, args })
    }

    /// `LSET v$ = expr` / `RSET v$ = expr`
    fn parse_set_field(&mut self, right: bool) -> PResult<StmtKind> {
        self.advance(); // consume LSET or RSET
        let target = self.parse_lvalue()?;
        self.expect(Token::Eq)?;
        let value = self.parse_expression()?;
        Ok(StmtKind::SetField {
            target,
            value,
            right,
        })
    }

    /// `GET #n[, record]` / `PUT #n[, record]`
    fn parse_get_put(&mut self, is_put: bool) -> PResult<StmtKind> {
        self.advance(); // consume GET or PUT
        let file_num = self.parse_file_number()?;
        let record = if matches!(self.peek(), Token::Comma) {
            self.advance();
            // `GET #1,` with nothing after it means "the next record", the same
            // as leaving the comma off.
            if matches!(self.peek(), Token::Newline | Token::Colon | Token::Eof) {
                None
            } else {
                Some(self.parse_expression()?)
            }
        } else {
            None
        };
        Ok(StmtKind::GetPut {
            file_num,
            record,
            is_put,
        })
    }

    /// `LOCK #n[, start [TO end]]` / `UNLOCK #n[, start [TO end]]`
    fn parse_lock(&mut self, is_unlock: bool) -> PResult<StmtKind> {
        self.advance(); // consume LOCK or UNLOCK
        let file_num = self.parse_file_number()?;
        let range = if matches!(self.peek(), Token::Comma) {
            self.advance();
            let start = self.parse_expression()?;
            let end = if matches!(self.peek(), Token::To) {
                self.advance();
                Some(self.parse_expression()?)
            } else {
                None
            };
            Some((start, end))
        } else {
            None
        };
        Ok(StmtKind::Lock {
            file_num,
            range,
            is_unlock,
        })
    }

    fn parse_close(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume CLOSE

        // Bare CLOSE closes every open file.
        let file_num = if matches!(self.peek(), Token::Hash) {
            Some(self.parse_file_number()?)
        } else {
            None
        };

        Ok(StmtKind::Close { file_num })
    }

    // Expression parsing with precedence climbing
    fn parse_expression(&mut self) -> PResult<Expr> {
        self.parse_prec(1) // Start at lowest precedence
    }

    /// Precedence-climbing parser for binary expressions
    /// min_prec: minimum precedence level to parse at this level
    /// Consume a `.field.sub` chain, returning the names.
    ///
    /// The statement-level twin of [`Self::parse_field_chain`], which builds
    /// `Expr::Field` nodes instead. Assignment targets keep their path as a
    /// plain list of names inside an `LValue`, and three statement parsers
    /// spelled this loop out identically before it was hoisted here.
    fn parse_field_path(&mut self) -> PResult<Vec<String>> {
        let mut fields = Vec::new();
        while matches!(self.peek(), Token::Dot) {
            self.advance();
            match self.advance() {
                Token::Ident(f) => fields.push(f),
                tok => {
                    return err(format!(
                        "Expected a field name after '.', got {}",
                        describe_token(&tok)
                    ));
                }
            }
        }
        Ok(fields)
    }

    /// Consume any `.field` chain following an expression.
    fn parse_field_chain(&mut self, mut base: Expr) -> PResult<Expr> {
        while matches!(self.peek(), Token::Dot) {
            self.advance();
            match self.advance() {
                Token::Ident(field) => {
                    base = Expr::Field {
                        base: Box::new(base),
                        field,
                    }
                }
                tok => {
                    return err(format!(
                        "Expected a field name after '.', got {}",
                        describe_token(&tok)
                    ));
                }
            }
        }
        Ok(base)
    }

    /// Every descent into an expression passes through here, so this is the one
    /// place the depth has to be counted: parentheses re-enter via
    /// `parse_primary`, and unary `-`, `+` and `NOT` re-enter directly.
    fn parse_prec(&mut self, min_prec: u8) -> PResult<Expr> {
        self.depth += 1;
        let r = self.parse_prec_inner(min_prec);
        self.depth -= 1;
        r
    }

    fn parse_prec_inner(&mut self, min_prec: u8) -> PResult<Expr> {
        if self.depth > MAX_DEPTH {
            return err(format!("nesting is too deep (limit {} levels)", MAX_DEPTH));
        }

        // `NOT` is a prefix operator sitting between the comparisons and `AND`,
        // so its operand is everything that binds at least as tightly as a
        // comparison -- and no more.
        //
        // It used to take its operand at the *caller's* `min_prec`, which at
        // statement level is the lowest of all, so `NOT` swallowed whatever
        // followed it: `NOT A AND B` parsed as `NOT (A AND B)`. Taking the
        // operand at CMP_PREC keeps `NOT A = B` grouping as `NOT (A = B)` --
        // the form that actually matters -- while leaving `AND` to the loop
        // below, which is where it belongs.
        let mut left = if matches!(self.peek(), Token::Not) {
            self.advance();
            let operand = self.parse_prec(CMP_PREC)?;
            Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            }
        } else {
            self.parse_unary()?
        };

        // Parse binary operators with precedence climbing
        while let Some((prec, op)) = binary_op_info(self.peek()) {
            if prec < min_prec {
                break;
            }
            self.advance();
            // Every operator associates left to right, `^` included: GW-BASIC
            // and QuickBASIC evaluate `2 ^ 3 ^ 2` as `(2^3)^2` = 64. This used
            // to special-case `Pow` to bind right, giving 512.
            let next_min = prec + 1;
            let right = self.parse_prec(next_min)?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        match self.peek() {
            Token::Minus => {
                self.advance();
                // `^` binds tighter than unary minus, so -2^2 is -(2^2) = -4,
                // per LANGREF's precedence table and GW-BASIC. Parsing the
                // operand at the power level is what folds the exponentiation
                // in before the negation.
                let operand = self.parse_prec(POWER_PREC)?;
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(operand),
                })
            }
            Token::Plus => {
                self.advance();
                self.parse_prec(POWER_PREC)
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        match self.peek().clone() {
            Token::Integer(n) => {
                self.advance();
                Ok(Expr::Literal(Literal::Integer(n)))
            }
            Token::Float(f) => {
                self.advance();
                Ok(Expr::Literal(Literal::Float(f)))
            }
            Token::String(s) => {
                self.advance();
                Ok(Expr::Literal(Literal::String(s)))
            }
            Token::Ident(name) => {
                self.advance();
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let args = self.parse_expr_list()?;
                    self.expect(Token::RParen)?;

                    // `A(1)` is an array element or a call; the parser cannot
                    // tell, and used to guess from the DIM statements it had
                    // read so far. That made the AST depend on where the DIM
                    // was written -- the same source became FnCall before it
                    // and ArrayAccess after -- and ignored scope entirely, so a
                    // DIM inside a SUB changed how module-level code parsed.
                    // Sema resolves it against the finished symbol table.
                    self.parse_field_chain(Expr::FnCall { name, args })
                } else {
                    self.parse_field_chain(Expr::Variable(name))
                }
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expression()?;
                self.expect(Token::RParen)?;
                Ok(expr)
            }
            tok => err(format!(
                "unexpected {} in an expression",
                describe_token(&tok)
            )),
        }
    }

    fn parse_expr_list(&mut self) -> PResult<Vec<Expr>> {
        let mut exprs = Vec::new();
        if matches!(self.peek(), Token::RParen) {
            return Ok(exprs);
        }
        exprs.push(self.parse_expression()?);
        while matches!(self.peek(), Token::Comma) {
            self.advance();
            exprs.push(self.parse_expression()?);
        }
        Ok(exprs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    /// Parse, joining any errors so these tests keep their `Result<_, String>`
    /// shape now that the parser reports every error it finds rather than one.
    fn parse(input: &str) -> Result<Program, String> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize()?;
        let lines = lexer.line_map().to_vec();
        let mut parser = Parser::new(tokens, lines);
        parser.parse().map_err(|errors| {
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        })
    }

    // Label Tests

    #[test]
    fn test_label() {
        let prog = parse("10 PRINT X").unwrap();
        assert_eq!(prog.statements.len(), 2);
        if let StmtKind::Label(n) = &prog.statements[0].kind {
            assert_eq!(*n, 10);
        } else {
            panic!("Expected Label");
        }
    }

    #[test]
    fn test_multiple_labels() {
        let prog = parse("10 X = 1\n20 Y = 2\n30 END").unwrap();
        assert_eq!(prog.statements.len(), 6); // 3 labels + 3 statements
        assert!(matches!(&prog.statements[0].kind, StmtKind::Label(10)));
        assert!(matches!(&prog.statements[2].kind, StmtKind::Label(20)));
        assert!(matches!(&prog.statements[4].kind, StmtKind::Label(30)));
    }

    // Let Tests

    #[test]
    fn test_let_simple() {
        let prog = parse("X = 42").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Let {
            name,
            indices,
            value,
        } = &prog.statements[0].kind
        {
            assert_eq!(name, "X");
            assert!(indices.is_none());
            assert!(matches!(value, Expr::Literal(Literal::Integer(42))));
        } else {
            panic!("Expected Let");
        }
    }

    #[test]
    fn test_let_with_keyword() {
        let prog = parse("LET X = 42").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Let { name, .. } = &prog.statements[0].kind {
            assert_eq!(name, "X");
        } else {
            panic!("Expected Let");
        }
    }

    #[test]
    fn test_let_array_assignment() {
        let prog = parse("A(5) = 100").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Let {
            name,
            indices,
            value,
        } = &prog.statements[0].kind
        {
            assert_eq!(name, "A");
            assert!(indices.is_some());
            let idx = indices.as_ref().unwrap();
            assert_eq!(idx.len(), 1);
            assert!(matches!(value, Expr::Literal(Literal::Integer(100))));
        } else {
            panic!("Expected Let with array");
        }
    }

    #[test]
    fn test_let_expression() {
        let prog = parse("X = 1 + 2 * 3").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
            // Should be 1 + (2 * 3) due to precedence
            if let Expr::Binary { op, .. } = value {
                assert_eq!(*op, BinaryOp::Add);
            } else {
                panic!("Expected binary expression");
            }
        } else {
            panic!("Expected Let");
        }
    }

    // Print Tests

    #[test]
    fn test_print_string() {
        let prog = parse(r#"PRINT "Hello""#).unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Print { items, newline, .. } = &prog.statements[0].kind {
            assert_eq!(items.len(), 1);
            assert!(*newline);
        } else {
            panic!("Expected Print");
        }
    }

    #[test]
    fn test_print_multiple_items() {
        let prog = parse(r#"PRINT "A"; B; C"#).unwrap();
        if let StmtKind::Print { items, .. } = &prog.statements[0].kind {
            assert_eq!(items.len(), 5); // "A", Empty, B, Empty, C
        } else {
            panic!("Expected Print");
        }
    }

    #[test]
    fn test_print_with_tab() {
        let prog = parse(r#"PRINT A, B"#).unwrap();
        if let StmtKind::Print { items, .. } = &prog.statements[0].kind {
            assert!(items.iter().any(|i| matches!(i, PrintItem::Tab)));
        } else {
            panic!("Expected Print");
        }
    }

    #[test]
    fn test_print_no_newline() {
        let prog = parse(r#"PRINT X;"#).unwrap();
        if let StmtKind::Print { newline, .. } = &prog.statements[0].kind {
            assert!(!*newline);
        } else {
            panic!("Expected Print");
        }
    }

    // Input Tests

    #[test]
    fn test_input_simple() {
        let prog = parse("INPUT X").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Input { prompt, vars, .. } = &prog.statements[0].kind {
            assert!(prompt.is_none());
            assert_eq!(vars.len(), 1);
            assert_eq!(vars[0].name, "X");
        } else {
            panic!("Expected Input");
        }
    }

    #[test]
    fn test_input_with_prompt() {
        let prog = parse(r#"INPUT "Enter value: ", X"#).unwrap();
        if let StmtKind::Input { prompt, vars, .. } = &prog.statements[0].kind {
            assert_eq!(prompt.as_ref().unwrap(), "Enter value: ");
            assert_eq!(vars[0].name, "X");
        } else {
            panic!("Expected Input");
        }
    }

    #[test]
    fn test_input_multiple_vars() {
        let prog = parse("INPUT A, B, C").unwrap();
        if let StmtKind::Input { vars, .. } = &prog.statements[0].kind {
            assert_eq!(vars.len(), 3);
        } else {
            panic!("Expected Input");
        }
    }

    // LineInput Tests

    #[test]
    fn test_line_input_simple() {
        let prog = parse("LINE INPUT X$").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::LineInput { prompt, var, .. } = &prog.statements[0].kind {
            assert!(prompt.is_none());
            assert_eq!(var.name, "X$");
        } else {
            panic!("Expected LineInput");
        }
    }

    #[test]
    fn test_line_input_with_prompt() {
        let prog = parse(r#"LINE INPUT "Name: ", NAME$"#).unwrap();
        if let StmtKind::LineInput { prompt, var, .. } = &prog.statements[0].kind {
            assert_eq!(prompt.as_ref().unwrap(), "Name: ");
            assert_eq!(var.name, "NAME$");
        } else {
            panic!("Expected LineInput");
        }
    }

    // If Tests

    #[test]
    fn test_if_single_line() {
        let prog = parse("IF X > 0 THEN PRINT X").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::If {
            condition,
            then_branch,
            else_branch,
        } = &prog.statements[0].kind
        {
            assert!(matches!(
                condition,
                Expr::Binary {
                    op: BinaryOp::Gt,
                    ..
                }
            ));
            assert_eq!(then_branch.len(), 1);
            assert!(else_branch.is_none());
        } else {
            panic!("Expected If");
        }
    }

    #[test]
    fn test_if_single_line_with_else() {
        let prog = parse("IF X > 0 THEN PRINT X ELSE PRINT Y").unwrap();
        if let StmtKind::If {
            then_branch,
            else_branch,
            ..
        } = &prog.statements[0].kind
        {
            assert_eq!(then_branch.len(), 1);
            assert!(else_branch.is_some());
            assert_eq!(else_branch.as_ref().unwrap().len(), 1);
        } else {
            panic!("Expected If");
        }
    }

    #[test]
    fn test_if_block() {
        let prog = parse("IF X > 0 THEN\nPRINT X\nEND IF").unwrap();
        if let StmtKind::If {
            then_branch,
            else_branch,
            ..
        } = &prog.statements[0].kind
        {
            assert_eq!(then_branch.len(), 1);
            assert!(else_branch.is_none());
        } else {
            panic!("Expected If");
        }
    }

    #[test]
    fn test_if_block_with_else() {
        let prog = parse("IF X > 0 THEN\nPRINT X\nELSE\nPRINT Y\nEND IF").unwrap();
        if let StmtKind::If {
            then_branch,
            else_branch,
            ..
        } = &prog.statements[0].kind
        {
            assert_eq!(then_branch.len(), 1);
            assert!(else_branch.is_some());
        } else {
            panic!("Expected If");
        }
    }

    // For Tests

    #[test]
    fn test_for_simple() {
        let prog = parse("FOR I = 1 TO 10\nPRINT I\nNEXT I").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::For {
            var,
            start,
            end,
            step,
            body,
        } = &prog.statements[0].kind
        {
            assert_eq!(var, "I");
            assert!(matches!(start, Expr::Literal(Literal::Integer(1))));
            assert!(matches!(end, Expr::Literal(Literal::Integer(10))));
            assert!(step.is_none());
            assert_eq!(body.len(), 1);
        } else {
            panic!("Expected For");
        }
    }

    #[test]
    fn test_for_with_step() {
        let prog = parse("FOR I = 0 TO 100 STEP 10\nNEXT").unwrap();
        if let StmtKind::For { step, .. } = &prog.statements[0].kind {
            assert!(step.is_some());
            assert!(matches!(
                step.as_ref().unwrap(),
                Expr::Literal(Literal::Integer(10))
            ));
        } else {
            panic!("Expected For");
        }
    }

    #[test]
    fn test_for_negative_step() {
        let prog = parse("FOR I = 10 TO 1 STEP -1\nNEXT").unwrap();
        if let StmtKind::For { step, .. } = &prog.statements[0].kind {
            assert!(step.is_some());
        } else {
            panic!("Expected For");
        }
    }

    // While Tests

    #[test]
    fn test_while_simple() {
        let prog = parse("WHILE X < 10\nX = X + 1\nWEND").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::While { condition, body } = &prog.statements[0].kind {
            assert!(matches!(
                condition,
                Expr::Binary {
                    op: BinaryOp::Lt,
                    ..
                }
            ));
            assert_eq!(body.len(), 1);
        } else {
            panic!("Expected While");
        }
    }

    // DoLoop Tests

    #[test]
    fn test_do_loop_simple() {
        let prog = parse("DO\nX = X + 1\nLOOP").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::DoLoop {
            condition,
            cond_at_start,
            body,
            ..
        } = &prog.statements[0].kind
        {
            assert!(condition.is_none());
            assert!(!*cond_at_start);
            assert_eq!(body.len(), 1);
        } else {
            panic!("Expected DoLoop");
        }
    }

    #[test]
    fn test_do_while() {
        let prog = parse("DO WHILE X < 10\nX = X + 1\nLOOP").unwrap();
        if let StmtKind::DoLoop {
            condition,
            cond_at_start,
            is_until,
            ..
        } = &prog.statements[0].kind
        {
            assert!(condition.is_some());
            assert!(*cond_at_start);
            assert!(!*is_until);
        } else {
            panic!("Expected DoLoop");
        }
    }

    #[test]
    fn test_do_until() {
        let prog = parse("DO UNTIL X >= 10\nX = X + 1\nLOOP").unwrap();
        if let StmtKind::DoLoop {
            condition,
            cond_at_start,
            is_until,
            ..
        } = &prog.statements[0].kind
        {
            assert!(condition.is_some());
            assert!(*cond_at_start);
            assert!(*is_until);
        } else {
            panic!("Expected DoLoop");
        }
    }

    // SelectCase Tests

    #[test]
    fn test_select_case_simple() {
        let prog = parse("SELECT CASE X\nCASE 1\nPRINT 1\nEND SELECT").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::SelectCase { expr, cases } = &prog.statements[0].kind {
            assert!(matches!(expr, Expr::Variable(_)));
            assert_eq!(cases.len(), 1);
            assert!(cases[0].0.is_some()); // Has a value
            assert_eq!(cases[0].1.len(), 1); // One statement in body
        } else {
            panic!("Expected SelectCase");
        }
    }

    #[test]
    fn test_select_case_multiple() {
        let prog = parse("SELECT CASE X\nCASE 1\nPRINT 1\nCASE 2\nPRINT 2\nEND SELECT").unwrap();
        if let StmtKind::SelectCase { cases, .. } = &prog.statements[0].kind {
            assert_eq!(cases.len(), 2);
        } else {
            panic!("Expected SelectCase");
        }
    }

    #[test]
    fn test_select_case_with_else() {
        let prog = parse("SELECT CASE X\nCASE 1\nPRINT 1\nCASE ELSE\nPRINT 0\nEND SELECT").unwrap();
        if let StmtKind::SelectCase { cases, .. } = &prog.statements[0].kind {
            assert_eq!(cases.len(), 2);
            assert!(cases[0].0.is_some()); // CASE 1
            assert!(cases[1].0.is_none()); // CASE ELSE
        } else {
            panic!("Expected SelectCase");
        }
    }

    #[test]
    fn test_select_case_string() {
        let prog = parse("SELECT CASE A$\nCASE \"yes\"\nPRINT 1\nEND SELECT").unwrap();
        if let StmtKind::SelectCase { expr, cases } = &prog.statements[0].kind {
            assert!(matches!(expr, Expr::Variable(_)));
            assert_eq!(cases.len(), 1);
            match cases[0].0.as_deref() {
                Some([CaseClause::Value(Expr::Literal(Literal::String(s)))]) => {
                    assert_eq!(s, "yes")
                }
                other => panic!("Expected a single string CASE value, got {:?}", other),
            }
        } else {
            panic!("Expected SelectCase");
        }
    }

    // Goto Tests

    #[test]
    fn test_goto_line_number() {
        let prog = parse("GOTO 100").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Goto(target) = &prog.statements[0].kind {
            assert!(matches!(target, GotoTarget::Line(100)));
        } else {
            panic!("Expected Goto");
        }
    }

    #[test]
    fn test_goto_label() {
        let prog = parse("GOTO MYLOOP").unwrap();
        if let StmtKind::Goto(target) = &prog.statements[0].kind {
            if let GotoTarget::Label(name) = target {
                assert_eq!(name, "MYLOOP");
            } else {
                panic!("Expected label target");
            }
        } else {
            panic!("Expected Goto");
        }
    }

    // Gosub Tests

    #[test]
    fn test_gosub_line_number() {
        let prog = parse("GOSUB 1000").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Gosub(target) = &prog.statements[0].kind {
            assert!(matches!(target, GotoTarget::Line(1000)));
        } else {
            panic!("Expected Gosub");
        }
    }

    #[test]
    fn test_gosub_label() {
        let prog = parse("GOSUB MYSUB").unwrap();
        if let StmtKind::Gosub(target) = &prog.statements[0].kind {
            assert!(matches!(target, GotoTarget::Label(_)));
        } else {
            panic!("Expected Gosub");
        }
    }

    // Return Tests

    #[test]
    fn test_return() {
        let prog = parse("RETURN").unwrap();
        assert_eq!(prog.statements.len(), 1);
        assert!(matches!(&prog.statements[0].kind, StmtKind::Return));
    }

    // OnGoto Tests

    #[test]
    fn test_on_goto() {
        let prog = parse("ON X GOTO 10, 20, 30").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::OnGoto { expr, targets } = &prog.statements[0].kind {
            assert!(matches!(expr, Expr::Variable(_)));
            assert_eq!(targets.len(), 3);
        } else {
            panic!("Expected OnGoto");
        }
    }

    // Dim Tests

    #[test]
    fn test_dim_single() {
        let prog = parse("DIM A(10)").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Dim { decls } = &prog.statements[0].kind {
            assert_eq!(decls.len(), 1);
            assert_eq!(decls[0].name, "A");
            assert_eq!(decls[0].dimensions.as_ref().unwrap().len(), 1);
        } else {
            panic!("Expected Dim");
        }
    }

    #[test]
    fn test_dim_multiple() {
        let prog = parse("DIM A(10), B$(100), C(50)").unwrap();
        if let StmtKind::Dim { decls } = &prog.statements[0].kind {
            assert_eq!(decls.len(), 3);
            assert_eq!(decls[0].name, "A");
            assert_eq!(decls[1].name, "B$");
            assert_eq!(decls[2].name, "C");
        } else {
            panic!("Expected Dim");
        }
    }

    #[test]
    fn test_dim_2d() {
        let prog = parse("DIM A(10, 20)").unwrap();
        if let StmtKind::Dim { decls } = &prog.statements[0].kind {
            assert_eq!(decls.len(), 1);
            assert_eq!(decls[0].name, "A");
            assert_eq!(decls[0].dimensions.as_ref().unwrap().len(), 2);
        } else {
            panic!("Expected Dim");
        }
    }

    #[test]
    fn test_dim_3d() {
        let prog = parse("DIM Matrix(5, 10, 15)").unwrap();
        if let StmtKind::Dim { decls } = &prog.statements[0].kind {
            assert_eq!(decls.len(), 1);
            assert_eq!(decls[0].name, "MATRIX");
            assert_eq!(decls[0].dimensions.as_ref().unwrap().len(), 3);
        } else {
            panic!("Expected Dim");
        }
    }

    #[test]
    fn test_dim_mixed_declarators() {
        let prog = parse("DIM A(3), B AS INTEGER, C(2) AS LONG").unwrap();
        let StmtKind::Dim { decls } = &prog.statements[0].kind else {
            panic!("Expected Dim");
        };
        assert_eq!(decls.len(), 3);
        assert_eq!(decls[0].name, "A");
        assert!(decls[0].ty.is_none());
        assert_eq!(decls[1].name, "B");
        assert!(decls[1].dimensions.is_none());
        assert!(decls[1].ty.is_some());
        assert_eq!(decls[2].dimensions.as_ref().unwrap().len(), 1);
        assert!(decls[2].ty.is_some());
    }

    #[test]
    fn test_redim_with_as_type() {
        let prog = parse("REDIM PRESERVE A(5) AS INTEGER").unwrap();
        let StmtKind::Redim { decls, preserve } = &prog.statements[0].kind else {
            panic!("Expected Redim");
        };
        assert!(preserve);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].name, "A");
        assert!(decls[0].ty.is_some());
    }

    #[test]
    fn test_array_access_2d() {
        let prog = parse("X = A(1, 2)").unwrap();
        if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
            if let Expr::FnCall { name, args } = value {
                assert_eq!(name, "A");
                assert_eq!(args.len(), 2);
            } else {
                panic!("Expected FnCall (array access)");
            }
        } else {
            panic!("Expected Let");
        }
    }

    #[test]
    fn test_array_assign_2d() {
        let prog = parse("A(1, 2) = 42").unwrap();
        if let StmtKind::Let { name, indices, .. } = &prog.statements[0].kind {
            assert_eq!(name, "A");
            assert!(indices.is_some());
            assert_eq!(indices.as_ref().unwrap().len(), 2);
        } else {
            panic!("Expected Let with indices");
        }
    }

    // Sub Tests

    #[test]
    fn test_sub_no_params() {
        let prog = parse("SUB MySub\nPRINT X\nEND SUB").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Sub { name, params, body } = &prog.statements[0].kind {
            assert_eq!(name, "MYSUB");
            assert!(params.is_empty());
            assert_eq!(body.len(), 1);
        } else {
            panic!("Expected Sub");
        }
    }

    #[test]
    fn test_sub_with_params() {
        let prog = parse("SUB MySub(A, B, C)\nPRINT A + B + C\nEND SUB").unwrap();
        if let StmtKind::Sub { params, .. } = &prog.statements[0].kind {
            assert_eq!(params.len(), 3);
        } else {
            panic!("Expected Sub");
        }
    }

    // Function Tests

    #[test]
    fn test_function_no_params() {
        let prog = parse("FUNCTION GetValue\nGetValue = 42\nEND FUNCTION").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Function {
            name, params, body, ..
        } = &prog.statements[0].kind
        {
            assert_eq!(name, "GETVALUE");
            assert!(params.is_empty());
            assert_eq!(body.len(), 1);
        } else {
            panic!("Expected Function");
        }
    }

    #[test]
    fn test_function_with_params() {
        let prog = parse("FUNCTION Add(A, B)\nAdd = A + B\nEND FUNCTION").unwrap();
        if let StmtKind::Function { name, params, .. } = &prog.statements[0].kind {
            assert_eq!(name, "ADD");
            assert_eq!(params.len(), 2);
        } else {
            panic!("Expected Function");
        }
    }

    // Call Tests

    #[test]
    fn test_call_no_args() {
        let prog = parse("MySub").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Call { name, args } = &prog.statements[0].kind {
            assert_eq!(name, "MYSUB");
            assert!(args.is_empty());
        } else {
            panic!("Expected Call");
        }
    }

    #[test]
    fn test_call_with_parens() {
        let prog = parse("MySub(1, 2, 3)").unwrap();
        if let StmtKind::Call { name, args } = &prog.statements[0].kind {
            assert_eq!(name, "MYSUB");
            assert_eq!(args.len(), 3);
        } else {
            panic!("Expected Call");
        }
    }

    #[test]
    fn test_call_without_parens() {
        let prog = parse("MySub 1, 2, 3").unwrap();
        if let StmtKind::Call { args, .. } = &prog.statements[0].kind {
            assert_eq!(args.len(), 3);
        } else {
            panic!("Expected Call");
        }
    }

    // Data Tests

    #[test]
    fn test_data_integers() {
        let prog = parse("DATA 1, 2, 3, 4, 5").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Data(values) = &prog.statements[0].kind {
            assert_eq!(values.len(), 5);
            assert!(matches!(values[0], Literal::Integer(1)));
        } else {
            panic!("Expected Data");
        }
    }

    #[test]
    fn test_data_mixed() {
        let prog = parse(r#"DATA 1, 3.14, "hello""#).unwrap();
        if let StmtKind::Data(values) = &prog.statements[0].kind {
            assert_eq!(values.len(), 3);
            assert!(matches!(values[0], Literal::Integer(1)));
            assert!(matches!(values[1], Literal::Float(_)));
            assert!(matches!(values[2], Literal::String(_)));
        } else {
            panic!("Expected Data");
        }
    }

    #[test]
    fn test_data_negative() {
        let prog = parse("DATA -5, -3.14").unwrap();
        if let StmtKind::Data(values) = &prog.statements[0].kind {
            assert!(matches!(values[0], Literal::Integer(-5)));
        } else {
            panic!("Expected Data");
        }
    }

    // Read Tests

    #[test]
    fn test_read_single() {
        let prog = parse("READ X").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Read(vars) = &prog.statements[0].kind {
            assert_eq!(vars.len(), 1);
            assert_eq!(vars[0].name, "X");
        } else {
            panic!("Expected Read");
        }
    }

    #[test]
    fn test_read_multiple() {
        let prog = parse("READ A, B, C$").unwrap();
        if let StmtKind::Read(vars) = &prog.statements[0].kind {
            assert_eq!(vars.len(), 3);
        } else {
            panic!("Expected Read");
        }
    }

    // Restore Tests

    #[test]
    fn test_restore_simple() {
        let prog = parse("RESTORE").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let StmtKind::Restore(target) = &prog.statements[0].kind {
            assert!(target.is_none());
        } else {
            panic!("Expected Restore");
        }
    }

    #[test]
    fn test_restore_with_target() {
        let prog = parse("RESTORE 100").unwrap();
        if let StmtKind::Restore(target) = &prog.statements[0].kind {
            assert!(target.is_some());
        } else {
            panic!("Expected Restore");
        }
    }

    // Cls Tests

    #[test]
    fn test_cls() {
        let prog = parse("CLS").unwrap();
        assert_eq!(prog.statements.len(), 1);
        assert!(matches!(&prog.statements[0].kind, StmtKind::Cls));
    }

    // End Tests

    #[test]
    fn test_end() {
        let prog = parse("END").unwrap();
        assert_eq!(prog.statements.len(), 1);
        assert!(matches!(&prog.statements[0].kind, StmtKind::End));
    }

    // Stop Tests

    #[test]
    fn test_stop() {
        let prog = parse("STOP").unwrap();
        assert_eq!(prog.statements.len(), 1);
        assert!(matches!(&prog.statements[0].kind, StmtKind::Stop));
    }

    // Expression Tests

    #[test]
    fn test_expr_precedence() {
        // 2 + 3 * 4 should be 2 + (3 * 4) = 14, not (2 + 3) * 4 = 20
        let prog = parse("X = 2 + 3 * 4").unwrap();
        if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
            if let Expr::Binary { op, right, .. } = value {
                assert_eq!(*op, BinaryOp::Add);
                assert!(matches!(
                    right.as_ref(),
                    Expr::Binary {
                        op: BinaryOp::Mul,
                        ..
                    }
                ));
            } else {
                panic!("Expected binary expression");
            }
        } else {
            panic!("Expected Let");
        }
    }

    #[test]
    fn test_expr_power_left_associative() {
        // 2 ^ 3 ^ 2 is (2 ^ 3) ^ 2 = 64, as GW-BASIC and QuickBASIC evaluate
        // it. This asserted the opposite nesting until associativity was made
        // uniform: every operator here associates left to right.
        let prog = parse("X = 2 ^ 3 ^ 2").unwrap();
        if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
            if let Expr::Binary { op, left, .. } = value {
                assert_eq!(*op, BinaryOp::Pow);
                assert!(matches!(
                    left.as_ref(),
                    Expr::Binary {
                        op: BinaryOp::Pow,
                        ..
                    }
                ));
            } else {
                panic!("Expected binary expression");
            }
        } else {
            panic!("Expected Let");
        }
    }

    #[test]
    fn test_expr_parentheses() {
        let prog = parse("X = (2 + 3) * 4").unwrap();
        if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
            if let Expr::Binary { op, left, .. } = value {
                assert_eq!(*op, BinaryOp::Mul);
                assert!(matches!(
                    left.as_ref(),
                    Expr::Binary {
                        op: BinaryOp::Add,
                        ..
                    }
                ));
            } else {
                panic!("Expected binary expression");
            }
        } else {
            panic!("Expected Let");
        }
    }

    #[test]
    fn test_expr_unary_neg() {
        let prog = parse("X = -5").unwrap();
        if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
            assert!(matches!(
                value,
                Expr::Unary {
                    op: UnaryOp::Neg,
                    ..
                }
            ));
        } else {
            panic!("Expected Let");
        }
    }

    #[test]
    fn test_expr_unary_not() {
        let prog = parse("X = NOT Y").unwrap();
        if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
            assert!(matches!(
                value,
                Expr::Unary {
                    op: UnaryOp::Not,
                    ..
                }
            ));
        } else {
            panic!("Expected Let");
        }
    }

    #[test]
    fn test_expr_logical_operators() {
        // OR and XOR share the lowest level and associate left to right, and
        // AND binds tighter than both, so this groups as
        // ((A AND B) OR C) XOR D and the outermost operator is XOR.
        //
        // This asserted OR at the top, which held only because XOR used to have
        // a level of its own that bound tighter than AND -- contradicting
        // LANGREF's table, which puts OR and XOR together.
        let prog = parse("X = A AND B OR C XOR D").unwrap();
        let StmtKind::Let { value, .. } = &prog.statements[0].kind else {
            panic!("Expected Let");
        };
        let Expr::Binary {
            op: BinaryOp::Xor,
            left,
            ..
        } = value
        else {
            panic!("Expected XOR at the top, got {:?}", value);
        };
        let Expr::Binary {
            op: BinaryOp::Or,
            left: or_left,
            ..
        } = left.as_ref()
        else {
            panic!("Expected OR beneath the XOR, got {:?}", left);
        };
        assert!(
            matches!(
                or_left.as_ref(),
                Expr::Binary {
                    op: BinaryOp::And,
                    ..
                }
            ),
            "AND binds tighter than both"
        );
    }

    #[test]
    fn test_expr_comparison() {
        let prog = parse("X = A < B").unwrap();
        if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
            assert!(matches!(
                value,
                Expr::Binary {
                    op: BinaryOp::Lt,
                    ..
                }
            ));
        } else {
            panic!("Expected Let");
        }
    }

    #[test]
    fn test_expr_all_comparison_ops() {
        for (input, expected_op) in [
            ("X = A = B", BinaryOp::Eq),
            ("X = A <> B", BinaryOp::Ne),
            ("X = A < B", BinaryOp::Lt),
            ("X = A > B", BinaryOp::Gt),
            ("X = A <= B", BinaryOp::Le),
            ("X = A >= B", BinaryOp::Ge),
        ] {
            let prog = parse(input).unwrap();
            if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
                if let Expr::Binary { op, .. } = value {
                    assert_eq!(*op, expected_op, "Failed for input: {}", input);
                } else {
                    panic!("Expected binary for: {}", input);
                }
            } else {
                panic!("Expected Let for: {}", input);
            }
        }
    }

    #[test]
    fn test_expr_all_arithmetic_ops() {
        for (input, expected_op) in [
            ("X = A + B", BinaryOp::Add),
            ("X = A - B", BinaryOp::Sub),
            ("X = A * B", BinaryOp::Mul),
            ("X = A / B", BinaryOp::Div),
            ("X = A \\ B", BinaryOp::IntDiv),
            ("X = A MOD B", BinaryOp::Mod),
            ("X = A ^ B", BinaryOp::Pow),
        ] {
            let prog = parse(input).unwrap();
            if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
                if let Expr::Binary { op, .. } = value {
                    assert_eq!(*op, expected_op, "Failed for input: {}", input);
                } else {
                    panic!("Expected binary for: {}", input);
                }
            } else {
                panic!("Expected Let for: {}", input);
            }
        }
    }

    #[test]
    fn test_expr_function_call() {
        let prog = parse("X = SIN(3.14)").unwrap();
        if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
            if let Expr::FnCall { name, args } = value {
                assert_eq!(name, "SIN");
                assert_eq!(args.len(), 1);
            } else {
                panic!("Expected FnCall");
            }
        } else {
            panic!("Expected Let");
        }
    }

    #[test]
    fn test_expr_function_multiple_args() {
        let prog = parse("X = MID$(A$, 1, 5)").unwrap();
        if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
            if let Expr::FnCall { name, args } = value {
                assert_eq!(name, "MID$");
                assert_eq!(args.len(), 3);
            } else {
                panic!("Expected FnCall");
            }
        } else {
            panic!("Expected Let");
        }
    }

    // Integration Tests

    #[test]
    fn test_colon_separator() {
        let prog = parse("X = 1 : Y = 2 : PRINT X").unwrap();
        assert_eq!(prog.statements.len(), 3);
    }

    #[test]
    fn test_complex_program() {
        let prog = parse(
            r#"
            10 CLS
            20 PRINT "Enter a number:"
            30 INPUT N
            40 FOR I = 1 TO N
            50 PRINT I; " squared is "; I * I
            60 NEXT I
            70 END
        "#,
        )
        .unwrap();
        // Should have 7 labels + 7 statements = 14
        assert!(prog.statements.len() >= 7);
    }
}
