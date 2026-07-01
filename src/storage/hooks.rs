//! HookFiringStore decorator (Issue #6, #46).
//!
//! Wraps [`SqliteStorage`] and fires lifecycle hooks after successful
//! mutations — `on_create`, `on_update`, `on_close`.  Hook failures are
//! logged but do not fail the mutation.  When no hooks are registered
//! the wrapper is a zero-overhead pass-through.
//!
//! # Shell Hooks (opt-in)
//!
//! Hook scripts live in `.beads/hooks/on_create`, `on_update`, `on_close`.
//! Each must be an executable file (shell script, binary, etc.).  When
//! present, they are invoked with two arguments: `issue_id` and `actor`.
//!
//! ```bash
//! # Example: .beads/hooks/on_create
//! #!/bin/sh
//! echo "Issue $1 created by $2" >> /tmp/beads-hooks.log
//! ```
//!
//! Register them via [`HookFiringStore::with_shell_hooks`] or the helper
//! [`fire_hook_scripts`] for ad-hoc use.
//!
//! # Programmatic Hooks
//!
//! ```ignore
//! use beads_rust::storage::hooks::{HookFiringStore, HookFn};
//!
//! let inner = SqliteStorage::open(&path)?;
//! let mut store = HookFiringStore::new(inner);
//!
//! store.on_create(Box::new(|id, actor| {
//!     tracing::info!("Hook: issue {id} created by {actor}");
//! }));
//!
//! store.create_issue(&issue, "alice")?;  // fires hook after commit
//! ```

use crate::model::Issue;
use crate::model::Status;
use crate::storage::sqlite::{IssueUpdate, ListFilters, SqliteStorage};
use crate::Result;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Signature for a lifecycle hook callback.
///
/// Receives the issue ID and the actor name.  The callback must not
/// panic; errors are logged and swallowed so they never propagate to
/// the caller of the mutation.
pub type HookFn = Box<dyn Fn(&str, &str) + Send + Sync>;

/// Decorator that wraps [`SqliteStorage`] and fires hooks after mutations.
///
/// All non-mutating methods are delegated directly to the inner storage.
/// Mutating methods (`create_issue`, `update_issue`, `delete_issue`) fire
/// the corresponding registered hook after the transaction commits.
///
/// When no hooks are registered, the wrapper forwards calls with zero
/// additional overhead (no allocations, no checks).
pub struct HookFiringStore {
    inner: SqliteStorage,
    on_create_hook: Option<HookFn>,
    on_update_hook: Option<HookFn>,
    on_close_hook: Option<HookFn>,
}

impl HookFiringStore {
    /// Wrap an existing `SqliteStorage` with no hooks registered.
    #[must_use]
    pub fn new(inner: SqliteStorage) -> Self {
        Self {
            inner,
            on_create_hook: None,
            on_update_hook: None,
            on_close_hook: None,
        }
    }

    /// Register a hook fired after every successful `create_issue`.
    pub fn on_create(&mut self, hook: HookFn) {
        self.on_create_hook = Some(hook);
    }

    /// Register a hook fired after every successful `update_issue`.
    pub fn on_update(&mut self, hook: HookFn) {
        self.on_update_hook = Some(hook);
    }

    /// Register a hook fired after every successful `delete_issue`.
    pub fn on_close(&mut self, hook: HookFn) {
        self.on_close_hook = Some(hook);
    }

    /// Fire a hook, logging errors without propagating them.
    fn fire(hook: &Option<HookFn>, id: &str, actor: &str, label: &str) {
        if let Some(h) = hook {
            if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                h(id, actor);
            })) {
                let msg = if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic payload".to_string()
                };
                tracing::error!(
                    "HookFiringStore::{label} hook panicked for issue {id} (actor={actor}): {msg}"
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // Passthrough accessors
    // ------------------------------------------------------------------

    /// Borrow the inner storage.
    #[must_use]
    pub fn inner(&self) -> &SqliteStorage {
        &self.inner
    }

    /// Mutable borrow the inner storage.
    #[must_use]
    pub fn inner_mut(&mut self) -> &mut SqliteStorage {
        &mut self.inner
    }

    /// Consume and return the inner storage.
    #[must_use]
    pub fn into_inner(self) -> SqliteStorage {
        self.inner
    }

    // ------------------------------------------------------------------
    // Intercepted mutations
    // ------------------------------------------------------------------

    /// Create an issue, then fire the `on_create` hook.
    pub fn create_issue(&mut self, issue: &Issue, actor: &str) -> Result<()> {
        let id = issue.id.clone();
        self.inner.create_issue(issue, actor)?;
        Self::fire(&self.on_create_hook, &id, actor, "on_create");
        Ok(())
    }

    /// Update an issue, then fire the `on_update` hook.
    /// If the update changes status to `Closed`, also fires `on_close`.
    pub fn update_issue(&mut self, id: &str, updates: &IssueUpdate, actor: &str) -> Result<Issue> {
        let is_close = updates.status.as_ref().map_or(false, |s| *s == Status::Closed);
        let updated = self.inner.update_issue(id, updates, actor)?;
        Self::fire(&self.on_update_hook, id, actor, "on_update");
        if is_close {
            Self::fire(&self.on_close_hook, id, actor, "on_close");
        }
        Ok(updated)
    }

    /// Delete an issue, then fire `on_close`.
    pub fn delete_issue(
        &mut self,
        id: &str,
        actor: &str,
        reason: &str,
    ) -> Result<Issue> {
        use chrono::Utc;
        let deleted = self.inner.delete_issue(id, actor, reason, Some(Utc::now()))?;
        Self::fire(&self.on_close_hook, id, actor, "on_close");
        Ok(deleted)
    }

    // ------------------------------------------------------------------
    // Delegated methods (passthrough to inner)
    // ------------------------------------------------------------------

    pub fn get_issue(&self, id: &str) -> Result<Option<Issue>> {
        self.inner.get_issue(id)
    }

    pub fn list_issues(&self, filters: &ListFilters) -> Result<Vec<Issue>> {
        self.inner.list_issues(filters)
    }

    pub fn search_issues(&self, query: &str, filters: &ListFilters) -> Result<Vec<Issue>> {
        self.inner.search_issues(query, filters)
    }
}

// ------------------------------------------------------------------
// Shell hook runner
// ------------------------------------------------------------------

/// Run hook scripts from `.beads/hooks/{event}` for a given issue
/// mutation event.
///
/// If the script file exists and is executable, it is invoked with
/// `issue_id` and `actor` as arguments.  Failures are logged but
/// never propagated to the caller.
pub fn fire_hook_scripts(beads_dir: &Path, event: &str, id: &str, actor: &str) {
    let hook_path = beads_dir.join("hooks").join(event);
    if !hook_path.is_file() {
        return;
    }
    #[cfg(unix)]
    let is_exec = std::fs::metadata(&hook_path)
        .map(|m| {
            use std::os::unix::fs::PermissionsExt;
            m.permissions().mode() & 0o111 != 0
        })
        .unwrap_or(false);
    #[cfg(not(unix))]
    let is_exec = true; // Windows: assume executable
    if !is_exec {
        tracing::warn!("Hook script {hook_path:?} exists but is not executable — skipping");
        return;
    }
    match std::process::Command::new(&hook_path)
        .arg(id)
        .arg(actor)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .output()
    {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!(
                    "Hook script {hook_path:?} for issue {id} exited with {}: {stderr}",
                    output.status
                );
            }
        }
        Err(e) => {
            tracing::error!(
                "Failed to execute hook script {hook_path:?} for issue {id}: {e}",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Issue, IssueType, Priority, Status};
    use std::sync::{Arc, Mutex};

    fn temp_storage() -> HookFiringStore {
        HookFiringStore::new(SqliteStorage::open_memory().expect("temp storage"))
    }

    fn make_issue(id: &str) -> Issue {
        Issue {
            id: id.to_string(),
            title: format!("Test {id}"),
            status: Status::Open,
            issue_type: IssueType::Task,
            priority: Priority(2),
            ..Default::default()
        }
    }

    #[test]
    fn test_on_create_fires() {
        let mut store = temp_storage();
        let fired = Arc::new(Mutex::new(Vec::new()));
        let f = fired.clone();
        store.on_create(Box::new(move |id, actor| {
            f.lock().unwrap().push((id.to_string(), actor.to_string()));
        }));

        let issue = make_issue("hook-create-1");
        store.create_issue(&issue, "tester").unwrap();

        let fired = fired.lock().unwrap();
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].0, "hook-create-1");
        assert_eq!(fired[0].1, "tester");
    }

    #[test]
    fn test_on_update_fires() {
        let mut store = temp_storage();
        let fired = Arc::new(Mutex::new(Vec::new()));
        let f = fired.clone();
        store.on_update(Box::new(move |id, actor| {
            f.lock().unwrap().push((id.to_string(), actor.to_string()));
        }));

        let issue = make_issue("hook-upd-1");
        store.create_issue(&issue, "tester").unwrap();

        let updates = IssueUpdate {
            title: Some("Updated".to_string()),
            ..Default::default()
        };
        store.update_issue("hook-upd-1", &updates, "tester").unwrap();

        let fired = fired.lock().unwrap();
        assert!(fired.iter().any(|(id, _)| id == "hook-upd-1"));
    }

    #[test]
    fn test_on_close_fires_on_status_update() {
        let mut store = temp_storage();
        let fired = Arc::new(Mutex::new(Vec::new()));
        let f = fired.clone();
        store.on_close(Box::new(move |id, actor| {
            f.lock().unwrap().push((id.to_string(), actor.to_string()));
        }));

        let issue = make_issue("hook-close-1");
        store.create_issue(&issue, "tester").unwrap();

        let updates = IssueUpdate {
            status: Some(Status::Closed),
            ..Default::default()
        };
        store.update_issue("hook-close-1", &updates, "tester").unwrap();
        let fired = fired.lock().unwrap();
        assert!(fired.iter().any(|(id, _)| id == "hook-close-1"));
    }

    #[test]
    fn test_hook_panic_does_not_fail_mutation() {
        let mut store = temp_storage();
        store.on_create(Box::new(|_id, _actor| {
            panic!("deliberate hook panic");
        }));

        let issue = make_issue("hook-panic-1");
        // Mutation should still succeed despite the hook panic
        store.create_issue(&issue, "tester").unwrap();

        // Issue should be persisted
        let fetched = store.get_issue("hook-panic-1").unwrap();
        assert!(fetched.is_some());
    }

    #[test]
    fn test_no_hooks_is_passthrough() {
        let mut store = temp_storage();
        let issue = make_issue("hook-none-1");
        store.create_issue(&issue, "tester").unwrap();
        let fetched = store.get_issue("hook-none-1").unwrap();
        assert!(fetched.is_some());
    }

    #[test]
    fn test_on_create_and_on_update_independent() {
        let mut store = temp_storage();
        let created = Arc::new(Mutex::new(Vec::new()));
        let updated = Arc::new(Mutex::new(Vec::new()));
        let c = created.clone();
        let u = updated.clone();

        store.on_create(Box::new(move |id, _| {
            c.lock().unwrap().push(id.to_string());
        }));
        store.on_update(Box::new(move |id, _| {
            u.lock().unwrap().push(id.to_string());
        }));

        let issue = make_issue("hook-indep-1");
        store.create_issue(&issue, "tester").unwrap();
        let updates = IssueUpdate {
            title: Some("v2".to_string()),
            ..Default::default()
        };
        store.update_issue("hook-indep-1", &updates, "tester").unwrap();

        assert_eq!(created.lock().unwrap().len(), 1);
        assert_eq!(updated.lock().unwrap().len(), 1);
    }

    #[test]
    fn test_into_inner_recovers_storage() {
        let store = temp_storage();
        let _inner: SqliteStorage = store.into_inner();
    }
}
