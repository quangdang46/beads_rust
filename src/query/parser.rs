//! Recursive-descent parser for the Query DSL.
//!
//! Precedence (lowest to highest): OR < AND < NOT < primary
//! Ported from Go beads `/internal/query/parser.go`.

use crate::query::ast::{ComparisonOp, QueryNode};
use crate::query::lexer::{Lexer, Token, TokenType};

/// Parse errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub pos: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "query parse error at position {}: {}", self.pos, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Parser for query expressions.
struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Token,
    peeked: Option<Token>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            lexer: Lexer::new(input),
            current: Token {
                type_: TokenType::Eof,
                value: String::new(),
                pos: 0,
            },
            peeked: None,
        }
    }

    fn advance(&mut self) -> Result<(), ParseError> {
        if let Some(tok) = self.peeked.take() {
            self.current = tok;
            Ok(())
        } else {
            let tok = self
                .lexer
                .next_token()
                .map_err(|msg| ParseError { message: msg, pos: self.current.pos })?;
            self.current = tok;
            Ok(())
        }
    }

    fn peek(&mut self) -> Result<&Token, ParseError> {
        if self.peeked.is_none() {
            let tok = self
                .lexer
                .next_token()
                .map_err(|msg| ParseError { message: msg, pos: self.current.pos })?;
            self.peeked = Some(tok);
        }
        Ok(self.peeked.as_ref().unwrap())
    }

    fn parse(&mut self) -> Result<QueryNode, ParseError> {
        self.advance()?;
        if matches!(self.current.type_, TokenType::Eof) {
            return Err(ParseError {
                message: "empty query".into(),
                pos: 0,
            });
        }
        let node = self.parse_or()?;
        if !matches!(self.current.type_, TokenType::Eof) {
            return Err(ParseError {
                message: format!(
                    "unexpected token {:?} at position {} (expected end of query)",
                    self.current.value, self.current.pos
                ),
                pos: self.current.pos,
            });
        }
        Ok(node)
    }

    /// parse_or handles OR expressions (lowest precedence).
    fn parse_or(&mut self) -> Result<QueryNode, ParseError> {
        let mut left = self.parse_and()?;
        while matches!(self.current.type_, TokenType::Or) {
            self.advance()?;
            let right = self.parse_and()?;
            left = QueryNode::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// parse_and handles AND expressions.
    fn parse_and(&mut self) -> Result<QueryNode, ParseError> {
        let mut left = self.parse_not()?;
        while matches!(self.current.type_, TokenType::And) {
            self.advance()?;
            let right = self.parse_not()?;
            left = QueryNode::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// parse_not handles NOT expressions (right-associative).
    fn parse_not(&mut self) -> Result<QueryNode, ParseError> {
        if matches!(self.current.type_, TokenType::Not) {
            self.advance()?;
            let operand = self.parse_not()?;
            return Ok(QueryNode::Not(Box::new(operand)));
        }
        self.parse_primary()
    }

    /// parse_primary handles comparisons and parenthesized expressions.
    fn parse_primary(&mut self) -> Result<QueryNode, ParseError> {
        if matches!(self.current.type_, TokenType::LParen) {
            self.advance()?;
            let node = self.parse_or()?;
            if !matches!(self.current.type_, TokenType::RParen) {
                return Err(ParseError {
                    message: format!(
                        "expected ')' at position {}, got {:?}",
                        self.current.pos, self.current.value
                    ),
                    pos: self.current.pos,
                });
            }
            self.advance()?;
            return Ok(node);
        }
        self.parse_comparison()
    }

    /// parse_comparison parses: field op value
    fn parse_comparison(&mut self) -> Result<QueryNode, ParseError> {
        if !matches!(self.current.type_, TokenType::Ident) {
            return Err(ParseError {
                message: format!(
                    "expected field name at position {}, got {:?}",
                    self.current.pos, self.current.value
                ),
                pos: self.current.pos,
            });
        }

        let field = self.current.value.to_lowercase();
        self.advance()?;

        let op = match self.current.type_ {
            TokenType::Equals => ComparisonOp::Eq,
            TokenType::NotEquals => ComparisonOp::NotEq,
            TokenType::Less => ComparisonOp::Less,
            TokenType::LessEq => ComparisonOp::LessEq,
            TokenType::Greater => ComparisonOp::Greater,
            TokenType::GreaterEq => ComparisonOp::GreaterEq,
            _ => {
                return Err(ParseError {
                    message: format!(
                        "expected comparison operator at position {}, got {:?}",
                        self.current.pos, self.current.value
                    ),
                    pos: self.current.pos,
                })
            }
        };
        self.advance()?;

        let (value, _value_type) = match self.current.type_ {
            TokenType::Ident | TokenType::String | TokenType::Number | TokenType::Duration => {
                (self.current.value.clone(), self.current.type_)
            }
            _ => {
                return Err(ParseError {
                    message: format!(
                        "expected value at position {}, got {:?}",
                        self.current.pos, self.current.value
                    ),
                    pos: self.current.pos,
                })
            }
        };
        self.advance()?;

        Ok(QueryNode::Comparison { field, op, value })
    }
}

/// Parse a query string into an AST node.
pub fn parse(input: &str) -> Result<QueryNode, ParseError> {
    let mut parser = Parser::new(input);
    parser.parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_comparison() {
        let ast = parse("status=open").unwrap();
        assert_eq!(
            ast,
            QueryNode::Comparison {
                field: "status".into(),
                op: ComparisonOp::Eq,
                value: "open".into()
            }
        );
    }

    #[test]
    fn test_parse_and_chain() {
        let ast = parse("status=open AND priority>1").unwrap();
        assert_eq!(
            ast,
            QueryNode::And(
                Box::new(QueryNode::Comparison {
                    field: "status".into(),
                    op: ComparisonOp::Eq,
                    value: "open".into()
                }),
                Box::new(QueryNode::Comparison {
                    field: "priority".into(),
                    op: ComparisonOp::Greater,
                    value: "1".into()
                })
            )
        );
    }

    #[test]
    fn test_parse_or_chain() {
        let ast = parse("status=open OR status=blocked").unwrap();
        assert!(matches!(ast, QueryNode::Or(..)));
    }

    #[test]
    fn test_parse_parentheses() {
        let ast = parse("(status=open OR status=blocked) AND priority>1").unwrap();
        // Should parse as AND(Or(Eq, Eq), >)
        assert!(matches!(ast, QueryNode::And(..)));
    }

    #[test]
    fn test_parse_not() {
        let ast = parse("NOT status=closed").unwrap();
        assert!(matches!(ast, QueryNode::Not(..)));
    }

    #[test]
    fn test_parse_not_precedence() {
        // NOT binds tighter than AND: NOT status=closed AND priority>1
        let ast = parse("NOT status=closed AND priority>1").unwrap();
        assert!(matches!(ast, QueryNode::And(..)));
    }

    #[test]
    fn test_parse_empty_error() {
        let err = parse("").unwrap_err();
        assert!(err.message.contains("empty"));
    }

    #[test]
    fn test_parse_syntax_error() {
        let err = parse("status ").unwrap_err();
        assert!(err.message.contains("comparison operator") || err.message.contains("unexpected"));
    }

    #[test]
    fn test_parse_duration() {
        let ast = parse("updated>7d").unwrap();
        assert_eq!(
            ast,
            QueryNode::Comparison {
                field: "updated".into(),
                op: ComparisonOp::Greater,
                value: "7d".into()
            }
        );
    }

    #[test]
    fn test_parse_not_equals() {
        let ast = parse("status!=closed").unwrap();
        assert_eq!(
            ast,
            QueryNode::Comparison {
                field: "status".into(),
                op: ComparisonOp::NotEq,
                value: "closed".into()
            }
        );
    }
}
