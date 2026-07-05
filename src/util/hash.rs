//! Content hashing for issue deduplication and sync.
//!
//! Uses SHA256 over stable ordered fields with null separators.
//! Matches Go `bd` `ComputeContentHash` (`internal/types/types.go`)
//! byte-for-byte for cross-tool deduplication.

use sha2::{Digest, Sha256};

use crate::model::{BondRef, Issue, IssueType, MolType, Priority, Status, WorkType};

/// Lowercase hex encoding for digest outputs (sha2 0.11 no longer impls `LowerHex`
/// on `Array<u8, _>`, so we format bytes directly).
#[must_use]
pub fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(&mut s, "{b:02x}").expect("writing to String never fails");
    }
    s
}

/// Trait for types that can produce a deterministic content hash.
pub trait ContentHashable {
    /// Compute the content hash for this value.
    fn content_hash(&self) -> String;
}

impl ContentHashable for Issue {
    fn content_hash(&self) -> String {
        content_hash(self)
    }
}

/// Compute SHA256 content hash for an issue, matching Go `bd` field order.
///
/// Fields included in order (each followed by a null separator):
/// 1.  Title
/// 2.  Description
/// 3.  Design
/// 4.  AcceptanceCriteria
/// 5.  Notes
/// 6.  SpecID
/// 7.  Status (string)
/// 8.  Priority (int, formatted as decimal)
/// 9.  IssueType (string)
/// 10. Assignee
/// 11. Owner
/// 12. CreatedBy
/// 13. ExternalRef (strPtr — nil writes just the null byte)
/// 14. SourceSystem
/// 15. Pinned (flag: label "pinned" when true, empty string when false)
/// 16. Metadata (raw JSON string)
/// 17. IsTemplate (flag: label "template" when true, empty string when false)
/// 18. BondedFrom (for each BondRef: SourceID, BondType, BondPoint)
/// 19. AwaitType
/// 20. AwaitID
/// 21. Timeout (duration in nanoseconds, formatted as decimal)
/// 22. Waiters (for each waiter: string + null byte)
/// 23. MolType (string)
/// 24. WorkType (string)
/// 25. EventKind
/// 26. Actor
/// 27. Target
/// 28. Payload
///
/// Fields excluded:
/// - id, content_hash (circular)
/// - labels, dependencies, comments, events (separate entities)
/// - timestamps (created_at, updated_at, closed_at, started_at, etc.)
/// - tombstone fields (deleted_at, deleted_by, delete_reason)
/// - estimated_minutes, due_at, defer_until
/// - close_reason, closed_by_session
/// - quality_score, crystallizes
/// - holder, hook_bead, role_bead, agent_state, role_type, rig
#[must_use]
pub fn content_hash(issue: &Issue) -> String {
    content_hash_from_parts(
        &issue.title,
        issue.description.as_deref(),
        issue.design.as_deref(),
        issue.acceptance_criteria.as_deref(),
        issue.notes.as_deref(),
        issue.spec_id.as_deref(),
        &issue.status,
        &issue.priority,
        &issue.issue_type,
        issue.assignee.as_deref(),
        issue.owner.as_deref(),
        issue.created_by.as_deref(),
        issue.external_ref.as_deref(),
        issue.source_system.as_deref(),
        issue.pinned,
        issue.metadata.as_deref(),
        issue.is_template,
        &issue.bonded_from,
        issue.await_type.as_deref(),
        issue.await_id.as_deref(),
        issue.timeout_seconds,
        &issue.waiters,
        &issue.mol_type,
        &issue.work_type,
        issue.event_kind.as_deref(),
        None, // actor field not on Rust Issue (on Event struct)
        issue.target.as_deref(),
        issue.payload.as_deref(),
    )
}

/// Create a content hash from raw components (for import/validation with defaults for newer fields).
/// Convenience wrapper with the classic 15-param signature.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn content_hash_from_parts_v15(
    title: &str,
    description: Option<&str>,
    design: Option<&str>,
    acceptance_criteria: Option<&str>,
    notes: Option<&str>,
    status: &Status,
    priority: &Priority,
    issue_type: &IssueType,
    assignee: Option<&str>,
    owner: Option<&str>,
    created_by: Option<&str>,
    external_ref: Option<&str>,
    source_system: Option<&str>,
    pinned: bool,
    is_template: bool,
) -> String {
    content_hash_from_parts(
        title,
        description,
        design,
        acceptance_criteria,
        notes,
        None, // spec_id
        status,
        priority,
        issue_type,
        assignee,
        owner,
        created_by,
        external_ref,
        source_system,
        pinned,
        None, // metadata
        is_template,
        &[], // bonded_from
        None,
        None,
        None, // await_type, await_id, timeout_seconds
        &[],  // waiters
        &MolType::default(),
        &WorkType::default(),
        None,
        None,
        None,
        None, // event_kind, actor, target, payload
    )
}

/// Create a content hash from raw components (for import/validation).
/// Matches Go `ComputeContentHash` field order exactly.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn content_hash_from_parts(
    title: &str,
    description: Option<&str>,
    design: Option<&str>,
    acceptance_criteria: Option<&str>,
    notes: Option<&str>,
    spec_id: Option<&str>,
    status: &Status,
    priority: &Priority,
    issue_type: &IssueType,
    assignee: Option<&str>,
    owner: Option<&str>,
    created_by: Option<&str>,
    external_ref: Option<&str>,
    source_system: Option<&str>,
    pinned: bool,
    metadata: Option<&str>,
    is_template: bool,
    bonded_from: &[BondRef],
    await_type: Option<&str>,
    await_id: Option<&str>,
    timeout_seconds: Option<i64>,
    waiters: &[String],
    mol_type: &MolType,
    work_type: &WorkType,
    event_kind: Option<&str>,
    actor: Option<&str>,
    target: Option<&str>,
    payload: Option<&str>,
) -> String {
    let mut writer = HashFieldWriter::new();

    // ===== Core fields =====
    writer.field(title);
    writer.field_opt(description);
    writer.field_opt(design);
    writer.field_opt(acceptance_criteria);
    writer.field_opt(notes);
    writer.field_opt(spec_id); // Go: w.str(i.SpecID)
    writer.field(status.as_str());
    writer.field(&priority.0.to_string());
    writer.field(issue_type.as_str());
    writer.field_opt(assignee);
    writer.field_opt(owner);
    writer.field_opt(created_by);
    writer.field_strptr(external_ref); // Go: w.strPtr(i.ExternalRef)
    writer.field_opt(source_system);
    writer.field_flag(pinned, "pinned"); // Go: w.flag(i.Pinned, "pinned")
    writer.field_opt(metadata); // Go: w.str(string(i.Metadata))
    writer.field_flag(is_template, "template"); // Go: w.flag(i.IsTemplate, "template")

    // ===== BondedFrom (compound molecule lineage) =====
    // Go: for _, br := range i.BondedFrom { w.str(br.SourceID) ... }
    for br in bonded_from {
        writer.field(&br.source_id);
        writer.field(&br.bond_type);
        // BondPoint is Option<String> in Rust; Go stores it as plain string
        writer.field_opt(br.bond_point.as_deref());
    }

    // ===== Gate fields (async coordination) =====
    writer.field_opt(await_type); // Go: w.str(i.AwaitType)
    writer.field_opt(await_id); // Go: w.str(i.AwaitID)
    // Go: w.duration(i.Timeout) — duration is int64 nanoseconds
    writer.field(&timeout_to_nanos_string(timeout_seconds));
    // Go: for _, waiter := range i.Waiters { w.str(waiter) }
    for waiter in waiters {
        writer.field(waiter);
    }

    // ===== Molecule type =====
    writer.field(mol_type.as_str()); // Go: w.str(string(i.MolType))

    // ===== Work type =====
    writer.field(work_type.as_str()); // Go: w.str(string(i.WorkType))

    // ===== Event fields =====
    writer.field_opt(event_kind); // Go: w.str(i.EventKind)
    writer.field_opt(actor); // Go: w.str(i.Actor)
    writer.field_opt(target); // Go: w.str(i.Target)
    writer.field_opt(payload); // Go: w.str(i.Payload)

    writer.finalize()
}

/// Convert optional timeout_seconds (Rust) to a Go-compatible nanosecond string.
/// Go's `time.Duration` is nanoseconds. When the value is zero/NULL Go writes "0".
fn timeout_to_nanos_string(timeout_seconds: Option<i64>) -> String {
    match timeout_seconds {
        Some(secs) => {
            // Saturating mul to avoid overflow on absurdly large values
            let nanos = secs.saturating_mul(1_000_000_000);
            nanos.to_string()
        }
        None => "0".to_string(),
    }
}

struct HashFieldWriter {
    hasher: Sha256,
}

impl HashFieldWriter {
    fn new() -> Self {
        Self {
            hasher: Sha256::new(),
        }
    }

    fn field(&mut self, value: &str) {
        self.hasher.update(value.as_bytes());
        self.hasher.update(b"\x00");
    }

    fn field_opt(&mut self, value: Option<&str>) {
        self.field(value.unwrap_or(""));
    }

    /// Like `field_opt` but matches Go `strPtr`: write the pointer value (or
    /// nothing for nil/None) then always write the null separator.
    fn field_strptr(&mut self, value: Option<&str>) {
        if let Some(s) = value {
            self.hasher.update(s.as_bytes());
        }
        self.hasher.update(b"\x00");
    }

    fn field_flag(&mut self, value: bool, label: &str) {
        self.field(if value { label } else { "" });
    }

    fn finalize(self) -> String {
        hex_encode(&self.hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_issue() -> Issue {
        Issue {
            id: "bd-test123".to_string(),
            content_hash: None,
            title: "Test Issue".to_string(),
            description: Some("A test description".to_string()),
            status: Status::Open,
            priority: Priority::MEDIUM,
            issue_type: IssueType::Task,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            ..Default::default()
        }
    }

    #[test]
    fn test_content_hash_deterministic() {
        let issue = make_test_issue();
        let hash1 = content_hash(&issue);
        let hash2 = content_hash(&issue);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_content_hash_is_hex() {
        let issue = make_test_issue();
        let hash = content_hash(&issue);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hash.len(), 64); // SHA256 = 32 bytes = 64 hex chars
    }

    #[test]
    fn test_content_hash_changes_with_title() {
        let mut issue = make_test_issue();
        let hash1 = content_hash(&issue);

        issue.title = "Different Title".to_string();
        let hash2 = content_hash(&issue);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_content_hash_ignores_timestamps() {
        let mut issue = make_test_issue();
        let hash1 = content_hash(&issue);

        issue.updated_at = chrono::Utc::now();
        let hash2 = content_hash(&issue);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_content_hash_includes_pinned() {
        let mut issue = make_test_issue();
        let hash1 = content_hash(&issue);

        issue.pinned = true;
        let hash2 = content_hash(&issue);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_content_hash_includes_created_by() {
        let mut issue = make_test_issue();
        let hash1 = content_hash(&issue);

        issue.created_by = Some("tester@example.com".to_string());
        let hash2 = content_hash(&issue);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_content_hash_includes_source_system() {
        let mut issue = make_test_issue();
        let hash1 = content_hash(&issue);

        issue.source_system = Some("imported".to_string());
        let hash2 = content_hash(&issue);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_content_hash_from_parts() {
        let issue = make_test_issue();
        let direct = content_hash(&issue);
        let from_parts = content_hash_from_parts(
            &issue.title,
            issue.description.as_deref(),
            issue.design.as_deref(),
            issue.acceptance_criteria.as_deref(),
            issue.notes.as_deref(),
            issue.spec_id.as_deref(),
            &issue.status,
            &issue.priority,
            &issue.issue_type,
            issue.assignee.as_deref(),
            issue.owner.as_deref(),
            issue.created_by.as_deref(),
            issue.external_ref.as_deref(),
            issue.source_system.as_deref(),
            issue.pinned,
            issue.metadata.as_deref(),
            issue.is_template,
            &issue.bonded_from,
            issue.await_type.as_deref(),
            issue.await_id.as_deref(),
            issue.timeout_seconds,
            &issue.waiters,
            &issue.mol_type,
            &issue.work_type,
            issue.event_kind.as_deref(),
            None, // actor field not on Rust Issue (on Event struct)
            issue.target.as_deref(),
            issue.payload.as_deref(),
        );
        assert_eq!(direct, from_parts);
    }

    #[test]
    fn test_hex_encode_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn test_hex_encode_single_byte() {
        assert_eq!(hex_encode(&[0x00]), "00");
        assert_eq!(hex_encode(&[0x0a]), "0a");
        assert_eq!(hex_encode(&[0xff]), "ff");
        assert_eq!(hex_encode(&[0x80]), "80");
        assert_eq!(hex_encode(&[0x7f]), "7f");
        assert_eq!(hex_encode(&[0x01]), "01");
    }

    #[test]
    fn test_hex_encode_length_invariant() {
        for len in [0, 1, 2, 16, 32, 64, 128, 255] {
            let bytes: Vec<u8> = (0..len)
                .map(|i: u32| u8::try_from(i).unwrap_or(0))
                .collect();
            let hex = hex_encode(&bytes);
            assert_eq!(
                hex.len(),
                bytes.len() * 2,
                "hex_encode output length should be 2*input for {} bytes",
                len
            );
        }
    }

    #[test]
    fn test_hex_encode_32_bytes_sha256_width() {
        let bytes: Vec<u8> = (0..32).collect();
        let hex = hex_encode(&bytes);
        assert_eq!(hex.len(), 64);
        assert_eq!(
            hex,
            "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
        );
    }

    #[test]
    fn test_hex_encode_high_bit_bytes() {
        assert_eq!(hex_encode(&[0x80, 0xff, 0xfe, 0x7f]), "80fffe7f");
    }

    #[test]
    fn test_hex_encode_is_lowercase() {
        let bytes: Vec<u8> = (0xa0..=0xaf).collect();
        let hex = hex_encode(&bytes);
        assert!(hex.chars().all(|c| !c.is_ascii_uppercase()));
        assert_eq!(hex, "a0a1a2a3a4a5a6a7a8a9aaabacadaeaf");
    }

    #[test]
    fn test_hex_encode_all_zeros_32_bytes() {
        let bytes = [0u8; 32];
        let hex = hex_encode(&bytes);
        assert_eq!(hex.len(), 64);
        assert_eq!(hex, "0".repeat(64));
    }

    #[test]
    fn test_hex_encode_all_ff_32_bytes() {
        let bytes = [0xffu8; 32];
        let hex = hex_encode(&bytes);
        assert_eq!(hex.len(), 64);
        assert_eq!(hex, "f".repeat(64));
    }

    #[test]
    fn test_hex_encode_all_256_byte_values() {
        let bytes: Vec<u8> = (0..=255).collect();
        let hex = hex_encode(&bytes);
        assert_eq!(hex.len(), 512);
        let reference: String = bytes.iter().fold(String::new(), |mut acc, b| {
            use std::fmt::Write;
            write!(acc, "{b:02x}").unwrap();
            acc
        });
        assert_eq!(hex, reference);
    }

    #[test]
    fn test_hex_encode_matches_sha256_digest() {
        let mut hasher = Sha256::new();
        hasher.update(b"hello world");
        let digest = hasher.finalize();
        let hex = hex_encode(&digest);
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            hex,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }
}
