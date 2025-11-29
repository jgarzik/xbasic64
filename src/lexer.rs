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
    End,
    Stop,
    Rem,
    Data,
    Read,
    Restore,
    Cls,
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
            "END" => Token::End,
            "STOP" => Token::Stop,
            "REM" => Token::Rem,
            "DATA" => Token::Data,
            "READ" => Token::Read,
            "RESTORE" => Token::Restore,
            "CLS" => Token::Cls,
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

    #[test]
    fn test_hello_world() {
        let mut lexer = Lexer::new("PRINT \"Hello, World!\"");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Print);
        assert_eq!(tokens[1], Token::String("Hello, World!".to_string()));
    }

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
    fn test_operators() {
        // Use X = to avoid the number being parsed as line number
        let mut lexer = Lexer::new("X = 1 + 2 * 3 <> 4");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Ident("X".to_string()));
        assert_eq!(tokens[1], Token::Eq);
        assert_eq!(tokens[2], Token::Integer(1));
        assert_eq!(tokens[3], Token::Plus);
        assert_eq!(tokens[4], Token::Integer(2));
        assert_eq!(tokens[5], Token::Star);
        assert_eq!(tokens[6], Token::Integer(3));
        assert_eq!(tokens[7], Token::Ne);
        assert_eq!(tokens[8], Token::Integer(4));
    }

    #[test]
    fn test_type_suffix() {
        let mut lexer = Lexer::new("X% Y$ Z#");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0], Token::Ident("X%".to_string()));
        assert_eq!(tokens[1], Token::Ident("Y$".to_string()));
        assert_eq!(tokens[2], Token::Ident("Z#".to_string()));
    }
}
