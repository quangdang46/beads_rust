//! `SQLite` storage layer for `beads_rust`.
//!
//! This module provides the persistence layer using `SQLite` with:
//! - WAL mode for concurrent reads
//! - Transaction discipline for atomic writes
//! - Dirty tracking for JSONL export
//! - Blocked cache for ready/blocked queries
//!
//! # Submodules
//!
//! - [`events`] - Audit event storage (insertion, retrieval)
//! - [`schema`] - Database schema definitions
//! - [`sqlite`] - Main `SQLite` storage implementation

/// TEMPORARY, deleted in Phase 8 with the rest of the engine migration. See this module's
/// docs for the deletion gate.
pub mod db;
pub mod events;
pub mod schema;
pub mod sqlite;
pub mod trait_;

pub mod hooks;

pub use sqlite::{
    ChangelogIssueRow, CloseMetadataRow, EventAttribution, IssueUpdate, ListFilters, ReadyFilters,
    ReadySortPolicy, SqliteStorage, StatsIssueRow,
};
