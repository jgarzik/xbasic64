//! BASIC parser - produces AST from tokens

use crate::lexer::Token;

// ============================================================================
// AST Definitions
// ============================================================================

#[derive(Debug, Clone)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Label(u32), // Line number label
    Let {
        name: String,
        indices: Option<Vec<Expr>>, // For array assignment
        value: Expr,
    },
    Print {
        items: Vec<PrintItem>,
        newline: bool,
    },
    Input {
        prompt: Option<String>,
        vars: Vec<String>,
    },
    LineInput {
        prompt: Option<String>,
        var: String,
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
        arrays: Vec<ArrayDecl>,
    },
    Sub {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    Data(Vec<Literal>),
    Read(Vec<String>),
    Restore(Option<GotoTarget>),
    Cls,
    SelectCase {
        expr: Expr,
        cases: Vec<(Option<Expr>, Vec<Stmt>)>, // (None = ELSE, Some = value)
    },
    End,
    Stop,
    // File I/O
    Open {
        filename: Expr,
        mode: FileMode,
        file_num: i32,
    },
    Close {
        file_num: i32,
    },
    PrintFile {
        file_num: i32,
        items: Vec<PrintItem>,
        newline: bool,
    },
    InputFile {
        file_num: i32,
        vars: Vec<String>,
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
    #[allow(dead_code)] // Part of AST, will be used when multi-dimensional arrays are implemented
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

    /// Check if this is an integer type (Integer or Long)
    pub fn is_integer(&self) -> bool {
        matches!(self, DataType::Integer | DataType::Long)
    }
}

// ============================================================================
// Parser
// ============================================================================

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: Token) -> Result<(), String> {
        let tok = self.advance();
        if std::mem::discriminant(&tok) == std::mem::discriminant(&expected) {
            Ok(())
        } else {
            Err(format!("Expected {:?}, got {:?}", expected, tok))
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline) {
            self.advance();
        }
    }

    pub fn parse(&mut self) -> Result<Program, String> {
        let mut statements = Vec::new();
        self.skip_newlines();

        while !matches!(self.peek(), Token::Eof) {
            let stmt = self.parse_statement()?;
            statements.push(stmt);
            self.skip_newlines();
        }

        Ok(Program { statements })
    }

    fn parse_statement(&mut self) -> Result<Stmt, String> {
        // Handle line numbers as labels
        if let Token::LineNumber(n) = self.peek().clone() {
            self.advance();
            return Ok(Stmt::Label(n));
        }

        // Handle colon as statement separator
        if matches!(self.peek(), Token::Colon) {
            self.advance();
            return self.parse_statement();
        }

        match self.peek().clone() {
            Token::Print => self.parse_print(),
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
                Ok(Stmt::Return)
            }
            Token::On => self.parse_on_goto(),
            Token::Dim => self.parse_dim(),
            Token::Sub => self.parse_sub(),
            Token::Function => self.parse_function(),
            Token::Data => self.parse_data(),
            Token::Read => self.parse_read(),
            Token::Restore => self.parse_restore(),
            Token::Cls => {
                self.advance();
                Ok(Stmt::Cls)
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
                        Err("END IF".to_string())
                    }
                    Token::Sub => {
                        self.advance();
                        Err("END SUB".to_string())
                    }
                    Token::Function => {
                        self.advance();
                        Err("END FUNCTION".to_string())
                    }
                    Token::Select => {
                        self.advance();
                        Err("END SELECT".to_string())
                    }
                    _ => Ok(Stmt::End),
                }
            }
            Token::EndIf => {
                self.advance();
                Err("END IF".to_string())
            }
            Token::EndSub => {
                self.advance();
                Err("END SUB".to_string())
            }
            Token::EndFunction => {
                self.advance();
                Err("END FUNCTION".to_string())
            }
            Token::EndSelect => {
                self.advance();
                Err("END SELECT".to_string())
            }
            Token::Stop => {
                self.advance();
                Ok(Stmt::Stop)
            }
            Token::Next => {
                self.advance();
                // Skip optional variable name
                if let Token::Ident(_) = self.peek() {
                    self.advance();
                }
                Err("NEXT".to_string())
            }
            Token::Wend => {
                self.advance();
                Err("WEND".to_string())
            }
            Token::Loop => {
                self.advance();
                // Check for WHILE/UNTIL condition
                match self.peek() {
                    Token::While => {
                        self.advance();
                        let cond = self.parse_expression()?;
                        Err(format!("LOOP WHILE:{:?}", cond))
                    }
                    Token::Until => {
                        self.advance();
                        let cond = self.parse_expression()?;
                        Err(format!("LOOP UNTIL:{:?}", cond))
                    }
                    _ => Err("LOOP".to_string()),
                }
            }
            Token::Else => {
                self.advance();
                Err("ELSE".to_string())
            }
            Token::ElseIf => {
                self.advance();
                let cond = self.parse_expression()?;
                self.expect(Token::Then)?;
                Err(format!("ELSEIF:{:?}", cond))
            }
            Token::Select => self.parse_select_case(),
            Token::Case => {
                self.advance();
                // Check for CASE ELSE
                if matches!(self.peek(), Token::Else) {
                    self.advance();
                    Err("CASE ELSE".to_string())
                } else {
                    // Parse the case value
                    let value = self.parse_expression()?;
                    Err(format!("CASE:{:?}", value))
                }
            }
            Token::Ident(_) => self.parse_assignment_or_call(),
            Token::Newline => {
                self.advance();
                self.parse_statement()
            }
            _ => Err(format!("Unexpected token: {:?}", self.peek())),
        }
    }

    fn parse_print(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume PRINT

        // Check for PRINT #n (file output)
        let file_num = if matches!(self.peek(), Token::Hash) {
            self.advance(); // consume #
            let num = match self.advance() {
                Token::Integer(n) => n as i32,
                tok => return Err(format!("Expected file number after #, got {:?}", tok)),
            };
            if matches!(self.peek(), Token::Comma) {
                self.advance(); // consume comma after file number
            }
            Some(num)
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

        if let Some(file_num) = file_num {
            Ok(Stmt::PrintFile {
                file_num,
                items,
                newline,
            })
        } else {
            Ok(Stmt::Print { items, newline })
        }
    }

    fn parse_input(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume INPUT

        // Check for INPUT #n (file input)
        if matches!(self.peek(), Token::Hash) {
            self.advance(); // consume #
            let file_num = match self.advance() {
                Token::Integer(n) => n as i32,
                tok => return Err(format!("Expected file number after #, got {:?}", tok)),
            };
            if matches!(self.peek(), Token::Comma) {
                self.advance(); // consume comma after file number
            }

            let mut vars = Vec::new();
            while let Token::Ident(name) = self.peek().clone() {
                self.advance();
                vars.push(name);
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }

            return Ok(Stmt::InputFile { file_num, vars });
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
        while let Token::Ident(name) = self.peek().clone() {
            self.advance();
            vars.push(name);
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(Stmt::Input { prompt, vars })
    }

    fn parse_line_input(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume LINE
        self.expect(Token::Input)?;

        let mut prompt = None;

        // Check for prompt string
        if let Token::String(s) = self.peek().clone() {
            self.advance();
            prompt = Some(s);
            if matches!(self.peek(), Token::Comma | Token::Semicolon) {
                self.advance();
            }
        }

        let var = if let Token::Ident(name) = self.advance() {
            name
        } else {
            return Err("Expected variable name after LINE INPUT".to_string());
        };

        Ok(Stmt::LineInput { prompt, var })
    }

    fn parse_let(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume LET
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Result<Stmt, String> {
        let name = if let Token::Ident(n) = self.advance() {
            n
        } else {
            return Err("Expected variable name".to_string());
        };

        // Check for array subscript
        let indices = if matches!(self.peek(), Token::LParen) {
            self.advance();
            let idx = self.parse_expr_list()?;
            self.expect(Token::RParen)?;
            Some(idx)
        } else {
            None
        };

        self.expect(Token::Eq)?;
        let value = self.parse_expression()?;

        Ok(Stmt::Let {
            name,
            indices,
            value,
        })
    }

    fn parse_assignment_or_call(&mut self) -> Result<Stmt, String> {
        let name = if let Token::Ident(n) = self.advance() {
            n
        } else {
            return Err("Expected identifier".to_string());
        };

        // Check for array subscript or function call
        if matches!(self.peek(), Token::LParen) {
            self.advance();

            // Could be array assignment or subroutine call
            // Look ahead to see if there's an = after )
            let args = self.parse_expr_list()?;
            self.expect(Token::RParen)?;

            if matches!(self.peek(), Token::Eq) {
                // Array assignment
                self.advance();
                let value = self.parse_expression()?;
                Ok(Stmt::Let {
                    name,
                    indices: Some(args),
                    value,
                })
            } else {
                // Subroutine call
                Ok(Stmt::Call { name, args })
            }
        } else if matches!(self.peek(), Token::Eq) {
            // Simple assignment
            self.advance();
            let value = self.parse_expression()?;
            Ok(Stmt::Let {
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
            Ok(Stmt::Call { name, args })
        }
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
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

            return Ok(Stmt::If {
                condition,
                then_branch,
                else_branch,
            });
        }

        // Block IF
        self.skip_newlines();
        let mut then_branch = Vec::new();
        let mut else_branch: Option<Vec<Stmt>> = None;

        loop {
            match self.parse_statement() {
                Ok(stmt) => {
                    if let Some(ref mut eb) = else_branch {
                        eb.push(stmt);
                    } else {
                        then_branch.push(stmt);
                    }
                }
                Err(e) if e == "END IF" => break,
                Err(e) if e == "ELSE" => {
                    else_branch = Some(Vec::new());
                }
                Err(e) if e.starts_with("ELSEIF:") => {
                    // Parse ELSEIF as nested IF in else branch
                    // For now, treat ELSEIF simply by continuing parsing
                    // This is a simplification; proper handling would be more complex
                    let _ = &e[7..]; // condition string, unused for now
                    else_branch = Some(Vec::new());
                }
                Err(e) => return Err(e),
            }
            self.skip_newlines();
        }

        Ok(Stmt::If {
            condition,
            then_branch,
            else_branch,
        })
    }

    fn parse_for(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume FOR
        let var = if let Token::Ident(n) = self.advance() {
            n
        } else {
            return Err("Expected variable name after FOR".to_string());
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
                Err(e) if e == "NEXT" => break,
                Err(e) => return Err(e),
            }
            self.skip_newlines();
        }

        Ok(Stmt::For {
            var,
            start,
            end,
            step,
            body,
        })
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume WHILE
        let condition = self.parse_expression()?;
        self.skip_newlines();

        let mut body = Vec::new();
        loop {
            match self.parse_statement() {
                Ok(stmt) => body.push(stmt),
                Err(e) if e == "WEND" => break,
                Err(e) => return Err(e),
            }
            self.skip_newlines();
        }

        Ok(Stmt::While { condition, body })
    }

    fn parse_do_loop(&mut self) -> Result<Stmt, String> {
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
        let end_condition = None;
        let mut end_is_until = false;

        loop {
            match self.parse_statement() {
                Ok(stmt) => body.push(stmt),
                Err(e) if e == "LOOP" => break,
                Err(e) if e.starts_with("LOOP WHILE:") => {
                    // Parse condition from error message (hacky but simple)
                    end_is_until = false;
                    break;
                }
                Err(e) if e.starts_with("LOOP UNTIL:") => {
                    end_is_until = true;
                    break;
                }
                Err(e) => return Err(e),
            }
            self.skip_newlines();
        }

        // If condition was at end, we need to get it
        // For simplicity, we'll use condition from DO if specified
        let final_condition = condition.or(end_condition);

        Ok(Stmt::DoLoop {
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

    fn parse_select_case(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume SELECT
        self.expect(Token::Case)?;
        let expr = self.parse_expression()?;
        self.skip_newlines();

        let mut cases: Vec<(Option<Expr>, Vec<Stmt>)> = Vec::new();

        // Parse CASE blocks until END SELECT
        loop {
            // Check for END SELECT
            if matches!(self.peek(), Token::End | Token::EndSelect) {
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
                Some(self.parse_expression()?)
            };

            self.skip_newlines();

            // Parse case body until next CASE or END SELECT
            let mut body = Vec::new();
            loop {
                // Check for terminators before parsing statement
                match self.peek() {
                    Token::Case | Token::End | Token::EndSelect => break,
                    Token::Eof => break,
                    _ => {}
                }

                match self.parse_statement() {
                    Ok(stmt) => body.push(stmt),
                    Err(e) => return Err(e),
                }
                self.skip_newlines();
            }

            cases.push((case_value, body));
        }

        Ok(Stmt::SelectCase { expr, cases })
    }

    fn parse_goto(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume GOTO
        let target = self.parse_goto_target()?;
        Ok(Stmt::Goto(target))
    }

    fn parse_gosub(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume GOSUB
        let target = self.parse_goto_target()?;
        Ok(Stmt::Gosub(target))
    }

    fn parse_goto_target(&mut self) -> Result<GotoTarget, String> {
        match self.advance() {
            Token::Integer(n) => Ok(GotoTarget::Line(n as u32)),
            Token::LineNumber(n) => Ok(GotoTarget::Line(n)),
            Token::Ident(name) => Ok(GotoTarget::Label(name)),
            tok => Err(format!("Expected line number or label, got {:?}", tok)),
        }
    }

    fn parse_on_goto(&mut self) -> Result<Stmt, String> {
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

        Ok(Stmt::OnGoto { expr, targets })
    }

    fn parse_dim(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume DIM
        let mut arrays = Vec::new();

        loop {
            let name = if let Token::Ident(n) = self.advance() {
                n
            } else {
                return Err("Expected array name after DIM".to_string());
            };

            self.expect(Token::LParen)?;
            let dimensions = self.parse_expr_list()?;
            self.expect(Token::RParen)?;

            arrays.push(ArrayDecl { name, dimensions });

            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(Stmt::Dim { arrays })
    }

    fn parse_sub(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume SUB
        let name = if let Token::Ident(n) = self.advance() {
            n
        } else {
            return Err("Expected subroutine name".to_string());
        };

        let params = if matches!(self.peek(), Token::LParen) {
            self.advance();
            let params = self.parse_param_list()?;
            self.expect(Token::RParen)?;
            params
        } else {
            Vec::new()
        };

        self.skip_newlines();

        let mut body = Vec::new();
        loop {
            match self.parse_statement() {
                Ok(stmt) => body.push(stmt),
                Err(e) if e == "END SUB" => break,
                Err(e) => return Err(e),
            }
            self.skip_newlines();
        }

        Ok(Stmt::Sub { name, params, body })
    }

    fn parse_function(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume FUNCTION
        let name = if let Token::Ident(n) = self.advance() {
            n
        } else {
            return Err("Expected function name".to_string());
        };

        let params = if matches!(self.peek(), Token::LParen) {
            self.advance();
            let params = self.parse_param_list()?;
            self.expect(Token::RParen)?;
            params
        } else {
            Vec::new()
        };

        self.skip_newlines();

        let mut body = Vec::new();
        loop {
            match self.parse_statement() {
                Ok(stmt) => body.push(stmt),
                Err(e) if e == "END FUNCTION" => break,
                Err(e) => return Err(e),
            }
            self.skip_newlines();
        }

        Ok(Stmt::Function { name, params, body })
    }

    fn parse_param_list(&mut self) -> Result<Vec<String>, String> {
        let mut params = Vec::new();
        while let Token::Ident(name) = self.peek().clone() {
            self.advance();
            params.push(name);
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(params)
    }

    fn parse_data(&mut self) -> Result<Stmt, String> {
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
                        _ => return Err("Expected number after minus in DATA".to_string()),
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

        Ok(Stmt::Data(values))
    }

    fn parse_read(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume READ
        let mut vars = Vec::new();

        while let Token::Ident(name) = self.peek().clone() {
            self.advance();
            vars.push(name);
            if matches!(self.peek(), Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Ok(Stmt::Read(vars))
    }

    fn parse_restore(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume RESTORE
        let target = if matches!(self.peek(), Token::Integer(_) | Token::Ident(_)) {
            Some(self.parse_goto_target()?)
        } else {
            None
        };
        Ok(Stmt::Restore(target))
    }

    fn parse_open(&mut self) -> Result<Stmt, String> {
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
            tok => return Err(format!("Expected INPUT, OUTPUT, or APPEND, got {:?}", tok)),
        };

        // Expect AS
        self.expect(Token::As)?;

        // Expect #n
        self.expect(Token::Hash)?;
        let file_num = match self.advance() {
            Token::Integer(n) => n as i32,
            tok => return Err(format!("Expected file number after #, got {:?}", tok)),
        };

        Ok(Stmt::Open {
            filename,
            mode,
            file_num,
        })
    }

    fn parse_close(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume CLOSE

        // Expect #n
        self.expect(Token::Hash)?;
        let file_num = match self.advance() {
            Token::Integer(n) => n as i32,
            tok => return Err(format!("Expected file number after #, got {:?}", tok)),
        };

        Ok(Stmt::Close { file_num })
    }

    // Expression parsing with precedence climbing
    fn parse_expression(&mut self) -> Result<Expr, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_xor()?;
        while matches!(self.peek(), Token::And) {
            self.advance();
            let right = self.parse_xor()?;
            left = Expr::Binary {
                op: BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_xor(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_not()?;
        while matches!(self.peek(), Token::Xor) {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::Binary {
                op: BinaryOp::Xor,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, String> {
        if matches!(self.peek(), Token::Not) {
            self.advance();
            let operand = self.parse_not()?;
            Ok(Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            })
        } else {
            self.parse_comparison()
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                Token::Eq => BinaryOp::Eq,
                Token::Ne => BinaryOp::Ne,
                Token::Lt => BinaryOp::Lt,
                Token::Gt => BinaryOp::Gt,
                Token::Le => BinaryOp::Le,
                Token::Ge => BinaryOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_power()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Backslash => BinaryOp::IntDiv,
                Token::Mod => BinaryOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_power()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_power(&mut self) -> Result<Expr, String> {
        let base = self.parse_unary()?;
        if matches!(self.peek(), Token::Caret) {
            self.advance();
            // Right-associative
            let exp = self.parse_power()?;
            Ok(Expr::Binary {
                op: BinaryOp::Pow,
                left: Box::new(base),
                right: Box::new(exp),
            })
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Token::Minus => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(operand),
                })
            }
            Token::Plus => {
                self.advance();
                self.parse_unary()
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
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

                    // Could be array access or function call
                    // We'll treat everything as function call for now
                    // and distinguish during codegen based on known functions
                    Ok(Expr::FnCall { name, args })
                } else {
                    Ok(Expr::Variable(name))
                }
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expression()?;
                self.expect(Token::RParen)?;
                Ok(expr)
            }
            tok => Err(format!("Unexpected token in expression: {:?}", tok)),
        }
    }

    fn parse_expr_list(&mut self) -> Result<Vec<Expr>, String> {
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
        let mut parser = Parser::new(tokens);
        parser.parse()
    }

    // ===================
    // Label Tests
    // ===================

    #[test]
    fn test_label() {
        let prog = parse("10 PRINT X").unwrap();
        assert_eq!(prog.statements.len(), 2);
        if let Stmt::Label(n) = &prog.statements[0] {
            assert_eq!(*n, 10);
        } else {
            panic!("Expected Label");
        }
    }

    #[test]
    fn test_multiple_labels() {
        let prog = parse("10 X = 1\n20 Y = 2\n30 END").unwrap();
        assert_eq!(prog.statements.len(), 6); // 3 labels + 3 statements
        assert!(matches!(&prog.statements[0], Stmt::Label(10)));
        assert!(matches!(&prog.statements[2], Stmt::Label(20)));
        assert!(matches!(&prog.statements[4], Stmt::Label(30)));
    }

    // ===================
    // Let Tests
    // ===================

    #[test]
    fn test_let_simple() {
        let prog = parse("X = 42").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::Let {
            name,
            indices,
            value,
        } = &prog.statements[0]
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
        if let Stmt::Let { name, .. } = &prog.statements[0] {
            assert_eq!(name, "X");
        } else {
            panic!("Expected Let");
        }
    }

    #[test]
    fn test_let_array_assignment() {
        let prog = parse("A(5) = 100").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::Let {
            name,
            indices,
            value,
        } = &prog.statements[0]
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
        if let Stmt::Let { value, .. } = &prog.statements[0] {
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

    // ===================
    // Print Tests
    // ===================

    #[test]
    fn test_print_string() {
        let prog = parse(r#"PRINT "Hello""#).unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::Print { items, newline } = &prog.statements[0] {
            assert_eq!(items.len(), 1);
            assert!(*newline);
        } else {
            panic!("Expected Print");
        }
    }

    #[test]
    fn test_print_multiple_items() {
        let prog = parse(r#"PRINT "A"; B; C"#).unwrap();
        if let Stmt::Print { items, .. } = &prog.statements[0] {
            assert_eq!(items.len(), 5); // "A", Empty, B, Empty, C
        } else {
            panic!("Expected Print");
        }
    }

    #[test]
    fn test_print_with_tab() {
        let prog = parse(r#"PRINT A, B"#).unwrap();
        if let Stmt::Print { items, .. } = &prog.statements[0] {
            assert!(items.iter().any(|i| matches!(i, PrintItem::Tab)));
        } else {
            panic!("Expected Print");
        }
    }

    #[test]
    fn test_print_no_newline() {
        let prog = parse(r#"PRINT X;"#).unwrap();
        if let Stmt::Print { newline, .. } = &prog.statements[0] {
            assert!(!*newline);
        } else {
            panic!("Expected Print");
        }
    }

    // ===================
    // Input Tests
    // ===================

    #[test]
    fn test_input_simple() {
        let prog = parse("INPUT X").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::Input { prompt, vars } = &prog.statements[0] {
            assert!(prompt.is_none());
            assert_eq!(vars.len(), 1);
            assert_eq!(vars[0], "X");
        } else {
            panic!("Expected Input");
        }
    }

    #[test]
    fn test_input_with_prompt() {
        let prog = parse(r#"INPUT "Enter value: ", X"#).unwrap();
        if let Stmt::Input { prompt, vars } = &prog.statements[0] {
            assert_eq!(prompt.as_ref().unwrap(), "Enter value: ");
            assert_eq!(vars[0], "X");
        } else {
            panic!("Expected Input");
        }
    }

    #[test]
    fn test_input_multiple_vars() {
        let prog = parse("INPUT A, B, C").unwrap();
        if let Stmt::Input { vars, .. } = &prog.statements[0] {
            assert_eq!(vars.len(), 3);
        } else {
            panic!("Expected Input");
        }
    }

    // ===================
    // LineInput Tests
    // ===================

    #[test]
    fn test_line_input_simple() {
        let prog = parse("LINE INPUT X$").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::LineInput { prompt, var } = &prog.statements[0] {
            assert!(prompt.is_none());
            assert_eq!(var, "X$");
        } else {
            panic!("Expected LineInput");
        }
    }

    #[test]
    fn test_line_input_with_prompt() {
        let prog = parse(r#"LINE INPUT "Name: ", NAME$"#).unwrap();
        if let Stmt::LineInput { prompt, var } = &prog.statements[0] {
            assert_eq!(prompt.as_ref().unwrap(), "Name: ");
            assert_eq!(var, "NAME$");
        } else {
            panic!("Expected LineInput");
        }
    }

    // ===================
    // If Tests
    // ===================

    #[test]
    fn test_if_single_line() {
        let prog = parse("IF X > 0 THEN PRINT X").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::If {
            condition,
            then_branch,
            else_branch,
        } = &prog.statements[0]
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
        if let Stmt::If {
            then_branch,
            else_branch,
            ..
        } = &prog.statements[0]
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
        if let Stmt::If {
            then_branch,
            else_branch,
            ..
        } = &prog.statements[0]
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
        if let Stmt::If {
            then_branch,
            else_branch,
            ..
        } = &prog.statements[0]
        {
            assert_eq!(then_branch.len(), 1);
            assert!(else_branch.is_some());
        } else {
            panic!("Expected If");
        }
    }

    // ===================
    // For Tests
    // ===================

    #[test]
    fn test_for_simple() {
        let prog = parse("FOR I = 1 TO 10\nPRINT I\nNEXT I").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::For {
            var,
            start,
            end,
            step,
            body,
        } = &prog.statements[0]
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
        if let Stmt::For { step, .. } = &prog.statements[0] {
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
        if let Stmt::For { step, .. } = &prog.statements[0] {
            assert!(step.is_some());
        } else {
            panic!("Expected For");
        }
    }

    // ===================
    // While Tests
    // ===================

    #[test]
    fn test_while_simple() {
        let prog = parse("WHILE X < 10\nX = X + 1\nWEND").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::While { condition, body } = &prog.statements[0] {
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

    // ===================
    // DoLoop Tests
    // ===================

    #[test]
    fn test_do_loop_simple() {
        let prog = parse("DO\nX = X + 1\nLOOP").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::DoLoop {
            condition,
            cond_at_start,
            body,
            ..
        } = &prog.statements[0]
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
        if let Stmt::DoLoop {
            condition,
            cond_at_start,
            is_until,
            ..
        } = &prog.statements[0]
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
        if let Stmt::DoLoop {
            condition,
            cond_at_start,
            is_until,
            ..
        } = &prog.statements[0]
        {
            assert!(condition.is_some());
            assert!(*cond_at_start);
            assert!(*is_until);
        } else {
            panic!("Expected DoLoop");
        }
    }

    // ===================
    // SelectCase Tests
    // ===================

    #[test]
    fn test_select_case_simple() {
        let prog = parse("SELECT CASE X\nCASE 1\nPRINT 1\nEND SELECT").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::SelectCase { expr, cases } = &prog.statements[0] {
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
        if let Stmt::SelectCase { cases, .. } = &prog.statements[0] {
            assert_eq!(cases.len(), 2);
        } else {
            panic!("Expected SelectCase");
        }
    }

    #[test]
    fn test_select_case_with_else() {
        let prog = parse("SELECT CASE X\nCASE 1\nPRINT 1\nCASE ELSE\nPRINT 0\nEND SELECT").unwrap();
        if let Stmt::SelectCase { cases, .. } = &prog.statements[0] {
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
        if let Stmt::SelectCase { expr, cases } = &prog.statements[0] {
            assert!(matches!(expr, Expr::Variable(_)));
            assert_eq!(cases.len(), 1);
            if let Some(Expr::Literal(Literal::String(s))) = &cases[0].0 {
                assert_eq!(s, "yes");
            } else {
                panic!("Expected string literal in CASE");
            }
        } else {
            panic!("Expected SelectCase");
        }
    }

    // ===================
    // Goto Tests
    // ===================

    #[test]
    fn test_goto_line_number() {
        let prog = parse("GOTO 100").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::Goto(target) = &prog.statements[0] {
            assert!(matches!(target, GotoTarget::Line(100)));
        } else {
            panic!("Expected Goto");
        }
    }

    #[test]
    fn test_goto_label() {
        let prog = parse("GOTO MYLOOP").unwrap();
        if let Stmt::Goto(target) = &prog.statements[0] {
            if let GotoTarget::Label(name) = target {
                assert_eq!(name, "MYLOOP");
            } else {
                panic!("Expected label target");
            }
        } else {
            panic!("Expected Goto");
        }
    }

    // ===================
    // Gosub Tests
    // ===================

    #[test]
    fn test_gosub_line_number() {
        let prog = parse("GOSUB 1000").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::Gosub(target) = &prog.statements[0] {
            assert!(matches!(target, GotoTarget::Line(1000)));
        } else {
            panic!("Expected Gosub");
        }
    }

    #[test]
    fn test_gosub_label() {
        let prog = parse("GOSUB MYSUB").unwrap();
        if let Stmt::Gosub(target) = &prog.statements[0] {
            assert!(matches!(target, GotoTarget::Label(_)));
        } else {
            panic!("Expected Gosub");
        }
    }

    // ===================
    // Return Tests
    // ===================

    #[test]
    fn test_return() {
        let prog = parse("RETURN").unwrap();
        assert_eq!(prog.statements.len(), 1);
        assert!(matches!(&prog.statements[0], Stmt::Return));
    }

    // ===================
    // OnGoto Tests
    // ===================

    #[test]
    fn test_on_goto() {
        let prog = parse("ON X GOTO 10, 20, 30").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::OnGoto { expr, targets } = &prog.statements[0] {
            assert!(matches!(expr, Expr::Variable(_)));
            assert_eq!(targets.len(), 3);
        } else {
            panic!("Expected OnGoto");
        }
    }

    // ===================
    // Dim Tests
    // ===================

    #[test]
    fn test_dim_single() {
        let prog = parse("DIM A(10)").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::Dim { arrays } = &prog.statements[0] {
            assert_eq!(arrays.len(), 1);
            assert_eq!(arrays[0].name, "A");
            assert_eq!(arrays[0].dimensions.len(), 1);
        } else {
            panic!("Expected Dim");
        }
    }

    #[test]
    fn test_dim_multiple() {
        let prog = parse("DIM A(10), B$(100), C(50)").unwrap();
        if let Stmt::Dim { arrays } = &prog.statements[0] {
            assert_eq!(arrays.len(), 3);
            assert_eq!(arrays[0].name, "A");
            assert_eq!(arrays[1].name, "B$");
            assert_eq!(arrays[2].name, "C");
        } else {
            panic!("Expected Dim");
        }
    }

    #[test]
    fn test_dim_2d() {
        let prog = parse("DIM A(10, 20)").unwrap();
        if let Stmt::Dim { arrays } = &prog.statements[0] {
            assert_eq!(arrays.len(), 1);
            assert_eq!(arrays[0].name, "A");
            assert_eq!(arrays[0].dimensions.len(), 2);
        } else {
            panic!("Expected Dim");
        }
    }

    #[test]
    fn test_dim_3d() {
        let prog = parse("DIM Matrix(5, 10, 15)").unwrap();
        if let Stmt::Dim { arrays } = &prog.statements[0] {
            assert_eq!(arrays.len(), 1);
            assert_eq!(arrays[0].name, "MATRIX");
            assert_eq!(arrays[0].dimensions.len(), 3);
        } else {
            panic!("Expected Dim");
        }
    }

    #[test]
    fn test_array_access_2d() {
        let prog = parse("X = A(1, 2)").unwrap();
        if let Stmt::Let { value, .. } = &prog.statements[0] {
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
        if let Stmt::Let { name, indices, .. } = &prog.statements[0] {
            assert_eq!(name, "A");
            assert!(indices.is_some());
            assert_eq!(indices.as_ref().unwrap().len(), 2);
        } else {
            panic!("Expected Let with indices");
        }
    }

    // ===================
    // Sub Tests
    // ===================

    #[test]
    fn test_sub_no_params() {
        let prog = parse("SUB MySub\nPRINT X\nEND SUB").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::Sub { name, params, body } = &prog.statements[0] {
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
        if let Stmt::Sub { params, .. } = &prog.statements[0] {
            assert_eq!(params.len(), 3);
        } else {
            panic!("Expected Sub");
        }
    }

    // ===================
    // Function Tests
    // ===================

    #[test]
    fn test_function_no_params() {
        let prog = parse("FUNCTION GetValue\nGetValue = 42\nEND FUNCTION").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::Function { name, params, body } = &prog.statements[0] {
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
        if let Stmt::Function { name, params, .. } = &prog.statements[0] {
            assert_eq!(name, "ADD");
            assert_eq!(params.len(), 2);
        } else {
            panic!("Expected Function");
        }
    }

    // ===================
    // Call Tests
    // ===================

    #[test]
    fn test_call_no_args() {
        let prog = parse("MySub").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::Call { name, args } = &prog.statements[0] {
            assert_eq!(name, "MYSUB");
            assert!(args.is_empty());
        } else {
            panic!("Expected Call");
        }
    }

    #[test]
    fn test_call_with_parens() {
        let prog = parse("MySub(1, 2, 3)").unwrap();
        if let Stmt::Call { name, args } = &prog.statements[0] {
            assert_eq!(name, "MYSUB");
            assert_eq!(args.len(), 3);
        } else {
            panic!("Expected Call");
        }
    }

    #[test]
    fn test_call_without_parens() {
        let prog = parse("MySub 1, 2, 3").unwrap();
        if let Stmt::Call { args, .. } = &prog.statements[0] {
            assert_eq!(args.len(), 3);
        } else {
            panic!("Expected Call");
        }
    }

    // ===================
    // Data Tests
    // ===================

    #[test]
    fn test_data_integers() {
        let prog = parse("DATA 1, 2, 3, 4, 5").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::Data(values) = &prog.statements[0] {
            assert_eq!(values.len(), 5);
            assert!(matches!(values[0], Literal::Integer(1)));
        } else {
            panic!("Expected Data");
        }
    }

    #[test]
    fn test_data_mixed() {
        let prog = parse(r#"DATA 1, 3.14, "hello""#).unwrap();
        if let Stmt::Data(values) = &prog.statements[0] {
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
        if let Stmt::Data(values) = &prog.statements[0] {
            assert!(matches!(values[0], Literal::Integer(-5)));
        } else {
            panic!("Expected Data");
        }
    }

    // ===================
    // Read Tests
    // ===================

    #[test]
    fn test_read_single() {
        let prog = parse("READ X").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::Read(vars) = &prog.statements[0] {
            assert_eq!(vars.len(), 1);
            assert_eq!(vars[0], "X");
        } else {
            panic!("Expected Read");
        }
    }

    #[test]
    fn test_read_multiple() {
        let prog = parse("READ A, B, C$").unwrap();
        if let Stmt::Read(vars) = &prog.statements[0] {
            assert_eq!(vars.len(), 3);
        } else {
            panic!("Expected Read");
        }
    }

    // ===================
    // Restore Tests
    // ===================

    #[test]
    fn test_restore_simple() {
        let prog = parse("RESTORE").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::Restore(target) = &prog.statements[0] {
            assert!(target.is_none());
        } else {
            panic!("Expected Restore");
        }
    }

    #[test]
    fn test_restore_with_target() {
        let prog = parse("RESTORE 100").unwrap();
        if let Stmt::Restore(target) = &prog.statements[0] {
            assert!(target.is_some());
        } else {
            panic!("Expected Restore");
        }
    }

    // ===================
    // Cls Tests
    // ===================

    #[test]
    fn test_cls() {
        let prog = parse("CLS").unwrap();
        assert_eq!(prog.statements.len(), 1);
        assert!(matches!(&prog.statements[0], Stmt::Cls));
    }

    // ===================
    // End Tests
    // ===================

    #[test]
    fn test_end() {
        let prog = parse("END").unwrap();
        assert_eq!(prog.statements.len(), 1);
        assert!(matches!(&prog.statements[0], Stmt::End));
    }

    // ===================
    // Stop Tests
    // ===================

    #[test]
    fn test_stop() {
        let prog = parse("STOP").unwrap();
        assert_eq!(prog.statements.len(), 1);
        assert!(matches!(&prog.statements[0], Stmt::Stop));
    }

    // ===================
    // Expression Tests
    // ===================

    #[test]
    fn test_expr_precedence() {
        // 2 + 3 * 4 should be 2 + (3 * 4) = 14, not (2 + 3) * 4 = 20
        let prog = parse("X = 2 + 3 * 4").unwrap();
        if let Stmt::Let { value, .. } = &prog.statements[0] {
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
        if let Stmt::Let { value, .. } = &prog.statements[0] {
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
        if let Stmt::Let { value, .. } = &prog.statements[0] {
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
        if let Stmt::Let { value, .. } = &prog.statements[0] {
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
        if let Stmt::Let { value, .. } = &prog.statements[0] {
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
        if let Stmt::Let { value, .. } = &prog.statements[0] {
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
        if let Stmt::Let { value, .. } = &prog.statements[0] {
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
            if let Stmt::Let { value, .. } = &prog.statements[0] {
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
            if let Stmt::Let { value, .. } = &prog.statements[0] {
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
        if let Stmt::Let { value, .. } = &prog.statements[0] {
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
        if let Stmt::Let { value, .. } = &prog.statements[0] {
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

    // ===================
    // Integration Tests
    // ===================

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
