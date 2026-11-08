//! Query DSL AST node types.
//!
//! Ported from Go beads `/internal/query/parser.go`.

/// Comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonOp {
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
}

/// A node in the query AST.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryNode {
    /// Field comparison: field op value
    Comparison {
        field: String,
        op: ComparisonOp,
        value: String,
    },
    /// Logical AND
    And(Box<Self>, Box<Self>),
    /// Logical OR
    Or(Box<Self>, Box<Self>),
    /// Logical NOT
    Not(Box<Self>),
}
