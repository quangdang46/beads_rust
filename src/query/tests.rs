//! Integration tests for the query module.

use crate::query::*;

#[test]
fn test_parse_and_evaluate_basic() {
    let result = parse_and_evaluate("status=open").unwrap();
    assert!(!result.requires_predicate);
}

#[test]
fn test_parse_and_evaluate_and_chain() {
    let result = parse_and_evaluate("status=open AND priority=1").unwrap();
    assert!(!result.requires_predicate);
}

#[test]
fn test_parse_and_evaluate_duration() {
    let result = parse_and_evaluate("updated>7d").unwrap();
    assert!(!result.requires_predicate);
    assert!(result.filters.updated_after.is_some());
}

#[test]
fn test_parse_and_evaluate_metadata() {
    let result = parse_and_evaluate("metadata.component=backend").unwrap();
    assert_eq!(
        result.filters.metadata_filters,
        Some(vec!["component=backend".to_string()])
    );
}

#[test]
fn test_parse_and_evaluate_wildcard() {
    let result = parse_and_evaluate("id=br-*").unwrap();
    assert_eq!(result.filters.ids, Some(vec!["br-%".to_string()]));
}

#[test]
fn test_parse_and_evaluate_assignee() {
    let result = parse_and_evaluate("assignee=alice").unwrap();
    assert_eq!(result.filters.assignee, Some("alice".to_string()));
}

#[test]
fn test_parse_error_handling() {
    let err = parse_and_evaluate("status= ").unwrap_err();
    assert!(matches!(err, QueryError::ParseError(_)));
}

#[test]
fn test_unknown_field() {
    let err = parse_and_evaluate("foobar=open").unwrap_err();
    assert!(matches!(err, QueryError::UnknownField(_)));
}
