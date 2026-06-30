//! # Storage — trait-based storage interface
//!
//! Defines the [`Storage`] trait: the primary interface for beads persistence.
//! This allows alternative backends (SQLite, in-memory for tests, etc.) to be
//! substituted without changing the command layer.
//!
//! ## Design
//!
//! The trait exposes the 20 most-used storage operations. The
//! [`SqliteStorage`] backend implements this trait. For testing, see
//! [`InMemoryStorage`].
//!
//! ## Trait vs. Concrete Type
//!
//! The CLI layer accepts `&mut SqliteStorage` directly for performance.
//! For cases that need dynamic dispatch (e.g., mock/test backends), use
//! `Box<dyn Storage>`.

use crate::error::{BeadsError, Result};
use crate::model::{Event, Issue};
use crate::storage::sqlite::{
    IssueMetadata, IssueUpdate, ListFilters, ReadyFilters, StatsIssueRow,
};

/// Primary storage interface — implemented by [`SqliteStorage`][super::sqlite::SqliteStorage]
/// and [`InMemoryStorage`][super::InMemoryStorage].
///
/// This trait captures the 20 most-called storage operations. Unlisted methods
/// are available only on the concrete `SqliteStorage` type via downcasting.
pub trait Storage {
    // ------------------------------------------------------------------------
    // Issue CRUD
    // ------------------------------------------------------------------------

    /// Create a new issue.
    fn create_issue(&mut self, issue: &Issue, actor: &str) -> Result<()>;

    /// Fetch a single issue by ID, or `None` if it doesn't exist.
    fn get_issue(&self, id: &str) -> Result<Option<Issue>>;

    /// Update an existing issue with the given changes.
    fn update_issue(
        &mut self,
        id: &str,
        updates: &super::sqlite::IssueUpdate,
        actor: &str,
    ) -> Result<Issue>;

    /// Hard-delete an issue (sets `deleted_at`).
    fn delete_issue(&mut self, id: &str, actor: &str) -> Result<()>;

    /// List issues matching the given filters.
    fn list_issues(&self, filters: &ListFilters) -> Result<Vec<Issue>>;

    /// Fetch multiple issues by their IDs.
    fn get_issues_by_ids(&self, ids: &[String]) -> Result<Vec<Issue>>;

    /// Check whether an issue ID exists.
    fn id_exists(&self, id: &str) -> bool;

    /// Search issues by free-text query with filters.
    fn search_issues(&self, query: &str, filters: &ListFilters) -> Result<Vec<Issue>>;

    /// Get all issue metadata (id, title, status, priority) for quick enumeration.
    fn get_all_issues_metadata(&self) -> Result<Vec<IssueMetadata>>;

    // ------------------------------------------------------------------------
    // Labels
    // ------------------------------------------------------------------------

    /// Add a label to an issue.
    fn add_label(&mut self, issue_id: &str, label: &str, actor: &str) -> Result<()>;

    /// Remove a label from an issue.
    fn remove_label(&mut self, issue_id: &str, label: &str, actor: &str) -> Result<()>;

    /// Get all labels on an issue.
    fn get_labels(&self, issue_id: &str) -> Result<Vec<String>>;

    // ------------------------------------------------------------------------
    // Dependencies
    // ------------------------------------------------------------------------

    /// Add a dependency: `issue_id` depends on `depends_on_id`.
    fn add_dependency(
        &mut self,
        issue_id: &str,
        depends_on_id: &str,
        dep_type: &str,
        actor: &str,
    ) -> Result<()>;

    /// Remove a dependency.
    fn remove_dependency(&mut self, issue_id: &str, depends_on_id: &str, actor: &str)
        -> Result<()>;

    /// Get issues that `issue_id` depends on.
    fn get_dependencies(&self, issue_id: &str) -> Result<Vec<Issue>>;

    /// Get issues that depend on `issue_id`.
    fn get_dependents(&self, issue_id: &str) -> Result<Vec<Issue>>;

    // ------------------------------------------------------------------------
    // Ready / blocked work queries
    // ------------------------------------------------------------------------

    /// List issues that are ready to work (no open blockers), matching filters.
    fn get_ready_work(&self, filters: &ReadyFilters) -> Result<Vec<Issue>>;

    /// List issues that are blocked by open dependencies.
    fn get_blocked_issues(&self, filters: &ReadyFilters) -> Result<Vec<Issue>>;

    // ------------------------------------------------------------------------
    // Statistics
    // ------------------------------------------------------------------------

    /// Get per-status/priority/type breakdown for the stats panel.
    fn list_stats_issues(&self) -> Result<Vec<StatsIssueRow>>;

    // ------------------------------------------------------------------------
    // Audit events
    // ------------------------------------------------------------------------

    /// Get the audit events for an issue.
    fn get_events(&self, issue_id: &str, limit: usize) -> Result<Vec<Event>>;

    // ------------------------------------------------------------------------
    // Configuration
    // ------------------------------------------------------------------------

    /// Get a config value, or `None` if unset.
    fn get_config(&self, key: &str) -> Result<Option<String>>;

    /// Set a config key-value pair.
    fn set_config(&mut self, key: &str, value: &str) -> Result<()>;

    // ------------------------------------------------------------------------
    // Internal helpers (commonly used by commands)
    // ------------------------------------------------------------------------

    /// Check whether an issue ID matches the configured prefix.
    fn validate_prefix(&self, id: &str) -> Result<()> {
        let prefix = self
            .get_config("issue_prefix")?
            .unwrap_or_else(|| "beads".to_string());
        if !id.starts_with(&prefix) && !id.starts_with("beads-") {
            return Err(BeadsError::Validation {
                field: "id".to_string(),
                reason: format!(
                    "issue ID '{}' does not match project prefix '{}'",
                    id, prefix
                ),
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// In-memory storage backend (for testing)
// ---------------------------------------------------------------------------

use crate::model::{IssueType, Priority, Status};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// In-memory storage backend for unit testing.
///
/// This is a minimal implementation that stores issues in a `HashMap`.
/// It does NOT support:
/// - dependencies
/// - events
/// - ready/blocked queries (returns empty)
/// - transactions
/// - JSONL export dirty-tracking
///
/// Use `Arc<RwLock<InMemoryStorage>>` for shared test access.
pub struct InMemoryStorage {
    issues: HashMap<String, Issue>,
    labels: HashMap<String, Vec<String>>,
    deps: HashMap<String, Vec<String>>,
    dependents: HashMap<String, Vec<String>>,
    config: HashMap<String, String>,
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryStorage {
    /// Create a new empty in-memory store.
    pub fn new() -> Self {
        Self {
            issues: HashMap::new(),
            labels: HashMap::new(),
            deps: HashMap::new(),
            dependents: HashMap::new(),
            config: HashMap::new(),
        }
    }

    /// Pre-load a set of issues into the store.
    pub fn with_issues(mut self, issues: impl IntoIterator<Item = Issue>) -> Self {
        for issue in issues {
            let id = issue.id.clone();
            self.issues.insert(id, issue);
        }
        self
    }
}

impl Storage for InMemoryStorage {
    fn create_issue(&mut self, issue: &Issue, _actor: &str) -> Result<()> {
        if self.issues.contains_key(&issue.id) {
            return Err(BeadsError::IdCollision {
                id: issue.id.clone(),
            });
        }
        self.issues.insert(issue.id.clone(), issue.clone());
        Ok(())
    }

    fn get_issue(&self, id: &str) -> Result<Option<Issue>> {
        Ok(self.issues.get(id).cloned())
    }

    fn update_issue(
        &mut self,
        id: &str,
        updates: &super::sqlite::IssueUpdate,
        _actor: &str,
    ) -> Result<Issue> {
        let issue = self
            .issues
            .get_mut(id)
            .ok_or(BeadsError::IssueNotFound { id: id.to_string() })?;

        if let Some(title) = &updates.title {
            issue.title = title.clone();
        }
        if let Some(description) = &updates.description {
            issue.description = description.clone();
        }
        if let Some(status) = &updates.status {
            issue.status = status.clone();
        }
        if let Some(priority) = &updates.priority {
            issue.priority = *priority;
        }
        if let Some(assignee) = &updates.assignee {
            issue.assignee = assignee.clone();
        }
        if let Some(metadata) = &updates.metadata {
            if let Some(m) = metadata {
                issue.metadata = Some(m.clone());
            }
        }

        let updated = issue.clone();
        *issue = updated.clone();
        Ok(updated)
    }

    fn delete_issue(&mut self, id: &str, _actor: &str) -> Result<()> {
        let issue = self
            .issues
            .get_mut(id)
            .ok_or(BeadsError::IssueNotFound { id: id.to_string() })?;
        issue.deleted_at = Some(chrono::Utc::now());
        Ok(())
    }

    fn list_issues(&self, filters: &ListFilters) -> Result<Vec<Issue>> {
        let mut results: Vec<Issue> = self
            .issues
            .values()
            .filter(|i| i.deleted_at.is_none())
            .cloned()
            .collect();

        if let Some(statuses) = &filters.statuses {
            results.retain(|i| statuses.contains(&i.status));
        }
        if let Some(priorities) = &filters.priorities {
            results.retain(|i| priorities.contains(&i.priority));
        }
        if let Some(types) = &filters.types {
            results.retain(|i| types.contains(&i.issue_type));
        }
        if let Some(assignee) = &filters.assignee {
            results.retain(|i| i.assignee.as_deref() == Some(assignee));
        }
        if filters.unassigned {
            results.retain(|i| i.assignee.is_none());
        }

        Ok(results)
    }

    fn get_issues_by_ids(&self, ids: &[String]) -> Result<Vec<Issue>> {
        let mut results = Vec::new();
        for id in ids {
            if let Some(issue) = self.issues.get(id) {
                results.push(issue.clone());
            }
        }
        Ok(results)
    }

    fn id_exists(&self, id: &str) -> bool {
        self.issues.contains_key(id)
    }

    fn search_issues(&self, query: &str, filters: &ListFilters) -> Result<Vec<Issue>> {
        let query_lower = query.to_lowercase();
        let all: Vec<Issue> = self
            .issues
            .values()
            .filter(|i| i.deleted_at.is_none())
            .filter(|i| {
                i.title.to_lowercase().contains(&query_lower)
                    || i.description
                        .as_ref()
                        .map(|d| d.to_lowercase().contains(&query_lower))
                        .unwrap_or(false)
            })
            .cloned()
            .collect();

        let mut results: Vec<Issue> = all;

        if let Some(statuses) = &filters.statuses {
            results.retain(|i| statuses.contains(&i.status));
        }
        if let Some(priorities) = &filters.priorities {
            results.retain(|i| priorities.contains(&i.priority));
        }

        Ok(results)
    }

    fn get_all_issues_metadata(&self) -> Result<Vec<IssueMetadata>> {
        Ok(self
            .issues
            .values()
            .filter(|i| i.deleted_at.is_none())
            .map(|i| IssueMetadata {
                id: i.id.clone(),
                external_ref: i.external_ref.clone(),
                content_hash: i.content_hash.clone(),
                status: i.status.clone(),
                updated_at: i.updated_at,
            })
            .collect())
    }

    fn add_label(&mut self, issue_id: &str, label: &str, _actor: &str) -> Result<()> {
        if !self.issues.contains_key(issue_id) {
            return Err(BeadsError::IssueNotFound {
                id: issue_id.to_string(),
            });
        }
        let labels = self.labels.entry(issue_id.to_string()).or_default();
        if !labels.contains(&label.to_string()) {
            labels.push(label.to_string());
        }
        Ok(())
    }

    fn remove_label(&mut self, issue_id: &str, label: &str, _actor: &str) -> Result<()> {
        let labels = self.labels.get_mut(issue_id);
        if let Some(labels) = labels {
            labels.retain(|l| l != label);
        }
        Ok(())
    }

    fn get_labels(&self, issue_id: &str) -> Result<Vec<String>> {
        Ok(self.labels.get(issue_id).cloned().unwrap_or_default())
    }

    fn add_dependency(
        &mut self,
        issue_id: &str,
        depends_on_id: &str,
        _dep_type: &str,
        _actor: &str,
    ) -> Result<()> {
        if !self.issues.contains_key(issue_id) {
            return Err(BeadsError::IssueNotFound {
                id: issue_id.to_string(),
            });
        }
        if !self.issues.contains_key(depends_on_id) {
            return Err(BeadsError::IssueNotFound {
                id: depends_on_id.to_string(),
            });
        }
        let deps = self.deps.entry(issue_id.to_string()).or_default();
        if !deps.contains(&depends_on_id.to_string()) {
            deps.push(depends_on_id.to_string());
        }
        let dependents = self.dependents.entry(depends_on_id.to_string()).or_default();
        if !dependents.contains(&issue_id.to_string()) {
            dependents.push(issue_id.to_string());
        }
        Ok(())
    }

    fn remove_dependency(
        &mut self,
        issue_id: &str,
        depends_on_id: &str,
        _actor: &str,
    ) -> Result<()> {
        if let Some(deps) = self.deps.get_mut(issue_id) {
            deps.retain(|d| d != depends_on_id);
        }
        if let Some(dependents) = self.dependents.get_mut(depends_on_id) {
            dependents.retain(|d| d != issue_id);
        }
        Ok(())
    }

    fn get_dependencies(&self, issue_id: &str) -> Result<Vec<Issue>> {
        let dep_ids = self.deps.get(issue_id).cloned().unwrap_or_default();
        let mut results = Vec::new();
        for id in dep_ids {
            if let Some(issue) = self.issues.get(&id) {
                results.push(issue.clone());
            }
        }
        Ok(results)
    }

    fn get_dependents(&self, issue_id: &str) -> Result<Vec<Issue>> {
        let dependent_ids = self.dependents.get(issue_id).cloned().unwrap_or_default();
        let mut results = Vec::new();
        for id in dependent_ids {
            if let Some(issue) = self.issues.get(&id) {
                results.push(issue.clone());
            }
        }
        Ok(results)
    }

    fn get_ready_work(&self, _filters: &ReadyFilters) -> Result<Vec<Issue>> {
        // Minimal: return all open issues not blocked
        Ok(self
            .issues
            .values()
            .filter(|i| {
                i.deleted_at.is_none()
                    && i.status == Status::Open
                    && !matches!(i.issue_type, IssueType::Epic)
                    && self.deps.get(&i.id).map_or(true, |d| d.is_empty())
            })
            .cloned()
            .collect())
    }

    fn get_blocked_issues(&self, _filters: &ReadyFilters) -> Result<Vec<Issue>> {
        Ok(self
            .issues
            .values()
            .filter(|i| {
                i.deleted_at.is_none()
                    && i.status == Status::Open
                    && self
                        .deps
                        .get(&i.id)
                        .map_or(false, |d| !d.is_empty())
            })
            .cloned()
            .collect())
    }

    fn list_stats_issues(&self) -> Result<Vec<StatsIssueRow>> {
        // In-memory store doesn't track stats — return the full issue list
        // cast to StatsIssueRow (same underlying shape)
        Ok(self
            .issues
            .values()
            .filter(|i| i.deleted_at.is_none())
            .map(|i| StatsIssueRow {
                id: i.id.clone(),
                status: i.status.clone(),
                priority: i.priority,
                issue_type: i.issue_type.clone(),
                assignee: i.assignee.clone(),
                created_at: i.created_at,
                closed_at: i.closed_at,
                defer_until: i.defer_until,
                ephemeral: i.ephemeral,
                pinned: i.pinned,
                is_template: i.is_template,
            })
            .collect())
    }

    fn get_events(&self, _issue_id: &str, _limit: usize) -> Result<Vec<Event>> {
        // In-memory store doesn't track events
        Ok(vec![])
    }

    fn get_config(&self, key: &str) -> Result<Option<String>> {
        Ok(self.config.get(key).cloned())
    }

    fn set_config(&mut self, key: &str, value: &str) -> Result<()> {
        self.config.insert(key.to_string(), value.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Priority;

    fn make_issue(id: &str, title: &str) -> Issue {
        Issue {
            id: id.to_string(),
            title: title.to_string(),
            status: Status::Open,
            priority: Priority::CRITICAL,
            issue_type: IssueType::Task,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            ..Default::default()
        }
    }

    #[test]
    fn test_in_memory_create_get() {
        let mut store = InMemoryStorage::new();
        let issue = make_issue("test-1", "Test issue");
        store.create_issue(&issue, "alice").unwrap();

        let found = store.get_issue("test-1").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().title, "Test issue");
    }

    #[test]
    fn test_in_memory_update() {
        let mut store = InMemoryStorage::new();
        let issue = make_issue("test-1", "Original");
        store.create_issue(&issue, "alice").unwrap();

        let updates = super::super::sqlite::IssueUpdate {
            title: Some("Updated".to_string()),
            status: Some(Status::InProgress),
            ..Default::default()
        };
        let updated = store.update_issue("test-1", &updates, "alice").unwrap();
        assert_eq!(updated.title, "Updated");
        assert_eq!(updated.status, Status::InProgress);
    }

    #[test]
    fn test_in_memory_labels() {
        let mut store = InMemoryStorage::new();
        store.create_issue(&make_issue("test-1", "Test"), "alice").unwrap();

        store.add_label("test-1", "backend", "alice").unwrap();
        store.add_label("test-1", "urgent", "alice").unwrap();

        let labels = store.get_labels("test-1").unwrap();
        assert_eq!(labels, vec!["backend", "urgent"]);
    }

    #[test]
    fn test_in_memory_dependencies() {
        let mut store = InMemoryStorage::new();
        store.create_issue(&make_issue("a", "A"), "alice").unwrap();
        store.create_issue(&make_issue("b", "B"), "alice").unwrap();

        store.add_dependency("a", "b", "blocks", "alice").unwrap();

        let deps = store.get_dependencies("a").unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].id, "b");

        let dependents = store.get_dependents("b").unwrap();
        assert_eq!(dependents.len(), 1);
        assert_eq!(dependents[0].id, "a");
    }

    #[test]
    fn test_in_memory_search() {
        let mut store = InMemoryStorage::new();
        store
            .issues
            .insert("1".to_string(), make_issue("1", "Fix login bug"));
        store
            .issues
            .insert("2".to_string(), make_issue("2", "Add dark mode"));
        store
            .issues
            .insert("3".to_string(), make_issue("3", "Fix performance"));

        let results = store.search_issues("fix", &ListFilters::default()).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_in_memory_config() {
        let mut store = InMemoryStorage::new();
        store.set_config("issue_prefix", "test").unwrap();
        assert_eq!(store.get_config("issue_prefix").unwrap(), Some("test".to_string()));
        assert_eq!(store.get_config("missing").unwrap(), None);
    }

    #[test]
    fn test_in_memory_id_exists() {
        let mut store = InMemoryStorage::new();
        store.create_issue(&make_issue("test-1", "Test"), "alice").unwrap();
        assert!(store.id_exists("test-1"));
        assert!(!store.id_exists("test-99"));
    }

    #[test]
    fn test_storage_trait_object() {
        let issue = make_issue("test-1", "Trait test");
        let store: Box<dyn Storage> = Box::new(InMemoryStorage::new().with_issues([issue]));
        assert!(store.get_issue("test-1").unwrap().is_some());
    }
}
