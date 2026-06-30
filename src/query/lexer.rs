//! Lexer for the Query DSL.
//!
//! Tokenizes query strings like `status=open AND priority>1` into tokens.
//! Ported from Go beads `/internal/query/lexer.go`.

use std::iter::Peekable;
use std::str::Chars;

/// Token types in the query DSL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenType {
    Eof,
    Ident,     // field names, values, logical ops
    String,    // quoted strings
    Number,    // numeric values (0-9, .)
    Duration,  // duration values like 7d, 24h, 30m
    Equals,    // =
    NotEquals, // !=
    Less,      // <
    LessEq,    // <=
    Greater,   // >
    GreaterEq, // >=
    And,       // AND
    Or,        // OR
    Not,       // NOT
    LParen,    // (
    RParen,    // )
    Comma,     // ,
}

/// A token with its type, value, and position.
#[derive(Debug, Clone)]
pub(crate) struct Token {
    pub(crate) type_: TokenType,
    pub(crate) value: String,
    pub(crate) pos: usize,
}

/// Lexer tokenizes query strings.
pub(crate) struct Lexer<'a> {
    input: &'a str,
    chars: Peekable<Chars<'a>>,
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(input: &'a str) -> Self {
        Self {
            input,
            chars: input.chars().peekable(),
            pos: 0,
        }
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.chars.next();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn peek(&mut self) -> Option<&char> {
        self.chars.peek()
    }

    fn skip_whitespace(&mut self) {
        while let Some(&ch) = self.peek() {
            if ch.is_ascii_whitespace() {
                self.next();
            } else {
                break;
            }
        }
    }

    fn read_string(&mut self, quote: char, start: usize) -> Result<Token, String> {
        let mut value = String::new();
        loop {
            match self.next() {
                None => return Err(format!("unterminated string starting at position {start}")),
                Some(ch) if ch == quote => {
                    return Ok(Token {
                        type_: TokenType::String,
                        value,
                        pos: start,
                    });
                }
                Some(ch) => value.push(ch),
            }
        }
    }

    /// Read the next token from the input.
    pub(crate) fn next_token(&mut self) -> Result<Token, String> {
        self.skip_whitespace();
        let start = self.pos;
        let ch = match self.next() {
            None => {
                return Ok(Token {
                    type_: TokenType::Eof,
                    value: String::new(),
                    pos: start,
                });
            }
            Some(c) => c,
        };

        match ch {
            '(' => Ok(Token { type_: TokenType::LParen, value: "(".into(), pos: start }),
            ')' => Ok(Token { type_: TokenType::RParen, value: ")".into(), pos: start }),
            ',' => Ok(Token { type_: TokenType::Comma, value: ",".into(), pos: start }),
            '=' => Ok(Token { type_: TokenType::Equals, value: "=".into(), pos: start }),
            '!' => {
                if self.peek() == Some(&'=') {
                    self.next();
                    Ok(Token { type_: TokenType::NotEquals, value: "!=".into(), pos: start })
                } else {
                    Err(format!("unexpected '!' at position {start} (did you mean '!=' or 'NOT'?)"))
                }
            }
            '<' => {
                if self.peek() == Some(&'=') {
                    self.next();
                    Ok(Token { type_: TokenType::LessEq, value: "<=".into(), pos: start })
                } else {
                    Ok(Token { type_: TokenType::Less, value: "<".into(), pos: start })
                }
            }
            '>' => {
                if self.peek() == Some(&'=') {
                    self.next();
                    Ok(Token { type_: TokenType::GreaterEq, value: ">=".into(), pos: start })
                } else {
                    Ok(Token { type_: TokenType::Greater, value: ">".into(), pos: start })
                }
            }
            '\"' | '\'' => self.read_string(ch, start),
            '*' => {
                // Wildcard: re-interpret as an identifier character
                // Read remaining ident chars
                while let Some(&ch) = self.peek() {
                    if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' || ch == '*' {
                        self.next();
                    } else {
                        break;
                    }
                }
                let value: String = self.input[start..self.pos].to_string();
                Ok(Token { type_: TokenType::Ident, value, pos: start })
            }
            _ if ch.is_ascii_digit() || ch == '+' => {
                self.read_number_or_duration(start)
            }
            _ if is_ident_start(ch) => self.read_ident_or_keyword(start),
            _ => Err(format!("unexpected character {ch:?} at position {start}")),
        }
    }

    fn read_number_or_duration(&mut self, start: usize) -> Result<Token, String> {
        let end = self.read_digits(start);
        let value = self.input[start..end].to_string();

        // Check if next char is a duration suffix
        match self.peek() {
            Some(&'d') | Some(&'h') | Some(&'m') | Some(&'s') => {
                let ch = self.next().unwrap();
                Ok(Token { type_: TokenType::Duration, value: format!("{value}{ch}"), pos: start })
            }
            _ => Ok(Token { type_: TokenType::Number, value, pos: start }),
        }
    }

    fn read_digits(&mut self, start: usize) -> usize {
        while let Some(&ch) = self.peek() {
            if ch.is_ascii_digit() || ch == '.' {
                self.next();
            } else {
                break;
            }
        }
        self.pos
    }

    fn read_ident_or_keyword(&mut self, start: usize) -> Result<Token, String> {
        while let Some(&ch) = self.peek() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' || ch == '*' {
                self.next();
            } else {
                break;
            }
        }
        let value: String = self.input[start..self.pos].to_string();
        let type_ = match value.to_ascii_uppercase().as_str() {
            "AND" => TokenType::And,
            "OR" => TokenType::Or,
            "NOT" => TokenType::Not,
            _ => TokenType::Ident,
        };
        Ok(Token { type_, value, pos: start })
    }
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}
