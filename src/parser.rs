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
    End,
    Stop,
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
            Token::End => {
                self.advance();
                // Check for END IF, END SUB, END FUNCTION
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

        Ok(Stmt::Print { items, newline })
    }

    fn parse_input(&mut self) -> Result<Stmt, String> {
        self.advance(); // consume INPUT
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

    #[test]
    fn test_print() {
        let prog = parse(r#"PRINT "Hello""#).unwrap();
        assert_eq!(prog.statements.len(), 1);
    }

    #[test]
    fn test_assignment() {
        let prog = parse("X = 42").unwrap();
        assert_eq!(prog.statements.len(), 1);
    }

    #[test]
    fn test_expression() {
        let prog = parse("X = 1 + 2 * 3").unwrap();
        assert_eq!(prog.statements.len(), 1);
    }

    #[test]
    fn test_for_loop() {
        let prog = parse("FOR I = 1 TO 10\nPRINT I\nNEXT I").unwrap();
        assert_eq!(prog.statements.len(), 1);
        if let Stmt::For { body, .. } = &prog.statements[0] {
            assert_eq!(body.len(), 1);
        } else {
            panic!("Expected FOR statement");
        }
    }
}
