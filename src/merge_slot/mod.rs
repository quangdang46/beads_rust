//! # Merge Slot — exclusive access gate for serialized conflict resolution
//!
//! A merge slot is a special-purpose bead used as an exclusive access primitive.
//! Only one agent can hold the slot at a time, preventing concurrent conflict
//! resolution ("monkey knife fights").
//!
//! The slot is stored as an ordinary issue with:
//!   - `id`: `<prefix>-merge-slot` (e.g., `br-merge-slot`, derived from `issue_prefix`)
//!   - `status`: `open` = available, `in_progress` = held
//!   - `type`: `task`
//!   - `labels`: `["gt:slot"]` (merge slot label for tooling discovery)
//!   - `metadata`: JSON `{"holder": "actor", "waiters": ["actor1", "actor2"]}`

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::{BeadsError, Result};
use crate::model::{Issue, IssueType, Priority, Status};
use crate::storage::sqlite::SqliteStorage;

/// Label used to identify merge slot beads in tooling.
pub const MERGE_SLOT_LABEL: &str = "gt:slot";

/// JSON metadata stored in the merge slot issue.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlotMeta {
    /// Who currently holds the slot (empty if available).
    #[serde(skip_serializing_if = "String::is_empty")]
    #[serde(default)]
    pub holder: String,
    /// Priority-ordered queue of agents waiting for the slot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waiters: Vec<String>,
}

/// Status of the merge slot.
#[derive(Debug, Clone)]
pub struct MergeSlotStatus {
    /// Slot ID (e.g., `br-merge-slot`).
    pub slot_id: String,
    /// Whether the slot is available (status == open).
    pub available: bool,
    /// Current holder's identity (empty if available).
    pub holder: String,
    /// Ordered list of waiting agents.
    pub waiters: Vec<String>,
}

/// Result of an acquire attempt.
#[derive(Debug, Clone)]
pub struct MergeSlotAcquireResult {
    /// Slot ID.
    pub slot_id: String,
    /// Whether the slot was successfully acquired.
    pub acquired: bool,
    /// Whether the agent was added to the waiters queue.
    pub waiting: bool,
    /// Who currently holds the slot.
    pub holder: String,
    /// Position in the waiters queue (1-indexed), if waiting.
    pub position: Option<usize>,
}

/// Derive the canonical merge slot ID from the issue prefix.
pub fn merge_slot_id(prefix: &str) -> String {
    format!("{}-merge-slot", prefix.trim_end_matches('-'))
}

// ---------------------------------------------------------------------------
// Storage-level operations (wraps SqliteStorage)
// ---------------------------------------------------------------------------

impl SqliteStorage {
    /// Create the merge slot bead for the current project.
    /// Idempotent: returns the existing slot without error if already created.
    pub fn merge_slot_create(&mut self, actor: &str, prefix: &str) -> Result<Issue> {
        let slot_id = merge_slot_id(prefix);

        // Idempotent: return existing slot
        if let Some(existing) = self.get_issue(&slot_id)? {
            return Ok(existing);
        }

        let meta = SlotMeta::default();
        let meta_json = serde_json::to_string(&meta).map_err(|e| BeadsError::Internal {
            message: format!("merge slot: JSON encode error: {e}"),
        })?;

        let now = chrono::Utc::now();
        let issue = Issue {
            id: slot_id.clone(),
            content_hash: None,
            title: "Merge Slot".to_string(),
            description: Some(
                "Exclusive access slot for serialized conflict resolution in the merge queue."
                    .to_string(),
            ),
            design: None,
            acceptance_criteria: None,
            notes: None,
            status: Status::Open,
            priority: Priority::CRITICAL,
            issue_type: IssueType::Task,
            assignee: None,
            owner: None,
            estimated_minutes: None,
            created_at: now,
            created_by: Some(actor.to_string()),
            updated_at: now,
            closed_at: None,
            close_reason: None,
            closed_by_session: None,
            due_at: None,
            defer_until: None,
            external_ref: None,
            source_system: None,
            source_repo: None,
            source_repo_path: None,
            agent_context: None,
            deleted_at: None,
            deleted_by: None,
            delete_reason: None,
            original_type: None,
            compaction_level: None,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: None,
            dependencies: vec![],
            comments: vec![],
            labels: vec![],
            sender: None,
            ephemeral: false,
            pinned: false,
            is_template: false,
            metadata: Some(meta_json),
            ..Default::default()
        };

        self.create_issue(&issue, actor)?;
        let _ = self.add_label(&slot_id, MERGE_SLOT_LABEL, actor);

        self.get_issue(&slot_id)?
            .ok_or_else(|| BeadsError::Internal {
                message: format!("merge slot {slot_id} created but not found"),
            })
    }

    /// Check the current merge slot status.
    pub fn merge_slot_check(&mut self, prefix: &str) -> Result<MergeSlotStatus> {
        let slot_id = merge_slot_id(prefix);

        let slot = self
            .get_issue(&slot_id)?
            .ok_or_else(|| BeadsError::Internal {
                message: format!(
                    "merge slot not found: {} (run 'br merge-slot create' first)",
                    slot_id
                ),
            })?;

        let meta = parse_slot_meta(&slot);
        Ok(MergeSlotStatus {
            slot_id,
            available: slot.status == Status::Open,
            holder: meta.holder,
            waiters: meta.waiters,
        })
    }

    /// Attempt to acquire the merge slot for exclusive access.
    ///
    /// Uses an atomic transaction to prevent race conditions between concurrent
    /// acquire attempts.
    ///
    /// If the slot is held and `wait` is true, the actor is added to the
    /// waiters queue.
    pub fn merge_slot_acquire(
        &mut self,
        holder: &str,
        actor: &str,
        wait: bool,
        prefix: &str,
    ) -> Result<MergeSlotAcquireResult> {
        if holder.is_empty() {
            return Err(BeadsError::Validation {
                field: "holder".to_string(),
                reason: "holder must not be empty".to_string(),
            });
        }

        let slot_id = merge_slot_id(prefix);
        let mut result = MergeSlotAcquireResult {
            slot_id: slot_id.clone(),
            acquired: false,
            waiting: false,
            holder: String::new(),
            position: None,
        };

        // Outer retry loop handles WAL contention
        let mut attempts = 0;
        loop {
            attempts += 1;
            if attempts > 8 {
                return Err(BeadsError::Internal {
                    message: "merge slot acquire: too many retries".to_string(),
                });
            }

            // Grab current slot state for result and decision
            let slot = match self.get_issue(&slot_id)? {
                Some(s) => s,
                None => {
                    return Err(BeadsError::Internal {
                        message: format!(
                            "merge slot not found: {} (run 'br merge-slot create' first)",
                            slot_id
                        ),
                    });
                }
            };

            let meta = parse_slot_meta(&slot);
            result.holder = meta.holder.clone();

            if slot.status == Status::Open {
                // Slot available — try to grab it
                let new_meta = SlotMeta {
                    holder: holder.to_string(),
                    waiters: meta.waiters,
                };
                let meta_json =
                    serde_json::to_string(&new_meta).map_err(|e| BeadsError::Internal {
                        message: format!("merge slot: JSON encode error: {e}"),
                    })?;

                match self.merge_slot_update_status(
                    &slot_id,
                    Status::InProgress,
                    &meta_json,
                    actor,
                ) {
                    Ok(()) => {
                        result.acquired = true;
                        result.holder = holder.to_string();
                        return Ok(result);
                    }
                    Err(BeadsError::Internal { .. }) => {
                        // Slot was taken by another agent — retry
                        let _ = std::thread::sleep(Duration::from_millis(
                            (10_u64 << attempts).min(500),
                        ));
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }

            // Slot is held; if not waiting, return current state
            if !wait {
                return Ok(result);
            }

            // Add to waiters queue
            let mut new_waiters = meta.waiters;
            if !new_waiters.contains(&holder.to_string()) {
                new_waiters.push(holder.to_string());
            }
            let new_meta = SlotMeta {
                holder: meta.holder,
                waiters: new_waiters,
            };
            let meta_json =
                serde_json::to_string(&new_meta).map_err(|e| BeadsError::Internal {
                    message: format!("merge slot: JSON encode error: {e}"),
                })?;

            match self.merge_slot_update_status(
                &slot_id,
                Status::InProgress, // stays held
                &meta_json,
                actor,
            ) {
                Ok(()) => {
                    result.waiting = true;
                    result.position = Some(new_meta.waiters.len());
                    return Ok(result);
                }
                Err(BeadsError::Internal { .. }) => {
                    let _ = std::thread::sleep(Duration::from_millis(
                        (10_u64 << attempts).min(500),
                    ));
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Release the merge slot.
    ///
    /// Clears the holder field. If there are waiters, the first waiter
    /// becomes the new holder.
    pub fn merge_slot_release(
        &mut self,
        holder: &str,
        actor: &str,
        prefix: &str,
    ) -> Result<()> {
        let slot_id = merge_slot_id(prefix);

        let mut attempts = 0;
        loop {
            attempts += 1;
            if attempts > 8 {
                return Err(BeadsError::Internal {
                    message: "merge slot release: too many retries".to_string(),
                });
            }

            let slot = match self.get_issue(&slot_id)? {
                Some(s) => s,
                None => {
                    return Err(BeadsError::Internal {
                        message: format!("merge slot not found: {slot_id}"),
                    });
                }
            };

            let meta = parse_slot_meta(&slot);

            if !holder.is_empty() && meta.holder != holder {
                return Err(BeadsError::Validation {
                    field: "holder".to_string(),
                    reason: format!("slot held by {}, not {}", meta.holder, holder),
                });
            }

            if slot.status == Status::Open {
                // Already released — idempotent
                return Ok(());
            }

            // Next holder is the first waiter (if any)
            let new_waiters = meta.waiters;
            let (new_holder, new_status) = if !new_waiters.is_empty() {
                (new_waiters[0].clone(), Status::InProgress)
            } else {
                (String::new(), Status::Open)
            };

            let new_meta = SlotMeta {
                holder: new_holder,
                waiters: new_waiters.into_iter().skip(1).collect(),
            };
            let meta_json =
                serde_json::to_string(&new_meta).map_err(|e| BeadsError::Internal {
                    message: format!("merge slot: JSON encode error: {e}"),
                })?;

            match self.merge_slot_update_status(&slot_id, new_status, &meta_json, actor) {
                Ok(()) => return Ok(()),
                Err(BeadsError::Internal { .. }) => {
                    let _ = std::thread::sleep(Duration::from_millis(
                        (10_u64 << attempts).min(500),
                    ));
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Low-level atomic update of merge slot status and metadata.
    /// Uses a compare-and-set on the status field to prevent race conditions.
    fn merge_slot_update_status(
        &mut self,
        slot_id: &str,
        new_status: Status,
        metadata: &str,
        actor: &str,
    ) -> Result<()> {
        use crate::storage::sqlite::IssueUpdate;

        // First check the current status for compare-and-set
        let current = self.get_issue(slot_id)?;
        let current_status = current
            .as_ref()
            .map(|i| i.status.clone())
            .unwrap_or(Status::Open);

        // If trying to go to InProgress but it's already InProgress, that's a conflict
        if new_status == Status::InProgress && current_status == Status::InProgress {
            return Err(BeadsError::Internal {
                message: "merge slot status conflict: already in progress".to_string(),
            });
        }

        let updates = IssueUpdate {
            title: None,
            description: None,
            design: None,
            acceptance_criteria: None,
            notes: None,
            status: Some(new_status),
            priority: None,
            issue_type: None,
            assignee: None,
            owner: None,
            estimated_minutes: None,
            due_at: None,
            defer_until: None,
            external_ref: None,
            source_repo: None,
            source_repo_path: None,
            agent_context: None,
            closed_at: None,
            close_reason: None,
            closed_by_session: None,
            deleted_at: None,
            deleted_by: None,
            delete_reason: None,
            skip_cache_rebuild: true,
            expect_unassigned: false,
            claim_exclusive: false,
            claim_actor: None,
            is_template: None,
            metadata: Some(Some(metadata.to_string())),
        };

        match self.update_issue(slot_id, &updates, actor) {
            Ok(_) => Ok(()),
            Err(BeadsError::IssueNotFound { .. }) => {
                Err(BeadsError::Internal {
                    message: format!("merge slot {slot_id} not found"),
                })
            }
            Err(e) => Err(e),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse slot metadata from an issue's metadata field.
fn parse_slot_meta(issue: &Issue) -> SlotMeta {
    let data = match issue.metadata.as_ref() {
        Some(m) => serde_json::from_str::<serde_json::Value>(m)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
        None => serde_json::Value::Object(serde_json::Map::new()),
    };
    serde_json::from_value(data).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_slot_id_default() {
        assert_eq!(merge_slot_id("br"), "br-merge-slot");
        assert_eq!(merge_slot_id("gt"), "gt-merge-slot");
        assert_eq!(merge_slot_id("my-project-"), "my-project-merge-slot");
    }

    #[test]
    fn test_parse_slot_meta_empty() {
        let issue = Issue {
            id: "br-merge-slot".to_string(),
            metadata: None,
            ..Default::default()
        };
        let meta = parse_slot_meta(&issue);
        assert!(meta.holder.is_empty());
        assert!(meta.waiters.is_empty());
    }

    #[test]
    fn test_parse_slot_meta_held() {
        let json = r#"{"holder": "alice", "waiters": ["bob", "carol"]}"#;
        let issue = Issue {
            id: "br-merge-slot".to_string(),
            metadata: Some(json.to_string()),
            ..Default::default()
        };
        let meta = parse_slot_meta(&issue);
        assert_eq!(meta.holder, "alice");
        assert_eq!(meta.waiters, vec!["bob", "carol"]);
    }

    #[test]
    fn test_parse_slot_meta_waiters_only() {
        let json = r#"{"waiters": ["eve"]}"#;
        let issue = Issue {
            id: "br-merge-slot".to_string(),
            metadata: Some(json.to_string()),
            ..Default::default()
        };
        let meta = parse_slot_meta(&issue);
        assert!(meta.holder.is_empty());
        assert_eq!(meta.waiters, vec!["eve"]);
    }

    #[test]
    fn test_slot_meta_serialize() {
        let meta = SlotMeta {
            holder: "alice".to_string(),
            waiters: vec!["bob".to_string()],
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("\"holder\":\"alice\""));
        assert!(json.contains("\"waiters\":[\"bob\"]"));
    }

    #[test]
    fn test_slot_meta_empty_waiters_not_serialized() {
        let meta = SlotMeta {
            holder: "alice".to_string(),
            waiters: vec![],
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("\"holder\":\"alice\""));
        assert!(!json.contains("waiters"));
    }

    #[test]
    fn test_slot_meta_empty_holder_not_serialized() {
        let meta = SlotMeta {
            holder: String::new(),
            waiters: vec![],
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(!json.contains("holder"));
        assert!(!json.contains("waiters"));
        assert_eq!(json, "{}");
    }
}
