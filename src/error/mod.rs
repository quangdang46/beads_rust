//! Error types and handling for `beads_rust`.
//!
//! This module provides structured errors that match the classic bd
//! behavior for JSON error output compatibility.
//!
//! # Design
//!
//! - Uses `thiserror` for derive-based error types
//! - Provides recovery hints for user-facing errors
//! - Matches bd's exit code conventions
//! - Provides structured JSON output for AI coding agents

mod context;
mod structured;

pub use context::{OptionExt, ResultExt};
pub use structured::{ErrorCode, StructuredError};

use std::path::PathBuf;
use thiserror::Error;

/// Primary error type for `beads_rust` operations.
///
/// Design: Structured variants for common cases.
#[derive(Error, Debug)]
pub enum BeadsError {
    // === Storage Errors ===
    /// Database file not found at the specified path.
    #[error("Database not found at '{path}'")]
    DatabaseNotFound { path: PathBuf },

    /// Database is locked by another process.
    #[error("Database is locked: {path}")]
    DatabaseLocked { path: PathBuf },

    /// Database schema version doesn't match expected.
    #[error("Schema version mismatch: expected {expected}, found {found}")]
    SchemaMismatch { expected: i32, found: i32 },

    /// `SQLite` database error from the C engine (rusqlite).
    ///
    /// This is the payload the whole codebase is migrating to. Unlike the
    /// frankensqlite error it replaces, this type exposes the **result code** and the
    /// **extended result code** as data rather than burying them in a message string, so
    /// retry decisions can be made on the code alone. Never string-match the rendered
    /// message to decide anything: for `SqliteFailure(_, Some(msg))` the `Display` impl
    /// prints the message and *nothing else*, so the code is not even present in the text.
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// `SQLite` database error from the legacy pure-Rust engine (frankensqlite).
    ///
    /// TEMPORARY, deleted in Phase 8. It exists only because the two engines coexist on the
    /// migration branch: `src/storage/sqlite.rs` keeps producing frankensqlite errors until
    /// Phase 7, so removing this arm in Phase 2 would break 15+ `From` conversions in that
    /// file and leave the tree uncompilable for six consecutive phases. This is the same
    /// category of thing as `src/storage/db.rs`: a branch-only scaffold with a named deletion
    /// point, never a shipped compatibility shim. **If this variant still exists when Phase 8
    /// opens, Phase 8 does not merge.**
    #[error("Database error: {0}")]
    DatabaseLegacy(#[from] fsqlite_error::FrankenError),

    // === Issue Errors ===
    /// Issue with the specified ID was not found.
    #[error("Issue not found: {id}")]
    IssueNotFound { id: String },

    /// Attempted to create an issue with an ID that already exists.
    #[error("Issue ID collision: {id}")]
    IdCollision { id: String },

    /// Partial ID matches multiple issues.
    #[error("Ambiguous ID '{partial}': matches {matches:?}")]
    AmbiguousId {
        partial: String,
        matches: Vec<String>,
    },

    /// Issue ID format is invalid.
    #[error("Invalid issue ID format: {id}")]
    InvalidId { id: String },

    // === Validation Errors ===
    /// Field validation failed.
    #[error("Validation failed: {field}: {reason}")]
    Validation { field: String, reason: String },

    /// Multiple validation errors occurred.
    #[error("Validation errors: {errors:?}")]
    ValidationErrors { errors: Vec<ValidationError> },

    /// Invalid status value.
    #[error("Invalid status: {status}")]
    InvalidStatus { status: String },

    /// Invalid issue type value.
    #[error("Invalid issue type: {issue_type}")]
    InvalidType { issue_type: String },

    /// Priority out of valid range (0-4).
    #[error("Priority must be 0-4, got: {priority}")]
    InvalidPriority { priority: String },

    // === JSONL Errors ===
    /// Failed to parse a line in the JSONL file.
    #[error("JSONL parse error at line {line}: {reason}")]
    JsonlParse { line: usize, reason: String },

    /// Issue prefix doesn't match expected prefix.
    #[error("Prefix mismatch: expected '{expected}', found '{found}'")]
    PrefixMismatch { expected: String, found: String },

    /// Import found conflicting issues.
    #[error("Import collision: {count} issues have conflicting content")]
    ImportCollision { count: usize },

    /// Conflict detected between local and external changes.
    #[error("Sync conflict: {message}")]
    SyncConflict { message: String },

    // === Dependency Errors ===
    /// Adding the dependency would create a cycle.
    #[error("Cycle detected in dependencies: {path}")]
    DependencyCycle { path: String },

    /// Cannot delete an issue that has dependents.
    #[error("Cannot delete: {id} has {count} dependents")]
    HasDependents { id: String, count: usize },

    /// Self-referential dependency.
    #[error("Issue cannot depend on itself: {id}")]
    SelfDependency { id: String },

    /// Dependency target not found.
    #[error("Dependency target not found: {id}")]
    DependencyNotFound { id: String },

    /// Duplicate dependency.
    #[error("Dependency already exists: {from} -> {to}")]
    DuplicateDependency { from: String, to: String },

    // === Schema Skew Errors ===
    /// Database schema version is ahead of the binary's known version
    /// (forward drift). The binary is too old to safely read this DB.
    #[error(
        "Schema skew: database is at v{db_version}, binary knows up to v{binary_version}. Rebuild or set BR_IGNORE_SCHEMA_SKEW=1 to proceed (some queries may fail)."
    )]
    SchemaSkewForward {
        db_version: i32,
        binary_version: i32,
    },
    /// Database schema version is behind the binary's known version and
    /// the connection is read-only (cannot migrate). Run a write command
    /// in this workspace to migrate.
    #[error(
        "Schema skew: database is at v{db_version}, binary expects v{binary_version}, and the read-only open cannot migrate it. Run a write command in this workspace, or set BR_IGNORE_SCHEMA_SKEW=1 to proceed (some queries may fail)."
    )]
    SchemaSkewBehind {
        db_version: i32,
        binary_version: i32,
    },

    // === Configuration Errors ===
    /// Configuration file error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// External command failed or returned unusable output.
    #[error("External command failed: {command}: {reason}")]
    ExternalCommand { command: String, reason: String },

    /// Internal consistency check failed.
    #[error("Internal error: {message}")]
    Internal { message: String },

    /// Beads workspace not initialized.
    #[error("Beads not initialized: run 'br init' first")]
    NotInitialized,

    /// Already initialized.
    #[error("Already initialized at '{path}'")]
    AlreadyInitialized { path: PathBuf },

    // === I/O Errors ===
    /// File system I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// YAML parsing error.
    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yml::Error),

    // === Wrapped errors (for gradual migration) ===
    /// Error with additional context.
    #[error("{context}: {source}")]
    WithContext {
        context: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    // === Operational Errors ===
    /// All requested items were skipped (already closed, not found, etc.).
    #[error("Nothing to do: {reason}")]
    NothingToDo { reason: String },

    // === Policy Errors ===
    /// One or more closure-time policy gates fired.
    ///
    /// Display format intentionally repeats the gate that fired and a
    /// short explanation so terminal output stays readable; structured
    /// callers should serialise the inner [`crate::close_policy::PolicyViolation`]s
    /// via [`StructuredError::context`].
    #[error("Policy violation closing {issue_id}: {summary}")]
    PolicyViolation {
        issue_id: String,
        summary: String,
        violations: Vec<crate::close_policy::PolicyViolation>,
    },
}

impl BeadsError {
    /// True when this is a database error from *either* engine.
    ///
    /// Phase 8 deletes [`Self::DatabaseLegacy`], after which this collapses to a single
    /// variant. Call sites that mean "is this a database error" should use this rather than
    /// naming a variant: writing `matches!(e, BeadsError::Database(_))` matches only the C
    /// engine today, and silently stops firing for frankensqlite errors with no compile error
    /// and no failing test.
    #[must_use]
    pub fn is_database_error(&self) -> bool {
        matches!(self, Self::Database(_) | Self::DatabaseLegacy(_))
    }

    /// Returns true if the error is transient and can be retried.
    ///
    /// # What counts as transient, and why
    ///
    /// Only two SQLite result codes mean "another actor holds a lock, try again":
    /// `SQLITE_BUSY` and `SQLITE_LOCKED`. Everything else that is not a
    /// [`std::io::Error`] is either a permanent condition or a data-path failure, and
    /// retrying it just burns the caller's backoff budget before returning the same answer.
    ///
    /// Three calls that are easy to get wrong, decided explicitly:
    ///
    /// * **`SQLITE_IOERR` is NOT transient.** It is tempting to treat it as
    ///   "the disk was busy", but that is backwards. `sqliteErrorFromPosixError` maps
    ///   `EACCES`, `EAGAIN`, `ETIMEDOUT`, `EBUSY`, `EINTR` and `ENOLCK` to `SQLITE_BUSY`,
    ///   so NFS/SMB lock contention already arrives here as `SQLITE_BUSY` and is covered.
    ///   What actually produces `SQLITE_IOERR` is the *data* path failing: `unixRead` /
    ///   `unixWrite` returning `EIO`/`ENOSPC`, a short read, or a fault during page I/O.
    ///   Retrying a full disk is a hot loop against a disk that is not going to recover.
    ///
    /// * **`SQLITE_PROTOCOL` is NOT transient.** The database is locked by a process using
    ///   an incompatible locking protocol. That is a stable environmental fact, not a race.
    ///   Classifying it as transient converts an immediate, accurate error into a ~12.7s
    ///   stall in the 8-attempt exponential loops and then surfaces the same error.
    ///
    /// * **`SQLITE_BUSY_SNAPSHOT` is NOT transient even though its primary code is
    ///   `SQLITE_BUSY`.** SQLite documents that this specific case is *not* resolved by
    ///   waiting, because the other connection's snapshot is stale for good. Masking the
    ///   extended code down to `0xff` would classify it as retryable and spin.
    ///
    /// The [`Self::Io`] arm is deliberately preserved. Deleting it would silently change
    /// retry behaviour for non-database I/O — `Interrupted`, `TimedOut` and `WouldBlock`
    /// are transient for reasons that have nothing to do with SQLite.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Database(e) => Self::sqlite_error_is_transient(e),
            // Phase 8 deletes this arm along with the variant. Until then the legacy engine
            // still produces these, and the retry loops in `storage/sqlite.rs` still gate on
            // them, so dropping it now would stop those loops from ever retrying.
            Self::DatabaseLegacy(e) => e.is_transient(),
            Self::Io(e) => {
                matches!(
                    e.kind(),
                    std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::WouldBlock
                )
            }
            _ => false,
        }
    }

    /// Classify a `rusqlite` error as retryable, on the result code alone.
    ///
    /// Never string-matches the rendered message. For `SqliteFailure(_, Some(msg))` the
    /// `Display` impl prints `msg` and nothing else, so the result code is not present in
    /// the text at all.
    #[must_use]
    fn sqlite_error_is_transient(err: &rusqlite::Error) -> bool {
        // A BUSY that is a snapshot conflict needs the extended code to tell it apart.
        if err.sqlite_extended_error_code() == Some(rusqlite::ffi::SQLITE_BUSY_SNAPSHOT as i32) {
            return false;
        }
        matches!(
            err.sqlite_error_code(),
            Some(
                rusqlite::ffi::ErrorCode::DatabaseBusy | rusqlite::ffi::ErrorCode::DatabaseLocked
            )
        )
    }
}

/// A single field validation error.
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// The field that failed validation.
    pub field: String,
    /// The reason for the validation failure.
    pub message: String,
}

impl ValidationError {
    /// Create a new validation error.
    #[must_use]
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for ValidationError {}

impl BeadsError {
    /// Can the user fix this without code changes?
    #[must_use]
    pub const fn is_user_recoverable(&self) -> bool {
        matches!(
            self,
            Self::DatabaseNotFound { .. }
                | Self::NotInitialized
                | Self::IssueNotFound { .. }
                | Self::Validation { .. }
                | Self::InvalidStatus { .. }
                | Self::InvalidType { .. }
                | Self::InvalidPriority { .. }
                | Self::PrefixMismatch { .. }
                | Self::AmbiguousId { .. }
                | Self::PolicyViolation { .. }
        )
    }

    /// Should we suggest re-running with --force?
    #[must_use]
    pub const fn suggests_force(&self) -> bool {
        matches!(
            self,
            Self::HasDependents { .. }
                | Self::ImportCollision { .. }
                | Self::AlreadyInitialized { .. }
        )
    }

    /// Human-friendly suggestion for fixing this error.
    #[must_use]
    pub const fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::NotInitialized => Some("Run: br init"),
            Self::DatabaseNotFound { .. } => Some("Check path or run: br init"),
            Self::AmbiguousId { .. } => Some("Provide more characters of the ID"),
            Self::HasDependents { .. } => Some("Use --force or --cascade to delete anyway"),
            Self::ImportCollision { .. } => Some("Use --force to overwrite or resolve manually"),
            Self::DependencyCycle { .. } => Some("Remove one dependency to break the cycle"),
            Self::SelfDependency { .. } => Some("An issue cannot depend on itself"),
            Self::AlreadyInitialized { .. } => Some("Use --force to reinitialize"),
            Self::InvalidPriority { .. } => {
                Some("Use a priority between 0 (critical) and 4 (backlog)")
            }
            Self::InvalidStatus { .. } => Some(
                "Valid statuses: open, in_progress, blocked, deferred, draft, closed, tombstone, pinned",
            ),
            Self::InvalidType { .. } => {
                Some("Valid types: task, bug, feature, epic, chore, docs, question")
            }
            Self::PolicyViolation { .. } => Some(
                "Fix the violation(s) above, or pass --bypass-policy --bypass-reason \"<text>\" if your project's policy.yaml allows bypass.",
            ),
            _ => None,
        }
    }

    /// Get the exit code for this error.
    ///
    /// Delegates to [`ErrorCode::exit_code()`] via [`StructuredError`] for
    /// consistent, categorized exit codes (1–8).
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        StructuredError::from_error(self).code.exit_code()
    }

    /// Create a validation error for a specific field.
    #[must_use]
    pub fn validation(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Validation {
            field: field.into(),
            reason: reason.into(),
        }
    }

    /// Create an external command failure.
    #[must_use]
    pub fn external_command(command: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::ExternalCommand {
            command: command.into(),
            reason: reason.into(),
        }
    }

    /// Create an internal consistency error.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Create from multiple validation errors.
    #[must_use]
    pub fn from_validation_errors(errors: Vec<ValidationError>) -> Self {
        if errors.is_empty() {
            Self::ValidationErrors { errors }
        } else if errors.len() == 1 {
            let err = &errors[0];
            Self::Validation {
                field: err.field.clone(),
                reason: err.message.clone(),
            }
        } else {
            Self::ValidationErrors { errors }
        }
    }
}

/// Result type using `BeadsError`.
pub type Result<T> = std::result::Result<T, BeadsError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = BeadsError::IssueNotFound {
            id: "bd-abc123".to_string(),
        };
        assert_eq!(err.to_string(), "Issue not found: bd-abc123");
    }

    #[test]
    fn test_validation_error() {
        let err = BeadsError::validation("title", "cannot be empty");
        assert_eq!(err.to_string(), "Validation failed: title: cannot be empty");
    }

    #[test]
    fn test_external_command_uses_io_error_code() {
        let err = BeadsError::external_command("git", "failed to resolve ref");
        let structured = StructuredError::from_error(&err);

        assert_eq!(structured.code, ErrorCode::IoError);
        assert_eq!(err.exit_code(), 8);
    }

    #[test]
    fn test_internal_uses_internal_error_code() {
        let err = BeadsError::internal("routed command produced mismatched counts");
        let structured = StructuredError::from_error(&err);

        assert_eq!(structured.code, ErrorCode::InternalError);
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn test_user_recoverable() {
        let recoverable = BeadsError::NotInitialized;
        assert!(recoverable.is_user_recoverable());

        // Only the constructor changed in this test: the payload type is now the C engine's
        // error rather than frankensqlite's. The assertion below is byte-identical, which is
        // what the Phase 2 "zero test edits" criterion is actually about.
        let not_recoverable = BeadsError::Database(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR as i32),
            Some("test".to_string()),
        ));
        assert!(!not_recoverable.is_user_recoverable());
    }

    /// Table-driven check of the decided transient table, including the extended codes
    /// that a primary-code-only classifier gets wrong.
    ///
    /// The expected value is the *decision*, not something derived from the classifier, so
    /// this test fails if the implementation drifts rather than restating it back at us.
    #[test]
    fn sqlite_transience_table() {
        use rusqlite::ffi;

        let cases: &[(ffi::ErrorCode, bool)] = &[
            // Lock contention: the only genuinely retryable pair.
            (ffi::ErrorCode::DatabaseBusy, true),
            (ffi::ErrorCode::DatabaseLocked, true),
            // Permanent conditions. Retrying returns the same answer.
            (ffi::ErrorCode::DatabaseCorrupt, false),
            (ffi::ErrorCode::NotADatabase, false),
            (ffi::ErrorCode::ReadOnly, false),
            (ffi::ErrorCode::ConstraintViolation, false),
            // Data-path failure. `sqliteErrorFromPosixError` routes lock contention to
            // SQLITE_BUSY, so IOERR here means the read/write path failed, not "busy".
            (ffi::ErrorCode::SystemIoFailure, false),
            // A stable environmental fact, not a race. Retry would stall ~12.7s.
            (ffi::ErrorCode::FileLockingProtocolFailed, false),
            (ffi::ErrorCode::OperationInterrupted, false),
            (ffi::ErrorCode::CannotOpen, false),
            (ffi::ErrorCode::DiskFull, false),
            (ffi::ErrorCode::OutOfMemory, false),
            (ffi::ErrorCode::SchemaChanged, false),
        ];

        for (code, expect_transient) in cases {
            let raw = sqlite_raw_code_for(*code);
            let err = BeadsError::Database(rusqlite::Error::SqliteFailure(
                ffi::Error::new(raw),
                Some("synthetic".to_string()),
            ));
            assert_eq!(
                err.is_transient(),
                *expect_transient,
                "transience for {code:?} (raw {raw}) disagreed with the documented table"
            );
        }
    }

    /// `SQLITE_BUSY_SNAPSHOT` has a primary code of `SQLITE_BUSY` but is documented as *not*
    /// resolvable by waiting, so masking the extended code would misclassify it.
    #[test]
    fn busy_snapshot_is_not_transient() {
        let err = BeadsError::Database(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY_SNAPSHOT as i32),
            Some("database is locked by another connection".to_string()),
        ));
        assert!(
            !err.is_transient(),
            "BUSY_SNAPSHOT must not be retried; waiting does not clear a stale snapshot"
        );
    }

    /// The legacy arm must keep working until Phase 8 deletes it. If it silently stopped
    /// returning true for busy, the retry loops still running on frankensqlite would stop
    /// retrying without any compile error.
    #[test]
    fn legacy_engine_arm_still_classifies() {
        use fsqlite_error::FrankenError;
        for (err, expect) in [
            (FrankenError::Busy, true),
            (FrankenError::DatabaseLocked { path: PathBuf::from("x") }, true),
            (FrankenError::Internal("x".to_string()), false),
        ] {
            let wrapped = BeadsError::DatabaseLegacy(err);
            assert_eq!(
                wrapped.is_transient(),
                expect,
                "legacy arm changed behaviour; the frankensqlite retry loops depend on it"
            );
        }
    }

    /// `Self::Io` must stay transient for the three kinds that are transient for reasons
    /// unrelated to SQLite. Deleting that arm would be a silent behaviour change.
    #[test]
    fn io_arm_preserved() {
        for kind in [
            std::io::ErrorKind::Interrupted,
            std::io::ErrorKind::TimedOut,
            std::io::ErrorKind::WouldBlock,
        ] {
            let err = BeadsError::Io(std::io::Error::new(kind, "x"));
            assert!(err.is_transient(), "{kind:?} must remain transient");
        }
        let other = BeadsError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "x"));
        assert!(!other.is_transient());
    }

    fn sqlite_raw_code_for(code: rusqlite::ffi::ErrorCode) -> i32 {
        use rusqlite::ffi;
        let raw = match code {
            ffi::ErrorCode::DatabaseBusy => ffi::SQLITE_BUSY,
            ffi::ErrorCode::DatabaseLocked => ffi::SQLITE_LOCKED,
            ffi::ErrorCode::DatabaseCorrupt => ffi::SQLITE_CORRUPT,
            ffi::ErrorCode::NotADatabase => ffi::SQLITE_NOTADB,
            ffi::ErrorCode::ReadOnly => ffi::SQLITE_READONLY,
            ffi::ErrorCode::ConstraintViolation => ffi::SQLITE_CONSTRAINT,
            ffi::ErrorCode::SystemIoFailure => ffi::SQLITE_IOERR,
            ffi::ErrorCode::FileLockingProtocolFailed => ffi::SQLITE_PROTOCOL,
            ffi::ErrorCode::OperationInterrupted => ffi::SQLITE_INTERRUPT,
            ffi::ErrorCode::CannotOpen => ffi::SQLITE_CANTOPEN,
            ffi::ErrorCode::DiskFull => ffi::SQLITE_FULL,
            ffi::ErrorCode::OutOfMemory => ffi::SQLITE_NOMEM,
            ffi::ErrorCode::SchemaChanged => ffi::SQLITE_SCHEMA,
            _ => panic!("no raw code mapped for {code:?}; add it rather than skipping"),
        };
        raw as i32
    }

    #[test]
    fn test_suggestion() {        let err = BeadsError::NotInitialized;
        assert_eq!(err.suggestion(), Some("Run: br init"));

        let err = BeadsError::AmbiguousId {
            partial: "bd-a".to_string(),
            matches: vec!["bd-abc".to_string(), "bd-abd".to_string()],
        };
        assert_eq!(err.suggestion(), Some("Provide more characters of the ID"));

        let err = BeadsError::InvalidStatus {
            status: "dra".to_string(),
        };
        assert_eq!(
            err.suggestion(),
            Some(
                "Valid statuses: open, in_progress, blocked, deferred, draft, closed, tombstone, pinned",
            )
        );
    }

    #[test]
    fn test_validation_error_struct() {
        let err = ValidationError::new("priority", "must be 0-4");
        assert_eq!(err.to_string(), "priority: must be 0-4");
    }
}
