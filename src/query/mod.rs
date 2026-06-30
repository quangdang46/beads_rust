//! Query DSL — string-based boolean expression filter engine.
//!
//! Ported from Go beads `/internal/query/` (1738 lines).
//! Translates expressions like `status=open AND priority>1` into
//! the existing `ListFilters` struct for SQL-based filtering.
//!
//! ## Supported syntax
//!
//! ```text
//! comparison  ::= field op value
//! op          ::= = | != | < | <= | > | >=
//! value       ::= identifier | string | number | duration
//! expr        ::= comparison (AND|OR comparison)*
//! ```

mod lexer;
mod ast;
mod parser;
mod evaluator;

pub use ast::{ComparisonOp, QueryNode};
pub use evaluator::{QueryError, QueryResult, evaluate};
pub use parser::parse;

/// Parse a query string and evaluate it into a `QueryResult`.
/// Shorthand for `parse(input).and_then(|ast| evaluate(&ast))`.
pub fn parse_and_evaluate(input: &str) -> Result<QueryResult, QueryError> {
    let ast = parse(input)?;
    evaluate(&ast)
}

#[cfg(test)]
mod tests;
