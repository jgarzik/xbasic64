//! BASIC parser - produces AST from tokens

// Copyright (c) 2025-2026 Jeff Garzik
// SPDX-License-Identifier: MIT

use crate::lexer::Token;
use std::collections::HashSet;

/// Binary operator precedence levels (higher = tighter binding)
/// Returns (precedence, BinaryOp) or None if not a binary operator
fn binary_op_info(token: &Token) -> Option<(u8, BinaryOp)> {
    match token {
        // Precedence 1: logical OR (lowest)
        Token::Or => Some((1, BinaryOp::Or)),
        // Precedence 2: logical AND
        Token::And => Some((2, BinaryOp::And)),
        // Precedence 3: logical XOR
        Token::Xor => Some((3, BinaryOp::Xor)),
        // Precedence 4: comparison
        Token::Eq => Some((4, BinaryOp::Eq)),
        Token::Ne => Some((4, BinaryOp::Ne)),
        Token::Lt => Some((4, BinaryOp::Lt)),
        Token::Gt => Some((4, BinaryOp::Gt)),
        Token::Le => Some((4, BinaryOp::Le)),
        Token::Ge => Some((4, BinaryOp::Ge)),
        // Precedence 5: additive
        Token::Plus => Some((5, BinaryOp::Add)),
        Token::Minus => Some((5, BinaryOp::Sub)),
        // Precedence 6: multiplicative
        Token::Star => Some((6, BinaryOp::Mul)),
        Token::Slash => Some((6, BinaryOp::Div)),
        Token::Backslash => Some((6, BinaryOp::IntDiv)),
        Token::Mod => Some((6, BinaryOp::Mod)),
        // Precedence 7: power (handled specially for right-associativity)
        Token::Caret => Some((POWER_PREC, BinaryOp::Pow)),
        _ => None,
    }
}

/// Precedence of `^`, the tightest-binding binary operator.
const POWER_PREC: u8 = 7;

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
    },
    /// `CLOSE #n`, or bare `CLOSE` to close every open file.
    Close {
        file_num: Option<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileMode {
    Input,
    Output,
    Append,
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
/// These are not errors. BASIC's block terminators are statements
/// syntactically, but they belong to the construct that opened the block, so
/// `parse_statement` reports them through the error channel and the enclosing
/// parser (`parse_if_body`, `parse_for`, ...) treats the matching one as a
/// normal end-of-body. Any terminator that reaches the top level without a
/// matching opener is rendered as a real diagnostic.
///
/// Conditions travel as payloads rather than through parser fields, so a
/// terminator cannot be separated from its expression.
#[derive(Debug, Clone)]
pub enum BlockEnd {
    EndIf,
    EndSub,
    EndFunction,
    EndSelect,
    Next,
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
            BlockEnd::Next => "NEXT",
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
            BlockEnd::Next => "FOR",
            BlockEnd::Wend => "WHILE",
            BlockEnd::Loop | BlockEnd::LoopWhile(_) | BlockEnd::LoopUntil(_) => "DO",
        }
    }
}

/// Why parsing of a statement stopped.
#[derive(Debug, Clone)]
pub enum ParseError {
    /// A block terminator was consumed; the enclosing block parser handles it.
    Block(BlockEnd),
    /// A genuine syntax error.
    Error(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Error(msg) => write!(f, "{}", msg),
            ParseError::Block(b) => write!(f, "{} without matching {}", b.keyword(), b.opener()),
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

/// Human-readable name for a token, for diagnostics.
fn describe_token(tok: &Token) -> String {
    match tok {
        Token::Ident(n) => format!("identifier '{}'", n),
        Token::Integer(n) => format!("{}", n),
        Token::Float(f) => format!("{}", f),
        Token::String(s) => format!("string \"{}\"", s),
        Token::Newline => "end of line".to_string(),
        Token::Eof => "end of file".to_string(),
        other => format!("{:?}", other),
    }
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
    /// Tracks declared array names for distinguishing array access from function calls
    declared_arrays: HashSet<String>,
    /// SUB/FUNCTION names, collected before parsing so that `Name:` at the
    /// start of a line is not mistaken for a label definition.
    declared_procs: HashSet<String>,
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

    fn expect(&mut self, expected: Token) -> PResult<()> {
        let tok = self.advance();
        if std::mem::discriminant(&tok) == std::mem::discriminant(&expected) {
            Ok(())
        } else {
            err(format!("Expected {:?}, got {:?}", expected, tok))
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline) {
            self.advance();
        }
    }

    pub fn parse(&mut self) -> Result<Program, LocatedParseError> {
        self.parse_program().map_err(|error| LocatedParseError {
            // `pos` is left at the token that stopped the parse.
            line: self.cur_line(),
            error,
        })
    }

    fn parse_program(&mut self) -> PResult<Program> {
        let mut statements = Vec::new();
        self.skip_newlines();

        while !matches!(self.peek(), Token::Eof) {
            let stmt = self.parse_statement()?;
            statements.push(stmt);
            self.skip_newlines();
        }

        Ok(Program { statements })
    }

    /// Parse one statement, tagging it with the line it started on.
    ///
    /// `?` propagates `ParseError::Block` unchanged, so the block-terminator
    /// protocol is unaffected by the wrapping.
    fn parse_statement(&mut self) -> PResult<Stmt> {
        let line = self.cur_line();
        let kind = self.parse_statement_kind()?;
        Ok(Stmt { line, kind })
    }

    fn parse_statement_kind(&mut self) -> PResult<StmtKind> {
        // Handle line numbers as labels
        if let Token::LineNumber(n) = self.peek().clone() {
            self.advance();
            return Ok(StmtKind::Label(n));
        }

        // A named label definition: `Retry:` at the start of a line.
        if self.at_label_definition() {
            let Token::Ident(name) = self.advance() else {
                unreachable!("at_label_definition checked for an identifier")
            };
            self.advance(); // consume ':'
            return Ok(StmtKind::LabelName(name));
        }

        // Handle colon as statement separator
        if matches!(self.peek(), Token::Colon) {
            self.advance();
            return self.parse_statement_kind();
        }

        match self.peek().clone() {
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
            Token::Data => self.parse_data(),
            Token::Read => self.parse_read(),
            Token::Restore => self.parse_restore(),
            Token::Cls => {
                self.advance();
                Ok(StmtKind::Cls)
            }
            Token::Open => self.parse_open(),
            Token::Close => self.parse_close(),
            Token::End => {
                self.advance();
                // Check for END IF, END SUB, END FUNCTION, END SELECT
                match self.peek() {
                    Token::If => {
                        self.advance();
                        // Return to caller - this is a terminator, not a statement
                        Err(ParseError::Block(BlockEnd::EndIf))
                    }
                    Token::Sub => {
                        self.advance();
                        Err(ParseError::Block(BlockEnd::EndSub))
                    }
                    Token::Function => {
                        self.advance();
                        Err(ParseError::Block(BlockEnd::EndFunction))
                    }
                    Token::Select => {
                        self.advance();
                        Err(ParseError::Block(BlockEnd::EndSelect))
                    }
                    _ => Ok(StmtKind::End),
                }
            }
            Token::EndIf => {
                self.advance();
                Err(ParseError::Block(BlockEnd::EndIf))
            }
            Token::EndSub => {
                self.advance();
                Err(ParseError::Block(BlockEnd::EndSub))
            }
            Token::EndFunction => {
                self.advance();
                Err(ParseError::Block(BlockEnd::EndFunction))
            }
            Token::EndSelect => {
                self.advance();
                Err(ParseError::Block(BlockEnd::EndSelect))
            }
            Token::Stop => {
                self.advance();
                Ok(StmtKind::Stop)
            }
            Token::Next => {
                self.advance();
                // Skip optional variable name
                if let Token::Ident(_) = self.peek() {
                    self.advance();
                }
                Err(ParseError::Block(BlockEnd::Next))
            }
            Token::Wend => {
                self.advance();
                Err(ParseError::Block(BlockEnd::Wend))
            }
            Token::Loop => {
                self.advance();
                // Check for WHILE/UNTIL condition
                match self.peek() {
                    Token::While => {
                        self.advance();
                        let cond = self.parse_expression()?;
                        Err(ParseError::Block(BlockEnd::LoopWhile(cond)))
                    }
                    Token::Until => {
                        self.advance();
                        let cond = self.parse_expression()?;
                        Err(ParseError::Block(BlockEnd::LoopUntil(cond)))
                    }
                    _ => Err(ParseError::Block(BlockEnd::Loop)),
                }
            }
            Token::Else => {
                self.advance();
                Err(ParseError::Block(BlockEnd::Else))
            }
            Token::ElseIf => {
                self.advance();
                let cond = self.parse_expression()?;
                self.expect(Token::Then)?;
                Err(ParseError::Block(BlockEnd::ElseIf(cond)))
            }
            Token::Select => self.parse_select_case(),
            // `parse_select_case` consumes CASE itself, so a CASE reaching here
            // is always outside any SELECT CASE.
            Token::Case => err("CASE without matching SELECT CASE"),
            Token::Ident(_) => self.parse_assignment_or_call(),
            Token::Newline => {
                self.advance();
                self.parse_statement_kind()
            }
            _ => err(format!("Unexpected token: {:?}", self.peek())),
        }
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
                vars,
                file_num: Some(file_num),
            });
        }

        let mut prompt = None;
        let mut vars = Vec::new();

        // Check for prompt string
        if let Token::String(s) = self.peek().clone() {
            self.advance();
            prompt = Some(s);
            // Expect comma or semicolon after prompt
            if matches!(self.peek(), Token::Comma | Token::Semicolon) {
                self.advance();
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
            vars,
            file_num: None,
        })
    }

    fn parse_line_input(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume LINE
        self.expect(Token::Input)?;

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

        // Check for prompt string
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
            Expr::ArrayAccess { name, indices } => LValue {
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
        self.advance(); // consume IF
        let condition = self.parse_expression()?;
        self.expect(Token::Then)?;

        // Check for single-line IF
        if !matches!(self.peek(), Token::Newline | Token::Eof) {
            // Single-line IF
            let then_branch = vec![self.parse_statement()?];

            let else_branch = if matches!(self.peek(), Token::Else) {
                self.advance();
                Some(vec![self.parse_statement()?])
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
        let (then_branch, else_branch) = self.parse_if_body()?;

        Ok(StmtKind::If {
            condition,
            then_branch,
            else_branch,
        })
    }

    /// Parse the body of an IF block, returning (then_branch, else_branch)
    /// Handles ELSEIF by constructing nested IF statements in else_branch
    fn parse_if_body(&mut self) -> PResult<(Vec<Stmt>, Option<Vec<Stmt>>)> {
        let mut body = Vec::new();

        loop {
            // Captured before parsing so an ELSEIF's synthesized nested IF can
            // be attributed to the ELSEIF line rather than to END IF.
            let stmt_line = self.cur_line();
            match self.parse_statement() {
                Ok(stmt) => {
                    body.push(stmt);
                }
                Err(ParseError::Block(BlockEnd::EndIf)) => {
                    return Ok((body, None));
                }
                Err(ParseError::Block(BlockEnd::Else)) => {
                    // Parse ELSE body until END IF
                    self.skip_newlines();
                    let mut else_body = Vec::new();
                    loop {
                        match self.parse_statement() {
                            Ok(stmt) => else_body.push(stmt),
                            Err(ParseError::Block(BlockEnd::EndIf)) => break,
                            Err(e) => return Err(e),
                        }
                        self.skip_newlines();
                    }
                    return Ok((body, Some(else_body)));
                }
                Err(ParseError::Block(BlockEnd::ElseIf(elseif_condition))) => {
                    // Recursively parse the rest as a nested IF
                    self.skip_newlines();
                    let (nested_then, nested_else) = self.parse_if_body()?;

                    let nested_if = Stmt {
                        line: stmt_line,
                        kind: StmtKind::If {
                            condition: elseif_condition,
                            then_branch: nested_then,
                            else_branch: nested_else,
                        },
                    };

                    return Ok((body, Some(vec![nested_if])));
                }
                Err(e) => return Err(e),
            }
            self.skip_newlines();
        }
    }

    fn parse_for(&mut self) -> PResult<StmtKind> {
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

        let mut body = Vec::new();
        loop {
            match self.parse_statement() {
                Ok(stmt) => body.push(stmt),
                Err(ParseError::Block(BlockEnd::Next)) => break,
                Err(e) => return Err(e),
            }
            self.skip_newlines();
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
        self.advance(); // consume WHILE
        let condition = self.parse_expression()?;
        self.skip_newlines();

        let mut body = Vec::new();
        loop {
            match self.parse_statement() {
                Ok(stmt) => body.push(stmt),
                Err(ParseError::Block(BlockEnd::Wend)) => break,
                Err(e) => return Err(e),
            }
            self.skip_newlines();
        }

        Ok(StmtKind::While { condition, body })
    }

    fn parse_do_loop(&mut self) -> PResult<StmtKind> {
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

        let mut body = Vec::new();
        let mut end_condition: Option<Expr> = None;
        let mut end_is_until = false;

        loop {
            match self.parse_statement() {
                Ok(stmt) => body.push(stmt),
                Err(ParseError::Block(BlockEnd::Loop)) => break,
                Err(ParseError::Block(BlockEnd::LoopWhile(cond))) => {
                    end_condition = Some(cond);
                    end_is_until = false;
                    break;
                }
                Err(ParseError::Block(BlockEnd::LoopUntil(cond))) => {
                    end_condition = Some(cond);
                    end_is_until = true;
                    break;
                }
                Err(e) => return Err(e),
            }
            self.skip_newlines();
        }

        // Use end condition if no start condition, or start condition takes precedence
        let final_condition = condition.or(end_condition);

        Ok(StmtKind::DoLoop {
            condition: final_condition,
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
        self.advance(); // consume SELECT
        self.expect(Token::Case)?;
        let expr = self.parse_expression()?;
        self.skip_newlines();

        let mut cases: Vec<(Option<Vec<CaseClause>>, Vec<Stmt>)> = Vec::new();

        // Parse CASE blocks until END SELECT
        loop {
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

                body.push(self.parse_statement()?);
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
            tok => err(format!("Expected line number or label, got {:?}", tok)),
        }
    }

    fn parse_on_goto(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume ON
        let expr = self.parse_expression()?;
        self.expect(Token::Goto)?;

        let mut targets = Vec::new();
        loop {
            targets.push(self.parse_goto_target()?);
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(StmtKind::OnGoto { expr, targets })
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
                // Track the name so that `name(i)` parses as an array access
                // rather than a function call.
                self.declared_arrays.insert(name.to_uppercase());
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

        let mut body = Vec::new();
        loop {
            match self.parse_statement() {
                Ok(stmt) => body.push(stmt),
                Err(ParseError::Block(BlockEnd::EndSub)) => break,
                Err(e) => return Err(e),
            }
            self.skip_newlines();
        }

        Ok(StmtKind::Sub { name, params, body })
    }

    /// `FUNCTION name(params)` ... `END FUNCTION`
    fn parse_function(&mut self) -> PResult<StmtKind> {
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

        let mut body = Vec::new();
        loop {
            match self.parse_statement() {
                Ok(stmt) => body.push(stmt),
                Err(ParseError::Block(BlockEnd::EndFunction)) => break,
                Err(e) => return Err(e),
            }
            self.skip_newlines();
        }

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

    fn parse_data(&mut self) -> PResult<StmtKind> {
        self.advance(); // consume DATA
        let mut values = Vec::new();

        loop {
            match self.peek().clone() {
                Token::Integer(n) => {
                    self.advance();
                    values.push(Literal::Integer(n));
                }
                Token::Float(f) => {
                    self.advance();
                    values.push(Literal::Float(f));
                }
                Token::String(s) => {
                    self.advance();
                    values.push(Literal::String(s));
                }
                Token::Minus => {
                    self.advance();
                    match self.advance() {
                        Token::Integer(n) => values.push(Literal::Integer(-n)),
                        Token::Float(f) => values.push(Literal::Float(-f)),
                        _ => return err("Expected number after minus in DATA"),
                    }
                }
                _ => break,
            }
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(StmtKind::Data(values))
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

        // Parse mode (INPUT, OUTPUT, APPEND)
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
            tok => return err(format!("Expected INPUT, OUTPUT, or APPEND, got {:?}", tok)),
        };

        // Expect AS
        self.expect(Token::As)?;

        let file_num = self.parse_file_number()?;

        Ok(StmtKind::Open {
            filename,
            mode,
            file_num,
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

    fn parse_prec(&mut self, min_prec: u8) -> PResult<Expr> {
        // Handle NOT prefix operator (binds tighter than binary ops)
        let mut left = if matches!(self.peek(), Token::Not) {
            self.advance();
            let operand = self.parse_prec(min_prec)?; // NOT is right-associative
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
            // Power is right-associative; others are left-associative
            let next_min = if op == BinaryOp::Pow { prec } else { prec + 1 };
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

                    // Distinguish array access from function call based on DIM declarations
                    let base = if self.declared_arrays.contains(&name.to_uppercase()) {
                        Expr::ArrayAccess {
                            name,
                            indices: args,
                        }
                    } else {
                        Expr::FnCall { name, args }
                    };
                    self.parse_field_chain(base)
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
            tok => err(format!("Unexpected token in expression: {:?}", tok)),
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

    fn parse(input: &str) -> Result<Program, String> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize()?;
        let lines = lexer.line_map().to_vec();
        let mut parser = Parser::new(tokens, lines);
        parser.parse().map_err(|e| e.to_string())
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
    fn test_expr_power_right_associative() {
        // 2 ^ 3 ^ 2 should be 2 ^ (3 ^ 2) = 512, not (2 ^ 3) ^ 2 = 64
        let prog = parse("X = 2 ^ 3 ^ 2").unwrap();
        if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
            if let Expr::Binary { op, right, .. } = value {
                assert_eq!(*op, BinaryOp::Pow);
                assert!(matches!(
                    right.as_ref(),
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
        let prog = parse("X = A AND B OR C XOR D").unwrap();
        if let StmtKind::Let { value, .. } = &prog.statements[0].kind {
            // OR has lowest precedence, then XOR, then AND
            assert!(matches!(
                value,
                Expr::Binary {
                    op: BinaryOp::Or,
                    ..
                }
            ));
        } else {
            panic!("Expected Let");
        }
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
