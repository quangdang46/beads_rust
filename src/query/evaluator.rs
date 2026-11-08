//! Evaluator for the Query DSL.
//!
//! Converts a query AST into `ListFilters` (SQL WHERE conditions).
//! Ported from Go beads `/internal/query/evaluator.go`.
//!
//! Simple AND chains of comparisons translate directly to ListFilters fields.
//! Complex queries (OR, NOT) produce a predicate function for in-memory filtering.

use crate::query::ast::{ComparisonOp, QueryNode};
use crate::storage::sqlite::ListFilters;
use chrono::{DateTime, Duration, Utc};
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
            Self::ComplexQuery(msg) => write!(f, "complex query: {msg}"),
            Self::UnknownField(msg) => write!(f, "unknown field: {msg}"),
            Self::InvalidValue(msg) => write!(f, "invalid value: {msg}"),
            Self::InvalidOperator(msg) => write!(f, "invalid operator: {msg}"),
            Self::ParseError(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for QueryError {}

impl From<crate::query::parser::ParseError> for QueryError {
    fn from(e: crate::query::parser::ParseError) -> Self {
        Self::ParseError(e.to_string())
    }
}

/// In-memory predicate over an issue, used when a query cannot be fully
/// expressed as storage-layer filters.
pub type IssuePredicate = Box<dyn Fn(&crate::model::Issue) -> bool + Send>;

/// Result of evaluating a query.
pub struct QueryResult {
    /// Filters suitable for passing to the storage layer.
    /// May be incomplete if the query contains OR/NOT.
    pub filters: ListFilters,
    /// Whether in-memory predicate filtering is needed.
    pub requires_predicate: bool,
    /// Predicate function for in-memory filtering (None when not needed).
    pub predicate: Option<IssuePredicate>,
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
        _ => Err(QueryError::InvalidValue(format!(
            "unknown duration unit: {s}"
        ))),
    }
}

/// Parse a timestamp value from a query comparison string.
///
/// Supports:
/// - Duration shorthand: "7d", "30m" (relative to now, meaning that long ago)
/// - ISO 8601 dates: "2026-01-15" (parsed as UTC midnight in local timezone)
/// - Natural language: "yesterday", "today", "now", "tomorrow", "next-week",
///   "last-week", "last-month"
/// - Relative offsets: "+1d" = one day from now, "-3d" = three days ago
fn parse_timestamp_value(s: &str) -> Result<DateTime<Utc>, QueryError> {
    // First try duration shorthand (old behavior: "7d" = 7 days ago)
    if s.ends_with(['d', 'h', 'm', 's']) && s.starts_with(|c: char| c.is_ascii_digit()) {
        let dur = parse_duration_shorthand(s)?;
        return Ok(Utc::now() - dur);
    }

    // Fall through to flexible timestamp parsing (ISO dates, natural language, +/- offsets)
    match crate::util::time::parse_flexible_timestamp(s, "timestamp") {
        Ok(dt) => Ok(dt),
        Err(_) => Err(QueryError::InvalidValue(format!(
            "invalid date/time value: {s} (try: 7d, 30m, 2026-01-15, yesterday, now, tomorrow, next-week)"
        ))),
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
        QueryNode::Comparison { field, op, value } => apply_comparison(field, *op, value, filters),
        QueryNode::And(left, right) => {
            build_filters(left, filters)?;
            build_filters(right, filters)
        }
        QueryNode::Not(inner) => {
            if let QueryNode::Comparison {
                field,
                op: ComparisonOp::Eq,
                value,
            } = inner.as_ref()
            {
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
                        return Err(QueryError::InvalidOperator(format!(
                            "NOT not supported for field {field}"
                        )));
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
                return Err(QueryError::InvalidOperator(
                    "status only supports = and !=".into(),
                ));
            }
            let status = crate::model::Status::from_str(value)
                .map_err(|_| QueryError::InvalidValue(format!("invalid status: {value}")))?;
            if op == ComparisonOp::Eq {
                filters.statuses.get_or_insert(Vec::new()).push(status);
            } else {
                // `!=` cannot be expressed as an IN-list (multiple status
                // comparisons are OR-ed); fall back to predicate filtering.
                return Err(QueryError::ComplexQuery(
                    "status != requires predicate filtering".into(),
                ));
            }
            Ok(())
        }
        "priority" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::ComplexQuery(
                    "priority !=, >, <, >=, <= requires predicate filtering (only = supported)"
                        .into(),
                ));
            }
            let priority = crate::model::Priority::from_str(value).map_err(|_| {
                QueryError::InvalidValue(format!(
                    "invalid priority: {value} (expected P0-P4 or 0-4)"
                ))
            })?;
            filters.priorities.get_or_insert(Vec::new()).push(priority);
            Ok(())
        }
        "type" | "issue_type" | "issue-type" => {
            if op != ComparisonOp::Eq && op != ComparisonOp::NotEq {
                return Err(QueryError::InvalidOperator(
                    "type only supports = and !=".into(),
                ));
            }
            let issue_type = crate::model::IssueType::from_str(value)
                .map_err(|_| QueryError::InvalidValue(format!("invalid type: {value}")))?;
            if op == ComparisonOp::Eq {
                filters.types.get_or_insert(Vec::new()).push(issue_type);
            } else {
                // Same OR-semantics problem as status !=; use the predicate.
                return Err(QueryError::ComplexQuery(
                    "type != requires predicate filtering".into(),
                ));
            }
            Ok(())
        }
        "assignee" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator(
                    "assignee only supports =".into(),
                ));
            }
            if value.eq_ignore_ascii_case("none")
                || value.eq_ignore_ascii_case("null")
                || value.is_empty()
            {
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
            if value.eq_ignore_ascii_case("none")
                || value.eq_ignore_ascii_case("null")
                || value.is_empty()
            {
                return Err(QueryError::ComplexQuery(
                    "label=none requires predicate".into(),
                ));
            }
            filters
                .labels
                .get_or_insert(Vec::new())
                .push(value.to_string());
            Ok(())
        }
        "title" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator(
                    "title only supports = (substring match)".into(),
                ));
            }
            filters.title_contains = Some(value.to_string());
            Ok(())
        }
        "created" | "created_at" => {
            let ts = parse_timestamp_value(value)?;
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
                ComparisonOp::NotEq => {
                    return Err(QueryError::InvalidOperator(
                        "invalid operator for timestamp field".into(),
                    ));
                }
            }
            Ok(())
        }
        "updated" | "updated_at" => {
            let ts = parse_timestamp_value(value)?;
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
                ComparisonOp::NotEq => {
                    return Err(QueryError::InvalidOperator(
                        "invalid operator for timestamp field".into(),
                    ));
                }
            }
            Ok(())
        }
        "id" => {
            if op != ComparisonOp::Eq && op != ComparisonOp::NotEq {
                return Err(QueryError::InvalidOperator(
                    "id only supports = and !=".into(),
                ));
            }
            if op == ComparisonOp::NotEq {
                return Err(QueryError::ComplexQuery(
                    "id != requires predicate filtering".into(),
                ));
            }
            if value.contains('*') {
                filters
                    .ids
                    .get_or_insert(Vec::new())
                    .push(value.replace('*', "%"));
            } else {
                filters
                    .ids
                    .get_or_insert(Vec::new())
                    .push(value.to_string());
            }
            Ok(())
        }
        "pinned" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::ComplexQuery(
                    "pinned != requires predicate filtering".into(),
                ));
            }
            let b = value.eq_ignore_ascii_case("true") || value == "1";
            filters.pinned = Some(b);
            Ok(())
        }
        "mol_type" | "mol-type" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator(
                    "mol_type only supports =".into(),
                ));
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
        "description" | "desc" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator(
                    "description only supports = (substring match)".into(),
                ));
            }
            // No SQL-side filter; use predicate path
            Err(QueryError::ComplexQuery(
                "description filtering requires predicate (no SQL-side filter)".into(),
            ))
        }
        "notes" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator(
                    "notes only supports = (substring match)".into(),
                ));
            }
            // No SQL-side filter; use predicate path
            Err(QueryError::ComplexQuery(
                "notes filtering requires predicate (no SQL-side filter)".into(),
            ))
        }
        "closed" | "closed_at" => {
            // SQL filtering for closed_at not yet implemented; use predicate path
            Err(QueryError::ComplexQuery(
                "closed_at filtering requires predicate (no SQL-side filter)".into(),
            ))
        }
        "started" | "started_at" => {
            // SQL filtering for started_at not yet implemented; use predicate path
            Err(QueryError::ComplexQuery(
                "started_at filtering requires predicate (no SQL-side filter)".into(),
            ))
        }
        "spec" | "spec_id" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator("spec only supports =".into()));
            }
            Err(QueryError::ComplexQuery(
                "spec filtering requires predicate (no SQL-side filter)".into(),
            ))
        }
        "parent" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator("parent only supports =".into()));
            }
            Err(QueryError::ComplexQuery(
                "parent filtering requires predicate (no SQL-side filter)".into(),
            ))
        }
        "ephemeral" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator(
                    "ephemeral only supports =".into(),
                ));
            }
            Err(QueryError::ComplexQuery(
                "ephemeral filtering requires predicate".into(),
            ))
        }
        "template" | "is_template" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator(
                    "template only supports =".into(),
                ));
            }
            Err(QueryError::ComplexQuery(
                "template filtering requires predicate".into(),
            ))
        }
        "has_metadata_key" => {
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator(
                    "has_metadata_key only supports =".into(),
                ));
            }
            Err(QueryError::ComplexQuery(
                "has_metadata_key filtering requires predicate (no SQL-side filter)".into(),
            ))
        }
        f if f.starts_with("metadata.") => {
            let key = &f["metadata.".len()..];
            if op != ComparisonOp::Eq {
                return Err(QueryError::InvalidOperator(
                    "metadata only supports =".into(),
                ));
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
fn build_predicate(node: &QueryNode) -> IssuePredicate {
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
#[allow(clippy::too_many_lines)]
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
        "type" | "issue_type" | "issue-type" => apply_str_op(&issue.issue_type.to_string(), value),
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
            if let Some(prefix) = value.strip_suffix('*') {
                let matched = id.starts_with(prefix);
                match op {
                    ComparisonOp::Eq => matched,
                    ComparisonOp::NotEq => !matched,
                    _ => false,
                }
            } else {
                apply_str_op(id, value)
            }
        }
        "labels" => match op {
            ComparisonOp::Eq => issue.labels.iter().any(|l| l.eq_ignore_ascii_case(value)),
            ComparisonOp::NotEq => !issue.labels.iter().any(|l| l.eq_ignore_ascii_case(value)),
            _ => false,
        },
        "description" | "desc" => {
            let desc = issue.description.as_deref().unwrap_or("");
            let issue_desc = desc.to_lowercase();
            let search = value.to_lowercase();
            match op {
                ComparisonOp::Eq => issue_desc.contains(&search),
                ComparisonOp::NotEq => !issue_desc.contains(&search),
                _ => false,
            }
        }
        "notes" => {
            let n = issue.notes.as_deref().unwrap_or("");
            let issue_notes = n.to_lowercase();
            let search = value.to_lowercase();
            match op {
                ComparisonOp::Eq => issue_notes.contains(&search),
                ComparisonOp::NotEq => !issue_notes.contains(&search),
                _ => false,
            }
        }
        "closed" | "closed_at" => {
            let ts = match parse_timestamp_value(value) {
                Ok(t) => t,
                Err(_) => return false,
            };
            let issue_ts = match issue.closed_at {
                Some(t) => t,
                None => return false,
            };
            match op {
                ComparisonOp::Eq => issue_ts.date_naive() == ts.date_naive(),
                ComparisonOp::NotEq => issue_ts.date_naive() != ts.date_naive(),
                ComparisonOp::Less => issue_ts < ts,
                ComparisonOp::LessEq => issue_ts <= ts,
                ComparisonOp::Greater => issue_ts > ts,
                ComparisonOp::GreaterEq => issue_ts >= ts,
            }
        }
        "started" | "started_at" => {
            let ts = match parse_timestamp_value(value) {
                Ok(t) => t,
                Err(_) => return false,
            };
            let issue_ts = match issue.started_at {
                Some(t) => t,
                None => return false,
            };
            match op {
                ComparisonOp::Eq => issue_ts.date_naive() == ts.date_naive(),
                ComparisonOp::NotEq => issue_ts.date_naive() != ts.date_naive(),
                ComparisonOp::Less => issue_ts < ts,
                ComparisonOp::LessEq => issue_ts <= ts,
                ComparisonOp::Greater => issue_ts > ts,
                ComparisonOp::GreaterEq => issue_ts >= ts,
            }
        }
        "spec" | "spec_id" => {
            let s = issue.spec_id.as_deref().unwrap_or("");
            apply_str_op(s, value)
        }
        "parent" => {
            // parent-child dep check would need to look at the dependency graph;
            // for now, treat as unknown field (no match) in predicate path
            false
        }
        "ephemeral" => match op {
            ComparisonOp::Eq if value == "true" || value == "1" => issue.ephemeral,
            ComparisonOp::Eq => !issue.ephemeral,
            ComparisonOp::NotEq if value == "true" || value == "1" => !issue.ephemeral,
            ComparisonOp::NotEq => issue.ephemeral,
            _ => false,
        },
        "template" | "is_template" => match op {
            ComparisonOp::Eq if value == "true" || value == "1" => issue.is_template,
            ComparisonOp::Eq => !issue.is_template,
            ComparisonOp::NotEq if value == "true" || value == "1" => !issue.is_template,
            ComparisonOp::NotEq => issue.is_template,
            _ => false,
        },
        "has_metadata_key" => {
            let key = value;
            issue
                .metadata
                .as_deref()
                .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                .is_some_and(|v| v.get(key).is_some())
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

/// Determine whether a predicate-path query can match closed (terminal)
/// issues. The storage layer excludes closed/tombstone rows unless
/// `include_closed` is set, so queries that negate `status` or explicitly
/// name a terminal status inside an `OR`/`NOT` tree must opt in — otherwise
/// matching rows are filtered out before the predicate ever sees them.
///
/// This is deliberately conservative: returning `true` only widens the SQL
/// candidate set (the predicate remains authoritative), while `false` is
/// only returned when no status comparison can admit a terminal status.
fn query_needs_closed(node: &QueryNode) -> bool {
    match node {
        QueryNode::Comparison { field, op, value } => {
            if field != "status" {
                return false;
            }
            match op {
                // `status!=x` admits every status except x — including closed
                // whenever x is not itself the excluded terminal status. Even
                // for `!=closed` this is safe: the predicate re-filters.
                ComparisonOp::NotEq => true,
                ComparisonOp::Eq => crate::model::Status::from_str(value)
                    .map(|s| s.is_terminal())
                    .unwrap_or(false),
                _ => false,
            }
        }
        QueryNode::And(left, right) | QueryNode::Or(left, right) => {
            query_needs_closed(left) || query_needs_closed(right)
        }
        QueryNode::Not(inner) => query_needs_closed(inner),
    }
}

/// Evaluate a query AST and produce a QueryResult.
pub fn evaluate(node: &QueryNode) -> Result<QueryResult, QueryError> {
    let mut filters = ListFilters::default();

    if can_use_filter_only(node) {
        // Try filter-only path; fall back to predicate path on ComplexQuery
        // (some fields like description/notes are predicate-only).
        match build_filters(node, &mut filters) {
            Ok(()) => {
                return Ok(QueryResult {
                    filters,
                    requires_predicate: false,
                    predicate: None,
                });
            }
            Err(QueryError::ComplexQuery(_)) => {
                // Fall through to predicate path
            }
            Err(e) => return Err(e),
        }
    }

    // Complex query: build base filters (best-effort), build predicate
    let _ = build_filters(node, &mut filters);
    if query_needs_closed(node) {
        filters.include_closed = true;
    }
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
        assert_eq!(
            result.filters.statuses,
            Some(vec![crate::model::Status::Open])
        );
    }

    #[test]
    fn test_and_chain() {
        let result = parse_and_evaluate("status=open AND type=bug").unwrap();
        assert!(!result.requires_predicate);
        assert_eq!(
            result.filters.statuses,
            Some(vec![crate::model::Status::Open])
        );
        assert_eq!(
            result.filters.types,
            Some(vec![crate::model::IssueType::Bug])
        );
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
        assert_eq!(
            result.filters.priorities,
            Some(vec![crate::model::Priority(2)])
        );
    }

    #[test]
    fn test_priority_range_predicate() {
        // priority range queries now use the predicate path
        let result = parse_and_evaluate("priority>2").unwrap();
        assert!(result.requires_predicate);
        assert!(result.predicate.is_some());
    }

    #[test]
    fn test_multiple_and() {
        let result = parse_and_evaluate("status=open AND priority=1 AND label=urgent").unwrap();
        assert_eq!(
            result.filters.statuses,
            Some(vec![crate::model::Status::Open])
        );
        assert_eq!(
            result.filters.priorities,
            Some(vec![crate::model::Priority(1)])
        );
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

    #[test]
    fn test_owner_filter_unquoted_email() {
        // Regression: `owner=alice@example.com` used to fail tokenization on
        // the `@` (or silently mismatch). `@` is now a valid ident character.
        let result = parse_and_evaluate("owner=alice@example.com").unwrap();
        assert_eq!(result.filters.owner, Some("alice@example.com".to_string()));
    }

    #[test]
    fn test_iso_date_created() {
        let result = parse_and_evaluate("created_at>2026-01-15").unwrap();
        assert!(!result.requires_predicate);
        assert!(result.filters.created_after.is_some());
    }

    #[test]
    fn test_natural_language_yesterday() {
        let result = parse_and_evaluate("created>yesterday").unwrap();
        assert!(!result.requires_predicate);
        assert!(result.filters.created_after.is_some());
    }

    #[test]
    fn test_natural_language_today() {
        let result = parse_and_evaluate("updated<today").unwrap();
        assert!(!result.requires_predicate);
        assert!(result.filters.updated_before.is_some());
    }

    #[test]
    fn test_natural_language_tomorrow() {
        let result = parse_and_evaluate("created<tomorrow").unwrap();
        assert!(!result.requires_predicate);
        assert!(result.filters.created_before.is_some());
    }

    #[test]
    fn test_natural_language_next_week() {
        let result = parse_and_evaluate("updated>next-week").unwrap();
        assert!(!result.requires_predicate);
        assert!(result.filters.updated_after.is_some());
    }

    #[test]
    fn test_natural_language_last_week() {
        let result = parse_and_evaluate("created_at>last-week").unwrap();
        assert!(!result.requires_predicate);
        assert!(result.filters.created_after.is_some());
    }

    #[test]
    fn test_natural_language_last_month() {
        let result = parse_and_evaluate("created<last-month").unwrap();
        assert!(!result.requires_predicate);
        assert!(result.filters.created_before.is_some());
    }

    #[test]
    fn test_natural_language_now() {
        let result = parse_and_evaluate("created<now").unwrap();
        assert!(!result.requires_predicate);
        assert!(result.filters.created_before.is_some());
    }

    #[test]
    fn test_positive_relative_offset() {
        let result = parse_and_evaluate("updated<+1d").unwrap();
        assert!(!result.requires_predicate);
        assert!(result.filters.updated_before.is_some());
    }

    #[test]
    fn test_negative_relative_offset() {
        let result = parse_and_evaluate("created>-3d").unwrap();
        assert!(!result.requires_predicate);
        assert!(result.filters.created_after.is_some());
    }

    #[test]
    fn test_invalid_timestamp_value() {
        let err = parse_and_evaluate("created>not-a-date").unwrap_err();
        assert!(matches!(err, QueryError::InvalidValue(_)));
    }

    // ---- New query DSL field tests ----

    #[test]
    fn test_description_field_requires_predicate() {
        let result = parse_and_evaluate("description=hello").unwrap();
        assert!(result.requires_predicate);
    }

    #[test]
    fn test_notes_field_requires_predicate() {
        let result = parse_and_evaluate("notes=something").unwrap();
        assert!(result.requires_predicate);
    }

    #[test]
    fn test_closed_at_field_requires_predicate() {
        let result = parse_and_evaluate("closed_at>7d").unwrap();
        assert!(result.requires_predicate);
    }

    #[test]
    fn test_started_at_field_requires_predicate() {
        let result = parse_and_evaluate("started>3d").unwrap();
        assert!(result.requires_predicate);
    }

    #[test]
    fn test_spec_field_requires_predicate() {
        let result = parse_and_evaluate("spec=abc-123").unwrap();
        assert!(result.requires_predicate);
    }

    #[test]
    fn test_parent_field_requires_predicate() {
        let result = parse_and_evaluate("parent=br-abc123").unwrap();
        assert!(result.requires_predicate);
    }

    #[test]
    fn test_ephemeral_field_requires_predicate() {
        let result = parse_and_evaluate("ephemeral=true").unwrap();
        assert!(result.requires_predicate);
    }

    #[test]
    fn test_template_field_requires_predicate() {
        let result = parse_and_evaluate("template=false").unwrap();
        assert!(result.requires_predicate);
    }

    #[test]
    fn test_has_metadata_key_requires_predicate() {
        let result = parse_and_evaluate("has_metadata_key=component").unwrap();
        assert!(result.requires_predicate);
    }

    fn make_eval_test_issue() -> crate::model::Issue {
        crate::model::Issue {
            id: "test-001".to_string(),
            title: "Test Issue".to_string(),
            description: Some("A test description".to_string()),
            status: crate::model::Status::Open,
            priority: crate::model::Priority::MEDIUM,
            issue_type: crate::model::IssueType::Task,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            ..crate::model::Issue::default()
        }
    }

    #[test]
    fn test_description_predicate_matches() {
        let mut issue = make_eval_test_issue();
        issue.description = Some("The quick brown fox".to_string());
        let pred_fn = build_predicate_for_test("description=quick").unwrap();
        assert!(pred_fn(&issue));
        let pred_fn2 = build_predicate_for_test("description=nonexistent").unwrap();
        assert!(!pred_fn2(&issue));
    }

    #[test]
    fn test_closed_at_predicate() {
        let mut issue = make_eval_test_issue();
        issue.closed_at = Some(chrono::Utc::now());
        let pred_fn = build_predicate_for_test("closed_at<+1d").unwrap();
        assert!(pred_fn(&issue));
    }

    #[test]
    fn test_spec_id_predicate() {
        let mut issue = make_eval_test_issue();
        issue.spec_id = Some("SPEC-001".to_string());
        let pred_fn = build_predicate_for_test("spec_id=SPEC-001").unwrap();
        assert!(pred_fn(&issue));
        let pred_fn2 = build_predicate_for_test("spec_id=SPEC-002").unwrap();
        assert!(!pred_fn2(&issue));
    }

    #[test]
    fn test_ephemeral_predicate() {
        let mut issue = make_eval_test_issue();
        issue.ephemeral = true;
        let pred_fn = build_predicate_for_test("ephemeral=true").unwrap();
        assert!(pred_fn(&issue));
    }

    #[test]
    fn test_template_predicate() {
        let mut issue = make_eval_test_issue();
        issue.is_template = true;
        let pred_fn = build_predicate_for_test("template=true").unwrap();
        assert!(pred_fn(&issue));
    }

    #[test]
    fn test_status_not_eq_uses_predicate() {
        let result = parse_and_evaluate("status!=open").unwrap();
        assert!(result.requires_predicate);
        assert!(result.filters.include_closed);
        let pred = result.predicate.unwrap();
        let mut issue = make_eval_test_issue(); // Open
        assert!(!pred(&issue));
        issue.status = crate::model::Status::Closed;
        assert!(pred(&issue)); // closed != open
        issue.status = crate::model::Status::InProgress;
        assert!(pred(&issue));
    }

    #[test]
    fn test_type_not_eq_uses_predicate() {
        let result = parse_and_evaluate("type!=feature").unwrap();
        assert!(result.requires_predicate);
        let pred = result.predicate.unwrap();
        let issue = make_eval_test_issue(); // Task
        assert!(pred(&issue));
        let mut feature = make_eval_test_issue();
        feature.issue_type = crate::model::IssueType::Feature;
        assert!(!pred(&feature));
    }

    #[test]
    fn test_issue_type_alias() {
        let result = parse_and_evaluate("issue_type=bug").unwrap();
        assert!(!result.requires_predicate);
        assert_eq!(
            result.filters.types,
            Some(vec![crate::model::IssueType::Bug])
        );
        let result2 = parse_and_evaluate("issue-type=bug").unwrap();
        assert_eq!(
            result2.filters.types,
            Some(vec![crate::model::IssueType::Bug])
        );
    }

    #[test]
    fn test_status_eq_closed_marks_filter_for_closed() {
        // Filter-only path: statuses carries Closed; the caller (list) is
        // responsible for include_closed — the evaluator leaves it false so
        // `status=closed` doesn't widen unrelated queries.
        let result = parse_and_evaluate("status=closed").unwrap();
        assert!(!result.requires_predicate);
        assert_eq!(
            result.filters.statuses,
            Some(vec![crate::model::Status::Closed])
        );
    }

    #[test]
    fn test_or_with_closed_status_needs_closed() {
        let result = parse_and_evaluate("status=open OR status=closed").unwrap();
        assert!(result.requires_predicate);
        assert!(result.filters.include_closed);
    }

    #[test]
    fn test_predicate_without_status_does_not_include_closed() {
        let result = parse_and_evaluate("priority>1").unwrap();
        assert!(result.requires_predicate);
        assert!(!result.filters.include_closed);
    }

    /// Helper: parse a query string and return the predicate function.
    /// Panics on parse/eval errors.
    fn build_predicate_for_test(query: &str) -> Option<IssuePredicate> {
        let result = parse_and_evaluate(query).unwrap();
        result.predicate
    }
}
