//! BASIC lexer - tokenizes source into tokens

use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Integer(i64),
    Float(f64),
    String(String),

    // Identifier with optional type suffix
    Ident(String),

    // Keywords
    Print,
    Input,
    Line,
    Let,
    Dim,
    If,
    Then,
    Else,
    ElseIf,
    EndIf,
    For,
    To,
    Step,
    Next,
    While,
    Wend,
    Do,
    Loop,
    Until,
    Goto,
    Gosub,
    Return,
    On,
    Sub,
    EndSub,
    Function,
    EndFunction,
    Select,
    Case,
    EndSelect,
    End,
    Stop,
    Rem,
    Data,
    Read,
    Restore,
    Cls,
    Open,
    Close,
    As,
    Output,
    Append,
    And,
    Or,
    Not,
    Xor,
    Mod,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Backslash,
    Caret,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,

    // Punctuation
    LParen,
    RParen,
    Comma,
    Semicolon,
    Colon,
    Hash,

    // Special
    Newline,
    LineNumber(u32),
    Eof,
}

pub struct Lexer<'a> {
    input: &'a str,
    chars: Peekable<Chars<'a>>,
    pos: usize,
    line: u32,
    at_line_start: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input,
            chars: input.chars().peekable(),
            pos: 0,
            line: 1,
            at_line_start: true,
        }
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.next();
        if let Some(ch) = c {
            self.pos += ch.len_utf8();
        }
        c
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c == ' ' || c == '\t' || c == '\r' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) {
        // Skip until newline
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.advance();
        }
    }

    fn read_string(&mut self) -> Result<String, String> {
        let mut s = String::new();
        self.advance(); // consume opening "
        loop {
            match self.advance() {
                Some('"') => {
                    // Check for escaped quote ""
                    if self.peek() == Some('"') {
                        self.advance();
                        s.push('"');
                    } else {
                        break;
                    }
                }
                Some('\n') | None => {
                    return Err("Unterminated string".to_string());
                }
                Some(c) => s.push(c),
            }
        }
        Ok(s)
    }

    fn read_number(&mut self, first: char) -> Token {
        let mut s = String::new();
        s.push(first);

        let mut is_float = false;
        let mut has_exponent = false;

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(self.advance().unwrap());
            } else if c == '.' && !is_float && !has_exponent {
                is_float = true;
                s.push(self.advance().unwrap());
            } else if (c == 'e' || c == 'E' || c == 'd' || c == 'D') && !has_exponent {
                has_exponent = true;
                is_float = true;
                s.push(self.advance().unwrap());
                // Handle optional sign after exponent
                if let Some(sign) = self.peek() {
                    if sign == '+' || sign == '-' {
                        s.push(self.advance().unwrap());
                    }
                }
            } else {
                break;
            }
        }

        // Replace D with E for parsing
        let s = s.replace(['d', 'D'], "e");

        if is_float {
            Token::Float(s.parse().unwrap_or(0.0))
        } else {
            Token::Integer(s.parse().unwrap_or(0))
        }
    }

    fn read_hex(&mut self) -> Token {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_hexdigit() {
                s.push(self.advance().unwrap());
            } else {
                break;
            }
        }
        let val = i64::from_str_radix(&s, 16).unwrap_or(0);
        Token::Integer(val)
    }

    fn read_identifier(&mut self, first: char) -> String {
        let mut s = String::new();
        s.push(first.to_ascii_uppercase());

        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' {
                s.push(self.advance().unwrap().to_ascii_uppercase());
            } else {
                break;
            }
        }

        // Check for type suffix
        if let Some(c) = self.peek() {
            if c == '%' || c == '&' || c == '!' || c == '#' || c == '$' {
                s.push(self.advance().unwrap());
            }
        }

        s
    }

    fn keyword_or_ident(&self, s: &str) -> Token {
        // Strip type suffix for keyword matching
        let base = s.trim_end_matches(['%', '&', '!', '#', '$']);

        match base {
            "PRINT" => Token::Print,
            "INPUT" => Token::Input,
            "LINE" => Token::Line,
            "LET" => Token::Let,
            "DIM" => Token::Dim,
            "IF" => Token::If,
            "THEN" => Token::Then,
            "ELSE" => Token::Else,
            "ELSEIF" => Token::ElseIf,
            "ENDIF" => Token::EndIf,
            "FOR" => Token::For,
            "TO" => Token::To,
            "STEP" => Token::Step,
            "NEXT" => Token::Next,
            "WHILE" => Token::While,
            "WEND" => Token::Wend,
            "DO" => Token::Do,
            "LOOP" => Token::Loop,
            "UNTIL" => Token::Until,
            "GOTO" => Token::Goto,
            "GOSUB" => Token::Gosub,
            "RETURN" => Token::Return,
            "ON" => Token::On,
            "SUB" => Token::Sub,
            "ENDSUB" => Token::EndSub,
            "FUNCTION" => Token::Function,
            "ENDFUNCTION" => Token::EndFunction,
            "SELECT" => Token::Select,
            "CASE" => Token::Case,
            "ENDSELECT" => Token::EndSelect,
            "END" => Token::End,
            "STOP" => Token::Stop,
            "REM" => Token::Rem,
            "DATA" => Token::Data,
            "READ" => Token::Read,
            "RESTORE" => Token::Restore,
            "CLS" => Token::Cls,
            "OPEN" => Token::Open,
            "CLOSE" => Token::Close,
            "AS" => Token::As,
            "OUTPUT" => Token::Output,
            "APPEND" => Token::Append,
            "AND" => Token::And,
            "OR" => Token::Or,
            "NOT" => Token::Not,
            "XOR" => Token::Xor,
            "MOD" => Token::Mod,
            _ => Token::Ident(s.to_string()),
        }
    }

    pub fn next_token(&mut self) -> Result<Token, String> {
        self.skip_whitespace();

        // Check for line number at start of line
        if self.at_line_start {
            if let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    let mut num = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_ascii_digit() {
                            num.push(self.advance().unwrap());
                        } else {
                            break;
                        }
                    }
                    self.at_line_start = false;
                    self.skip_whitespace();
                    return Ok(Token::LineNumber(num.parse().unwrap_or(0)));
                }
            }
        }
        self.at_line_start = false;

        let c = match self.advance() {
            Some(c) => c,
            None => return Ok(Token::Eof),
        };

        match c {
            '\n' => {
                self.line += 1;
                self.at_line_start = true;
                Ok(Token::Newline)
            }

            '"' => {
                self.pos -= 1; // back up to re-read the quote
                self.chars = self.input[self.pos..].chars().peekable();
                let s = self.read_string()?;
                Ok(Token::String(s))
            }

            '\'' => {
                self.skip_comment();
                Ok(Token::Newline) // Treat comment as end of statement
            }

            '+' => Ok(Token::Plus),
            '-' => Ok(Token::Minus),
            '*' => Ok(Token::Star),
            '/' => Ok(Token::Slash),
            '\\' => Ok(Token::Backslash),
            '^' => Ok(Token::Caret),
            '(' => Ok(Token::LParen),
            ')' => Ok(Token::RParen),
            ',' => Ok(Token::Comma),
            ';' => Ok(Token::Semicolon),
            ':' => Ok(Token::Colon),
            '#' => Ok(Token::Hash),

            '=' => Ok(Token::Eq),
            '<' => {
                if self.peek() == Some('>') {
                    self.advance();
                    Ok(Token::Ne)
                } else if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::Le)
                } else {
                    Ok(Token::Lt)
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::Ge)
                } else {
                    Ok(Token::Gt)
                }
            }

            '&' => {
                if self.peek() == Some('H') || self.peek() == Some('h') {
                    self.advance();
                    Ok(self.read_hex())
                } else {
                    // & alone could be long suffix but we handle that in identifiers
                    Ok(Token::Ident("&".to_string()))
                }
            }

            _ if c.is_ascii_digit() => Ok(self.read_number(c)),

            _ if c.is_ascii_alphabetic() => {
                let ident = self.read_identifier(c);

                // Handle REM as comment
                if ident == "REM" {
                    self.skip_comment();
                    return Ok(Token::Newline);
                }

                Ok(self.keyword_or_ident(&ident))
            }

            _ => Err(format!("Unexpected character: {}", c)),
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok == Token::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===================
    // Literal Tests
    // ===================

    #[test]
    fn test_integer_literal() {
        let mut lexer = Lexer::new("X = 42");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[1], Token::Eq);
        assert_eq!(tokens[2], Token::Integer(42));
    }

    #[test]
    fn test_float_literal_decimal() {
        let mut lexer = Lexer::new("X = 3.14159");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[2], Token::Float(3.14159));
    }

    #[test]
    fn test_float_literal_exponent() {
        let mut lexer = Lexer::new("X = 1E5");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[2], Token::Float(100000.0));

        let mut lexer = Lexer::new("X = 2e-3");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[2], Token::Float(0.002));
    }

    #[test]
    fn test_float_literal_d_exponent() {
        // BASIC uses D for double-precision exponent
        let mut lexer = Lexer::new("X = 1D5");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[2], Token::Float(100000.0));

        let mut lexer = Lexer::new("X = 2d+3");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[2], Token::Float(2000.0));
    }

    #[test]
    fn test_hex_literal() {
        let mut lexer = Lexer::new("X = &HFF");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[2], Token::Integer(255));

        let mut lexer = Lexer::new("X = &h10");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[2], Token::Integer(16));
    }

    #[test]
    fn test_string_literal() {
        let mut lexer = Lexer::new("PRINT \"Hello, World!\"");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Print);
        assert_eq!(tokens[1], Token::String("Hello, World!".to_string()));
    }

    #[test]
    fn test_string_escaped_quote() {
        let mut lexer = Lexer::new("X$ = \"He said \"\"Hi\"\"\"");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[2], Token::String("He said \"Hi\"".to_string()));
    }

    #[test]
    fn test_string_unterminated() {
        let mut lexer = Lexer::new("X$ = \"unterminated");
        let result = lexer.tokenize();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unterminated"));
    }

    // ===================
    // Identifier Tests
    // ===================

    #[test]
    fn test_identifier() {
        let mut lexer = Lexer::new("MyVar COUNTER foo123");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Ident("MYVAR".to_string()));
        assert_eq!(tokens[1], Token::Ident("COUNTER".to_string()));
        assert_eq!(tokens[2], Token::Ident("FOO123".to_string()));
    }

    #[test]
    fn test_type_suffix_all() {
        let mut lexer = Lexer::new("A% B& C! D# E$");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Ident("A%".to_string())); // integer
        assert_eq!(tokens[1], Token::Ident("B&".to_string())); // long
        assert_eq!(tokens[2], Token::Ident("C!".to_string())); // single
        assert_eq!(tokens[3], Token::Ident("D#".to_string())); // double
        assert_eq!(tokens[4], Token::Ident("E$".to_string())); // string
    }

    // ===================
    // Keyword Tests
    // ===================

    #[test]
    fn test_keywords_print_input() {
        let mut lexer = Lexer::new("PRINT INPUT LINE");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Print);
        assert_eq!(tokens[1], Token::Input);
        assert_eq!(tokens[2], Token::Line);
    }

    #[test]
    fn test_keywords_assignment() {
        let mut lexer = Lexer::new("LET DIM");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Let);
        assert_eq!(tokens[1], Token::Dim);
    }

    #[test]
    fn test_keywords_conditionals() {
        let mut lexer = Lexer::new("IF THEN ELSE ELSEIF ENDIF");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::If);
        assert_eq!(tokens[1], Token::Then);
        assert_eq!(tokens[2], Token::Else);
        assert_eq!(tokens[3], Token::ElseIf);
        assert_eq!(tokens[4], Token::EndIf);
    }

    #[test]
    fn test_keywords_for_loop() {
        let mut lexer = Lexer::new("FOR TO STEP NEXT");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::For);
        assert_eq!(tokens[1], Token::To);
        assert_eq!(tokens[2], Token::Step);
        assert_eq!(tokens[3], Token::Next);
    }

    #[test]
    fn test_keywords_while_loop() {
        let mut lexer = Lexer::new("WHILE WEND");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::While);
        assert_eq!(tokens[1], Token::Wend);
    }

    #[test]
    fn test_keywords_do_loop() {
        let mut lexer = Lexer::new("DO LOOP UNTIL");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Do);
        assert_eq!(tokens[1], Token::Loop);
        assert_eq!(tokens[2], Token::Until);
    }

    #[test]
    fn test_keywords_control_flow() {
        let mut lexer = Lexer::new("GOTO GOSUB RETURN ON");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Goto);
        assert_eq!(tokens[1], Token::Gosub);
        assert_eq!(tokens[2], Token::Return);
        assert_eq!(tokens[3], Token::On);
    }

    #[test]
    fn test_keywords_procedures() {
        let mut lexer = Lexer::new("SUB ENDSUB FUNCTION ENDFUNCTION");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Sub);
        assert_eq!(tokens[1], Token::EndSub);
        assert_eq!(tokens[2], Token::Function);
        assert_eq!(tokens[3], Token::EndFunction);
    }

    #[test]
    fn test_keywords_select_case() {
        let mut lexer = Lexer::new("SELECT CASE ENDSELECT");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Select);
        assert_eq!(tokens[1], Token::Case);
        assert_eq!(tokens[2], Token::EndSelect);
    }

    #[test]
    fn test_keywords_program_control() {
        let mut lexer = Lexer::new("END STOP CLS");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::End);
        assert_eq!(tokens[1], Token::Stop);
        assert_eq!(tokens[2], Token::Cls);
    }

    #[test]
    fn test_keywords_data() {
        let mut lexer = Lexer::new("DATA READ RESTORE");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Data);
        assert_eq!(tokens[1], Token::Read);
        assert_eq!(tokens[2], Token::Restore);
    }

    #[test]
    fn test_keywords_logical() {
        let mut lexer = Lexer::new("AND OR NOT XOR MOD");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::And);
        assert_eq!(tokens[1], Token::Or);
        assert_eq!(tokens[2], Token::Not);
        assert_eq!(tokens[3], Token::Xor);
        assert_eq!(tokens[4], Token::Mod);
    }

    #[test]
    fn test_keywords_case_insensitive() {
        let mut lexer = Lexer::new("print Print PRINT PrInT");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Print);
        assert_eq!(tokens[1], Token::Print);
        assert_eq!(tokens[2], Token::Print);
        assert_eq!(tokens[3], Token::Print);
    }

    // ===================
    // Operator Tests
    // ===================

    #[test]
    fn test_arithmetic_operators() {
        let mut lexer = Lexer::new("+ - * / \\ ^");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Plus);
        assert_eq!(tokens[1], Token::Minus);
        assert_eq!(tokens[2], Token::Star);
        assert_eq!(tokens[3], Token::Slash);
        assert_eq!(tokens[4], Token::Backslash);
        assert_eq!(tokens[5], Token::Caret);
    }

    #[test]
    fn test_comparison_operators() {
        let mut lexer = Lexer::new("= <> < > <= >=");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Eq);
        assert_eq!(tokens[1], Token::Ne);
        assert_eq!(tokens[2], Token::Lt);
        assert_eq!(tokens[3], Token::Gt);
        assert_eq!(tokens[4], Token::Le);
        assert_eq!(tokens[5], Token::Ge);
    }

    // ===================
    // Punctuation Tests
    // ===================

    #[test]
    fn test_punctuation() {
        let mut lexer = Lexer::new("( ) , ; :");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::LParen);
        assert_eq!(tokens[1], Token::RParen);
        assert_eq!(tokens[2], Token::Comma);
        assert_eq!(tokens[3], Token::Semicolon);
        assert_eq!(tokens[4], Token::Colon);
    }

    // ===================
    // Special Token Tests
    // ===================

    #[test]
    fn test_line_numbers() {
        let mut lexer = Lexer::new("10 PRINT\n20 END");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::LineNumber(10));
        assert_eq!(tokens[1], Token::Print);
        assert_eq!(tokens[2], Token::Newline);
        assert_eq!(tokens[3], Token::LineNumber(20));
        assert_eq!(tokens[4], Token::End);
    }

    #[test]
    fn test_newline() {
        let mut lexer = Lexer::new("A\nB\nC");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Ident("A".to_string()));
        assert_eq!(tokens[1], Token::Newline);
        assert_eq!(tokens[2], Token::Ident("B".to_string()));
        assert_eq!(tokens[3], Token::Newline);
        assert_eq!(tokens[4], Token::Ident("C".to_string()));
        assert_eq!(tokens[5], Token::Eof);
    }

    #[test]
    fn test_eof() {
        let mut lexer = Lexer::new("");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], Token::Eof);
    }

    // ===================
    // Comment Tests
    // ===================

    #[test]
    fn test_rem_comment() {
        let mut lexer = Lexer::new("X = 1 REM this is a comment\nY = 2");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Ident("X".to_string()));
        assert_eq!(tokens[1], Token::Eq);
        assert_eq!(tokens[2], Token::Integer(1));
        assert_eq!(tokens[3], Token::Newline); // REM becomes newline
        assert_eq!(tokens[4], Token::Newline); // actual \n
        assert_eq!(tokens[5], Token::Ident("Y".to_string()));
    }

    #[test]
    fn test_apostrophe_comment() {
        let mut lexer = Lexer::new("X = 1 ' this is a comment\nY = 2");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Ident("X".to_string()));
        assert_eq!(tokens[1], Token::Eq);
        assert_eq!(tokens[2], Token::Integer(1));
        assert_eq!(tokens[3], Token::Newline); // ' becomes newline
        assert_eq!(tokens[4], Token::Newline); // actual \n
        assert_eq!(tokens[5], Token::Ident("Y".to_string()));
    }

    // ===================
    // Integration Tests
    // ===================

    #[test]
    fn test_for_loop_statement() {
        let mut lexer = Lexer::new("FOR I = 1 TO 10 STEP 2");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::For);
        assert_eq!(tokens[1], Token::Ident("I".to_string()));
        assert_eq!(tokens[2], Token::Eq);
        assert_eq!(tokens[3], Token::Integer(1));
        assert_eq!(tokens[4], Token::To);
        assert_eq!(tokens[5], Token::Integer(10));
        assert_eq!(tokens[6], Token::Step);
        assert_eq!(tokens[7], Token::Integer(2));
    }

    #[test]
    fn test_function_call() {
        let mut lexer = Lexer::new("X = SIN(3.14) + COS(0)");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Ident("X".to_string()));
        assert_eq!(tokens[1], Token::Eq);
        assert_eq!(tokens[2], Token::Ident("SIN".to_string()));
        assert_eq!(tokens[3], Token::LParen);
        assert_eq!(tokens[4], Token::Float(3.14));
        assert_eq!(tokens[5], Token::RParen);
        assert_eq!(tokens[6], Token::Plus);
        assert_eq!(tokens[7], Token::Ident("COS".to_string()));
        assert_eq!(tokens[8], Token::LParen);
        assert_eq!(tokens[9], Token::Integer(0));
        assert_eq!(tokens[10], Token::RParen);
    }

    #[test]
    fn test_if_statement() {
        let mut lexer = Lexer::new("IF X > 10 AND Y < 5 THEN PRINT X");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::If);
        assert_eq!(tokens[1], Token::Ident("X".to_string()));
        assert_eq!(tokens[2], Token::Gt);
        assert_eq!(tokens[3], Token::Integer(10));
        assert_eq!(tokens[4], Token::And);
        assert_eq!(tokens[5], Token::Ident("Y".to_string()));
        assert_eq!(tokens[6], Token::Lt);
        assert_eq!(tokens[7], Token::Integer(5));
        assert_eq!(tokens[8], Token::Then);
        assert_eq!(tokens[9], Token::Print);
        assert_eq!(tokens[10], Token::Ident("X".to_string()));
    }

    #[test]
    fn test_print_with_separators() {
        let mut lexer = Lexer::new("PRINT A; B, C");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Print);
        assert_eq!(tokens[1], Token::Ident("A".to_string()));
        assert_eq!(tokens[2], Token::Semicolon);
        assert_eq!(tokens[3], Token::Ident("B".to_string()));
        assert_eq!(tokens[4], Token::Comma);
        assert_eq!(tokens[5], Token::Ident("C".to_string()));
    }

    #[test]
    fn test_dim_array() {
        let mut lexer = Lexer::new("DIM A(10), B$(100)");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Dim);
        assert_eq!(tokens[1], Token::Ident("A".to_string()));
        assert_eq!(tokens[2], Token::LParen);
        assert_eq!(tokens[3], Token::Integer(10));
        assert_eq!(tokens[4], Token::RParen);
        assert_eq!(tokens[5], Token::Comma);
        assert_eq!(tokens[6], Token::Ident("B$".to_string()));
        assert_eq!(tokens[7], Token::LParen);
        assert_eq!(tokens[8], Token::Integer(100));
        assert_eq!(tokens[9], Token::RParen);
    }

    #[test]
    fn test_colon_statement_separator() {
        let mut lexer = Lexer::new("X = 1 : Y = 2 : PRINT X");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Ident("X".to_string()));
        assert_eq!(tokens[1], Token::Eq);
        assert_eq!(tokens[2], Token::Integer(1));
        assert_eq!(tokens[3], Token::Colon);
        assert_eq!(tokens[4], Token::Ident("Y".to_string()));
        assert_eq!(tokens[5], Token::Eq);
        assert_eq!(tokens[6], Token::Integer(2));
        assert_eq!(tokens[7], Token::Colon);
        assert_eq!(tokens[8], Token::Print);
    }

    #[test]
    fn test_unexpected_character() {
        let mut lexer = Lexer::new("X = @");
        let result = lexer.tokenize();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unexpected character"));
    }
}
