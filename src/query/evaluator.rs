//! Evaluator for the Query DSL.
//!
//! Converts a query AST into `ListFilters` (SQL WHERE conditions).
//! Ported from Go beads `/internal/query/evaluator.go`.
//!
//! Simple AND chains of comparisons translate directly to ListFilters fields.
//! Complex queries (OR, NOT) produce a predicate function for in-memory filtering.

use crate::query::ast::{ComparisonOp, QueryNode};
use crate::storage::sqlite::ListFilters;
use chrono::{Duration, Utc};
use std::str::FromStr;

/// Error during query evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// Query contains OR/NOT that can't be expressed as ListFilters alone.
    ComplexQuery(String),
    /// Invalid field name.
    UnknownField(String),
    /// Invalid value for a field.
    InvalidValue(String),
    /// Invalid comparison operator for a field.
    InvalidOperator(String),
    /// Parse error (wraps parser error).
    ParseError(String),
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryError::ComplexQuery(msg) => write!(f, "complex query: {msg}"),
            QueryError::UnknownField(msg) => write!(f, "unknown field: {msg}"),
            QueryError::InvalidValue(msg) => write!(f, "invalid value: {msg}"),
            QueryError::InvalidOperator(msg) => write!(f, "invalid operator: {msg}"),
            QueryError::ParseError(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for QueryError {}

impl From<crate::query::parser::ParseError> for QueryError {
    fn from(e: crate::query::parser::ParseError) -> Self {
        QueryError::ParseError(e.to_string())
    }
}

/// Result of evaluating a query.
pub struct QueryResult {
    /// Filters suitable for passing to the storage layer.
    /// May be incomplete if the query contains OR/NOT.
    pub filters: ListFilters,
    /// Whether in-memory predicate filtering is needed.
    pub requires_predicate: bool,
    /// Predicate function for in-memory filtering (None when not needed).
    pub predicate: Option<Box<dyn Fn(&crate::model::Issue) -> bool + Send>>,
}

impl std::fmt::Debug for QueryResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryResult")
            .field("filters", &self.filters)
            .field("requires_predicate", &self.requires_predicate)
            .field("predicate", &self.predicate.is_some())
            .finish()
    }
}

/// Parse a duration shorthand (e.g. "7d", "24h", "30m") into a Duration.
fn parse_duration_shorthand(s: &str) -> Result<Duration, QueryError> {
    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: i64 = num_str
        .parse()
        .map_err(|_| QueryError::InvalidValue(format!("invalid duration number: {s}")))?;
    match unit {
        "d" => Ok(Duration::days(num)),
        "h" => Ok(Duration::hours(num)),
        "m" => Ok(Duration::minutes(num)),
        "s" => Ok(Duration::seconds(num)),
        _ => Err(QueryError::InvalidValue(format!("unknown duration unit: {s}"))),
    }
}

/// Parse a priority value from a string (e.g. "P2" or "2").
/// Check if a query can be expressed as ListFilters only (no predicate needed).
fn can_use_filter_only(node: &QueryNode) -> bool {
    match node {
        QueryNode::Comparison { .. } => true,
        QueryNode::And(left, right) => can_use_filter_only(left) && can_use_filter_only(right),
        QueryNode::Not(inner) => {
            matches!(inner.as_ref(), QueryNode::Comparison { field, op, .. }
                if (field == "status" || field == "type") && *op == ComparisonOp::Eq)
        }
        QueryNode::Or(_, _) => false,
    }
}

/// Build ListFilters from a filter-compatible AST node.
fn build_filters(node: &QueryNode, filters: &mut ListFilters) -> Result<(), QueryError> {
    match node {
        QueryNode::Comparison { field, op, value } => {
            apply_comparison(field, *op, value, filters)
        }
        QueryNode::And(left, right) => {
            build_filters(left, filters)?;
            build_filters(right, filters)
        }
        QueryNode::Not(inner) => {
            if let QueryNode::Comparison { field, op: ComparisonOp::Eq, value } = inner.as_ref() {
                match field.as_str() {
                    "status" => {
                        let excl = crate::model::Status::from_str(value).map_err(|_| {
                            QueryError::InvalidValue(format!("invalid status: {value}"))
                        })?;
                        let all = vec![
                            crate::model::Status::Open,
                            crate::model::Status::InProgress,
                            crate::model::Status::Blocked,
                            crate::model::Status::Deferred,
                            crate::model::Status::Draft,
                            crate::model::Status::Closed,
                            crate::model::Status::Tombstone,
                            crate::model::Status::Pinned,
                        ];
                        filters.statuses = Some(all.into_iter().filter(|s| s != &excl).collect());
                    }
                    "type" => {
                        return Err(QueryError::InvalidOperator(
                            "NOT type not supported in filter-only mode".into(),
                        ));
                    }
                    _ => {
                        return Err(QueryError::InvalidOperator(
                            format!("NOT not supported for field {field}"),
                        ));
                    }
                }
                Ok(())
            } else {
                Err(QueryError::ComplexQuery("complex NOT expression".into()))
            }
        }
        QueryNode::Or(..) => Err(QueryError::ComplexQuery(
            "OR not supported in filter-only mode".into(),
        )),
    }
}

/// Apply a single comparison to ListFilters.
#[allow(clippy::too_many_lines)]
fn apply_comparison(
    field: &str,
    op: ComparisonOp,
    value: &str,
    filters: &mut ListFilters,
) -> Result<(), QueryError> {
    match field {
        "status" => {
            if op != ComparisonOp::Eq && op != ComparisonOp::NotEq {
                return Err(QueryError::InvalidOperator("status only supports = and !=".into()));
            }
            let status = crate::model::Status::from_str(value)
                .map_err(|_| QueryError::InvalidValue(format!("invalid status: {value}")))?;
            if op == ComparisonOp::Eq {
                filters.statuses.get_or_insert(Vec::new()).push(status);
            }
            Ok(())
        }
        "priority" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::ComplexQuery(
                    "priority !=, >, <, >=, <= requires predicate filtering (only = supported)".into(),
                ));
            }
            let priority = crate::model::Priority::from_str(value).map_err(|_| {
                QueryError::InvalidValue(format!("invalid priority: {value} (expected P0-P4 or 0-4)"))
            })?;
            filters.priorities.get_or_insert(Vec::new()).push(priority);
            Ok(())
        }
        "type" => {
            if op != ComparisonOp::Eq && op != ComparisonOp::NotEq {
                return Err(QueryError::InvalidOperator("type only supports = and !=".into()));
            }
            let issue_type = crate::model::IssueType::from_str(value)
                .map_err(|_| QueryError::InvalidValue(format!("invalid type: {value}")))?;
            if op == ComparisonOp::Eq {
                filters.types.get_or_insert(Vec::new()).push(issue_type);
            }
            Ok(())
        }
        "assignee" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator("assignee only supports =".into()));
            }
            if value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("null") || value.is_empty() {
                filters.unassigned = true;
            } else {
                filters.assignee = Some(value.to_string());
            }
            Ok(())
        }
        "label" | "labels" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator("label only supports =".into()));
            }
            if value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("null") || value.is_empty() {
                return Err(QueryError::ComplexQuery("label=none requires predicate".into()));
            }
            filters.labels.get_or_insert(Vec::new()).push(value.to_string());
            Ok(())
        }
        "title" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator("title only supports = (substring match)".into()));
            }
            filters.title_contains = Some(value.to_string());
            Ok(())
        }
        "created" | "created_at" => {
            if !value.ends_with('d') && !value.ends_with('h') && !value.ends_with('m') && !value.ends_with('s') {
                return Err(QueryError::InvalidValue(
                    "created filter requires duration shorthand (e.g. 7d)".into(),
                ));
            }
            let dur = parse_duration_shorthand(value)?;
            let ts = Utc::now() - dur;
            match op {
                ComparisonOp::Eq => {
                    filters.created_after = Some(ts);
                    filters.created_before = Some(ts + Duration::days(1));
                }
                ComparisonOp::Greater | ComparisonOp::GreaterEq => {
                    filters.created_after = Some(ts);
                }
                ComparisonOp::Less | ComparisonOp::LessEq => {
                    filters.created_before = Some(ts);
                }
                _ => {
                    return Err(QueryError::InvalidOperator(
                        "invalid operator for timestamp field".into(),
                    ));
                }
            }
            Ok(())
        }
        "updated" | "updated_at" => {
            if !value.ends_with('d') && !value.ends_with('h') && !value.ends_with('m') && !value.ends_with('s') {
                return Err(QueryError::InvalidValue(
                    "updated filter requires duration shorthand (e.g. 7d)".into(),
                ));
            }
            let dur = parse_duration_shorthand(value)?;
            let ts = Utc::now() - dur;
            match op {
                ComparisonOp::Eq => {
                    filters.updated_after = Some(ts);
                    filters.updated_before = Some(ts + Duration::days(1));
                }
                ComparisonOp::Greater | ComparisonOp::GreaterEq => {
                    filters.updated_after = Some(ts);
                }
                ComparisonOp::Less | ComparisonOp::LessEq => {
                    filters.updated_before = Some(ts);
                }
                _ => {
                    return Err(QueryError::InvalidOperator(
                        "invalid operator for timestamp field".into(),
                    ));
                }
            }
            Ok(())
        }
        "id" => {
            if value.contains('*') {
                filters.ids.get_or_insert(Vec::new()).push(value.replace('*', "%"));
            } else {
                filters.ids.get_or_insert(Vec::new()).push(value.to_string());
            }
            Ok(())
        }
        "pinned" => {
            let b = value.eq_ignore_ascii_case("true") || value == "1";
            filters.pinned = Some(b);
            Ok(())
        }
        "mol_type" | "mol-type" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator("mol_type only supports =".into()));
            }
            let mt = crate::model::MolType::from_str(value)
                .map_err(|_| QueryError::InvalidValue(format!("invalid mol_type: {value}")))?;
            filters.mol_type = Some(mt);
            Ok(())
        }
        "owner" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator("owner only supports =".into()));
            }
            filters.owner = Some(value.to_string());
            Ok(())
        }
        f if f.starts_with("metadata.") => {
            let key = &f["metadata.".len()..];
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator("metadata only supports =".into()));
            }
            filters
                .metadata_filters
                .get_or_insert(Vec::new())
                .push(format!("{key}={value}"));
            Ok(())
        }
        other => Err(QueryError::UnknownField(other.to_string())),
    }
}

/// Build an in-memory predicate function from a query AST.
///
/// This is used for complex queries (OR, NOT) that can't be expressed
/// as SQL WHERE conditions alone.
fn build_predicate(node: &QueryNode) -> Box<dyn Fn(&crate::model::Issue) -> bool + Send> {
    match node {
        QueryNode::Comparison { field, op, value } => {
            let field = field.clone();
            let value = value.clone();
            let op = *op;
            Box::new(move |issue| evaluate_predicate_on_issue(issue, &field, op, &value))
        }
        QueryNode::And(left, right) => {
            let left_fn = build_predicate(left);
            let right_fn = build_predicate(right);
            Box::new(move |issue| left_fn(issue) && right_fn(issue))
        }
        QueryNode::Or(left, right) => {
            let left_fn = build_predicate(left);
            let right_fn = build_predicate(right);
            Box::new(move |issue| left_fn(issue) || right_fn(issue))
        }
        QueryNode::Not(inner) => {
            let inner_fn = build_predicate(inner);
            Box::new(move |issue| !inner_fn(issue))
        }
    }
}

/// Evaluate a single comparison against an Issue's in-memory fields.
fn evaluate_predicate_on_issue(
    issue: &crate::model::Issue,
    field: &str,
    op: ComparisonOp,
    value: &str,
) -> bool {
    use crate::model::Priority;

    let apply_str_op = |a: &str, b: &str| -> bool {
        match op {
            ComparisonOp::Eq => a.eq_ignore_ascii_case(b),
            ComparisonOp::NotEq => !a.eq_ignore_ascii_case(b),
            _ => false,
        }
    };

    match field {
        "status" => apply_str_op(&issue.status.to_string(), value),
        "type" => apply_str_op(&issue.issue_type.to_string(), value),
        "priority" => {
            let issue_p = issue.priority.0;
            let v: i32 = Priority::from_str(value).map(|p| p.0).unwrap_or(-1);
            match op {
                ComparisonOp::Eq => issue_p == v,
                ComparisonOp::NotEq => issue_p != v,
                ComparisonOp::Less => issue_p < v,
                ComparisonOp::LessEq => issue_p <= v,
                ComparisonOp::Greater => issue_p > v,
                ComparisonOp::GreaterEq => issue_p >= v,
            }
        }
        "assignee" => {
            let a = issue.assignee.as_deref().unwrap_or("");
            apply_str_op(a, value)
        }
        "owner" => {
            let a = issue.owner.as_deref().unwrap_or("");
            apply_str_op(a, value)
        }
        "unassigned" => {
            let expected = value == "true" || value == "1";
            let is_unassigned = issue.assignee.is_none();
            if op == ComparisonOp::NotEq {
                is_unassigned != expected
            } else {
                is_unassigned == expected
            }
        }
        "pinned" => {
            let expected = value == "true" || value == "1";
            if op == ComparisonOp::NotEq {
                issue.pinned != expected
            } else {
                issue.pinned == expected
            }
        }
        "title" => {
            let issue_title = issue.title.to_lowercase();
            let search = value.to_lowercase();
            match op {
                ComparisonOp::Eq => issue_title == search,
                ComparisonOp::NotEq => issue_title != search,
                _ => false,
            }
        }
        "id" => {
            let id = &issue.id;
            if value.ends_with('*') {
                let prefix = &value[..value.len() - 1];
                id.starts_with(prefix)
            } else {
                apply_str_op(id, value)
            }
        }
        "labels" => {
            match op {
                ComparisonOp::Eq => issue.labels.iter().any(|l| l.eq_ignore_ascii_case(value)),
                ComparisonOp::NotEq => !issue.labels.iter().any(|l| l.eq_ignore_ascii_case(value)),
                _ => false,
            }
        }
        _ if field.starts_with("metadata.") => {
            let key = &field["metadata.".len()..];
            let issue_val: String = issue
                .metadata
                .as_deref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .and_then(|v| v.get(key).and_then(|v| v.as_str().map(String::from)))
                .unwrap_or_default();
            apply_str_op(&issue_val, value)
        }
        _ => {
            // Unknown field — treat as no match
            false
        }
    }
}

/// Evaluate a query AST and produce a QueryResult.
pub fn evaluate(node: &QueryNode) -> Result<QueryResult, QueryError> {
    let mut filters = ListFilters::default();

    if can_use_filter_only(node) {
        build_filters(node, &mut filters)?;
        return Ok(QueryResult {
            filters,
            requires_predicate: false,
            predicate: None,
        });
    }

    // Complex query: build base filters (best-effort), build predicate
    let _ = build_filters(node, &mut filters);
    let predicate = build_predicate(node);
    Ok(QueryResult {
        filters,
        requires_predicate: true,
        predicate: Some(predicate),
    })
}

/// Shorthand: parse and evaluate a query string.
pub fn parse_and_evaluate(input: &str) -> Result<QueryResult, QueryError> {
    let ast = crate::query::parser::parse(input)?;
    evaluate(&ast)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_status() {
        let result = parse_and_evaluate("status=open").unwrap();
        assert!(!result.requires_predicate);
        assert_eq!(result.filters.statuses, Some(vec![crate::model::Status::Open]));
    }

    #[test]
    fn test_and_chain() {
        let result = parse_and_evaluate("status=open AND type=bug").unwrap();
        assert!(!result.requires_predicate);
        assert_eq!(result.filters.statuses, Some(vec![crate::model::Status::Open]));
        assert_eq!(result.filters.types, Some(vec![crate::model::IssueType::Bug]));
    }

    #[test]
    fn test_duration_shorthand() {
        let result = parse_and_evaluate("updated>7d").unwrap();
        assert!(!result.requires_predicate);
        assert!(result.filters.updated_after.is_some());
    }

    #[test]
    fn test_unknown_field() {
        let err = parse_and_evaluate("nonexistent=foo").unwrap_err();
        assert!(matches!(err, QueryError::UnknownField(_)));
    }

    #[test]
    fn test_wildcard_id() {
        let result = parse_and_evaluate("id=br-*").unwrap();
        assert_eq!(result.filters.ids, Some(vec!["br-%".to_string()]));
    }

    #[test]
    fn test_metadata_filter() {
        let result = parse_and_evaluate("metadata.component=backend").unwrap();
        assert_eq!(
            result.filters.metadata_filters,
            Some(vec!["component=backend".to_string()])
        );
    }

    #[test]
    fn test_pinned_bool() {
        let result = parse_and_evaluate("pinned=true").unwrap();
        assert_eq!(result.filters.pinned, Some(true));
    }

    #[test]
    fn test_mol_type_filter() {
        let result = parse_and_evaluate("mol_type=swarm").unwrap();
        assert_eq!(result.filters.mol_type, Some(crate::model::MolType::Swarm));
    }

    #[test]
    fn test_assignee_filter() {
        let result = parse_and_evaluate("assignee=alice").unwrap();
        assert_eq!(result.filters.assignee, Some("alice".to_string()));
    }

    #[test]
    fn test_assignee_none() {
        let result = parse_and_evaluate("assignee=none").unwrap();
        assert!(result.filters.unassigned);
    }

    #[test]
    fn test_title_filter() {
        let result = parse_and_evaluate("title=wip").unwrap();
        assert_eq!(result.filters.title_contains, Some("wip".to_string()));
    }

    #[test]
    fn test_priority_exact() {
        let result = parse_and_evaluate("priority=2").unwrap();
        assert_eq!(result.filters.priorities, Some(vec![crate::model::Priority(2)]));
    }

    #[test]
    fn test_priority_range_error() {
        let err = parse_and_evaluate("priority>2").unwrap_err();
        assert!(matches!(err, QueryError::ComplexQuery(_)));
    }

    #[test]
    fn test_multiple_and() {
        let result = parse_and_evaluate("status=open AND priority=1 AND label=urgent").unwrap();
        assert_eq!(result.filters.statuses, Some(vec![crate::model::Status::Open]));
        assert_eq!(result.filters.priorities, Some(vec![crate::model::Priority(1)]));
        assert_eq!(result.filters.labels, Some(vec!["urgent".to_string()]));
    }

    #[test]
    fn test_duration_created() {
        let result = parse_and_evaluate("created<30d").unwrap();
        assert!(!result.requires_predicate);
        assert!(result.filters.created_before.is_some());
    }

    #[test]
    fn test_owner_filter() {
        let result = parse_and_evaluate("owner=bob").unwrap();
        assert_eq!(result.filters.owner, Some("bob".to_string()));
    }
}
