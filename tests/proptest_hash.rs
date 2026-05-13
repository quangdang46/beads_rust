//! Property-based tests for content hashing.
//!
//! Uses proptest to verify that:
//! - Hash output is always valid hex format
//! - Hashing is deterministic
//! - Content changes produce hash changes
//! - Hash is SHA256 (64 hex chars)

use chrono::Utc;
use proptest::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use tracing::info;

use beads_rust::model::{Issue, IssueType, Priority, Status};
use beads_rust::util::{ContentHashable, content_hash, content_hash_from_parts};

/// Initialize test logging for proptest
fn init_test_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_test_writer()
        .try_init();
}

/// Create a test issue with the given title and description
fn make_issue(title: &str, description: Option<&str>) -> Issue {
    Issue {
        id: "bd-test".to_string(),
        content_hash: None,
        title: title.to_string(),
        description: description.map(ToString::to_string),
        design: None,
        acceptance_criteria: None,
        notes: None,
        status: Status::Open,
        priority: Priority::MEDIUM,
        issue_type: IssueType::Task,
        assignee: None,
        owner: None,
        estimated_minutes: None,
        created_at: Utc::now(),
        created_by: None,
        updated_at: Utc::now(),
        closed_at: None,
        close_reason: None,
        closed_by_session: None,
        due_at: None,
        defer_until: None,
        external_ref: None,
        source_system: None,
        source_repo: None,
        source_repo_path: None,
        deleted_at: None,
        deleted_by: None,
        delete_reason: None,
        original_type: None,
        compaction_level: None,
        compacted_at: None,
        compacted_at_commit: None,
        original_size: None,
        sender: None,
        ephemeral: false,
        pinned: false,
        is_template: false,
        labels: vec![],
        dependencies: vec![],
        comments: vec![],
    }
}

fn status_strategy() -> impl Strategy<Value = Status> {
    prop_oneof![
        Just(Status::Open),
        Just(Status::InProgress),
        Just(Status::Blocked),
        Just(Status::Deferred),
        Just(Status::Draft),
        Just(Status::Closed),
        Just(Status::Tombstone),
        Just(Status::Pinned),
        "[a-z][a-z0-9_-]{0,16}".prop_map(Status::Custom),
    ]
}

fn issue_type_strategy() -> impl Strategy<Value = IssueType> {
    prop_oneof![
        Just(IssueType::Task),
        Just(IssueType::Bug),
        Just(IssueType::Feature),
        Just(IssueType::Epic),
        Just(IssueType::Chore),
        Just(IssueType::Docs),
        Just(IssueType::Question),
        "[a-z][a-z0-9_-]{0,16}".prop_map(IssueType::Custom),
    ]
}

fn optional_text_strategy() -> impl Strategy<Value = Option<String>> {
    proptest::option::of("\\PC{0,80}")
}

fn go_bd_reference_content_hash(issue: &Issue) -> String {
    let mut hasher = Sha256::new();
    push_go_field(&mut hasher, &issue.title);
    push_go_field(&mut hasher, issue.description.as_deref().unwrap_or(""));
    push_go_field(&mut hasher, issue.design.as_deref().unwrap_or(""));
    push_go_field(
        &mut hasher,
        issue.acceptance_criteria.as_deref().unwrap_or(""),
    );
    push_go_field(&mut hasher, issue.notes.as_deref().unwrap_or(""));
    push_go_field(&mut hasher, issue.status.as_str());
    push_go_field(&mut hasher, &issue.priority.0.to_string());
    push_go_field(&mut hasher, issue.issue_type.as_str());
    push_go_field(&mut hasher, issue.assignee.as_deref().unwrap_or(""));
    push_go_field(&mut hasher, issue.owner.as_deref().unwrap_or(""));
    push_go_field(&mut hasher, issue.created_by.as_deref().unwrap_or(""));
    push_go_field(&mut hasher, issue.external_ref.as_deref().unwrap_or(""));
    push_go_field(&mut hasher, issue.source_system.as_deref().unwrap_or(""));
    push_go_field(&mut hasher, if issue.pinned { "pinned" } else { "" });
    push_go_field(&mut hasher, if issue.is_template { "template" } else { "" });
    push_go_field(&mut hasher, ""); // quality_score nil
    push_go_field(&mut hasher, ""); // crystallizes false
    push_go_field(&mut hasher, ""); // await_type
    push_go_field(&mut hasher, ""); // await_id
    push_go_field(&mut hasher, "0"); // timeout duration
    for _ in 0..12 {
        push_go_field(&mut hasher, "");
    }
    beads_rust::util::hex_encode(&hasher.finalize())
}

fn push_go_field(hasher: &mut Sha256, value: &str) {
    hasher.update(value.as_bytes());
    hasher.update(b"\x00");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 100,
        ..Default::default()
    })]

    /// Property: Hash output is always valid 64-char hex string (SHA256)
    #[test]
    fn hash_valid_hex_format(title in "\\PC{1,200}") {
        init_test_logging();
        info!(
            "proptest_hash_format: title_len={len}",
            len = title.len()
        );

        let issue = make_issue(&title, None);
        let hash = content_hash(&issue);

        info!("proptest_hash_format: hash={hash}");

        prop_assert_eq!(hash.len(), 64, "SHA256 hash should be 64 hex chars");
        prop_assert!(
            hash.chars().all(|c: char| c.is_ascii_hexdigit()),
            "Hash must be valid hex: {hash}"
        );
        // SHA256 hex uses lowercase
        prop_assert!(
            hash.chars().all(|c: char| !c.is_ascii_uppercase()),
            "Hash should be lowercase hex: {hash}"
        );
    }

    /// Property: Hash is deterministic for same issue
    #[test]
    fn hash_deterministic(
        title in "\\PC{1,100}",
        description in proptest::option::of("\\PC{0,200}"),
    ) {
        init_test_logging();
        info!(
            "proptest_hash_deterministic: title_len={len}",
            len = title.len()
        );

        let issue = make_issue(&title, description.as_deref());

        let hash1 = content_hash(&issue);
        let hash2 = content_hash(&issue);

        prop_assert_eq!(hash1, hash2, "Same issue must produce same hash");
    }

    /// Property: Different titles produce different hashes
    #[test]
    fn hash_changes_with_title(
        title1 in "[a-zA-Z0-9 ]{5,50}",
        title2 in "[a-zA-Z0-9 ]{5,50}",
    ) {
        init_test_logging();

        prop_assume!(title1 != title2);

        let issue1 = make_issue(&title1, None);
        let issue2 = make_issue(&title2, None);

        let hash1 = content_hash(&issue1);
        let hash2 = content_hash(&issue2);

        prop_assert_ne!(hash1, hash2, "Different titles should produce different hashes");
    }

    /// Property: ContentHashable trait produces same result as direct function
    #[test]
    fn trait_matches_function(title in "\\PC{1,100}") {
        init_test_logging();

        let issue = make_issue(&title, None);

        let trait_hash = ContentHashable::content_hash(&issue);
        let fn_hash = content_hash(&issue);

        prop_assert_eq!(trait_hash, fn_hash, "Trait and function should produce same hash");
    }

    /// Property: content_hash_from_parts produces same result as content_hash
    #[test]
    fn parts_match_direct(
        title in "\\PC{1,100}",
        description in proptest::option::of("\\PC{0,100}"),
        notes in proptest::option::of("\\PC{0,100}"),
    ) {
        init_test_logging();

        let mut issue = make_issue(&title, description.as_deref());
        issue.notes = notes;

        let direct = content_hash(&issue);
        let from_parts = content_hash_from_parts(
            &issue.title,
            issue.description.as_deref(),
            issue.design.as_deref(),
            issue.acceptance_criteria.as_deref(),
            issue.notes.as_deref(),
            &issue.status,
            &issue.priority,
            &issue.issue_type,
            issue.assignee.as_deref(),
            issue.owner.as_deref(),
            issue.created_by.as_deref(),
            issue.external_ref.as_deref(),
            issue.source_system.as_deref(),
            issue.pinned,
            issue.is_template,
        );

        prop_assert_eq!(direct, from_parts, "Direct and from_parts should match");
    }

    /// Property: Hash changes when status changes
    #[test]
    fn hash_changes_with_status(title in "\\PC{1,50}") {
        init_test_logging();

        let mut issue = make_issue(&title, None);
        let hash_open = content_hash(&issue);

        issue.status = Status::Closed;
        let hash_closed = content_hash(&issue);

        prop_assert_ne!(hash_open, hash_closed, "Status change should change hash");
    }

    /// Property: Hash changes when priority changes
    #[test]
    fn hash_changes_with_priority(title in "\\PC{1,50}") {
        init_test_logging();

        let mut issue = make_issue(&title, None);
        let hash_p2 = content_hash(&issue);

        issue.priority = Priority::CRITICAL;
        let hash_p0 = content_hash(&issue);

        prop_assert_ne!(hash_p2, hash_p0, "Priority change should change hash");
    }

    /// Property: Hash changes when pinned flag changes
    #[test]
    fn hash_changes_with_pinned(title in "\\PC{1,50}") {
        init_test_logging();

        let mut issue = make_issue(&title, None);
        let hash_unpinned = content_hash(&issue);

        issue.pinned = true;
        let hash_pinned = content_hash(&issue);

        prop_assert_ne!(hash_unpinned, hash_pinned, "Pinned change should change hash");
    }

    /// Property: Hash ignores timestamp changes
    #[test]
    fn hash_ignores_timestamps(title in "\\PC{1,50}") {
        init_test_logging();

        let mut issue = make_issue(&title, None);
        let hash1 = content_hash(&issue);

        // Change timestamps
        issue.updated_at = Utc::now();
        let hash2 = content_hash(&issue);

        prop_assert_eq!(hash1, hash2, "Timestamp changes should not affect hash");
    }

    /// Property: Rust content_hash stays byte-compatible with Go bd for all shared fields.
    #[test]
    fn hash_matches_go_bd_reference_for_shared_fields(
        title in "\\PC{1,80}",
        description in optional_text_strategy(),
        design in optional_text_strategy(),
        acceptance_criteria in optional_text_strategy(),
        notes in optional_text_strategy(),
        status in status_strategy(),
        priority in 0i32..=4,
        issue_type in issue_type_strategy(),
        assignee in optional_text_strategy(),
        owner in optional_text_strategy(),
        created_by in optional_text_strategy(),
        external_ref in optional_text_strategy(),
        source_system in optional_text_strategy(),
        pinned in any::<bool>(),
        is_template in any::<bool>(),
    ) {
        init_test_logging();

        let mut issue = make_issue(&title, description.as_deref());
        issue.design = design;
        issue.acceptance_criteria = acceptance_criteria;
        issue.notes = notes;
        issue.status = status;
        issue.priority = Priority(priority);
        issue.issue_type = issue_type;
        issue.assignee = assignee;
        issue.owner = owner;
        issue.created_by = created_by;
        issue.external_ref = external_ref;
        issue.source_system = source_system;
        issue.pinned = pinned;
        issue.is_template = is_template;

        prop_assert_eq!(
            content_hash(&issue),
            go_bd_reference_content_hash(&issue),
            "Rust hash must match Go bd canonical field writer"
        );
    }
}

/// Property: Low collision rate in batch hashing
#[test]
fn hash_low_collision_rate() {
    init_test_logging();
    info!("proptest_hash_collision: starting collision test");

    let mut hashes = HashSet::new();
    let batch_size = 1000;

    for i in 0..batch_size {
        let title = format!("Unique Issue Title Number {i} with extra text");
        let issue = make_issue(&title, Some(&format!("Description for issue {i}")));
        let hash = content_hash(&issue);

        assert!(
            !hashes.contains(&hash),
            "Collision detected at iteration {i}: hash={hash}"
        );
        hashes.insert(hash);
    }

    assert_eq!(
        hashes.len(),
        batch_size,
        "Should have {batch_size} unique hashes"
    );
    info!("proptest_hash_collision: PASS - {batch_size} unique hashes, 0 collisions");
}

/// Property: Hash is stable across issue type changes
#[test]
fn hash_changes_with_issue_type() {
    init_test_logging();

    let mut issue = make_issue("Test Issue", None);
    let hash_task = content_hash(&issue);

    issue.issue_type = IssueType::Bug;
    let hash_bug = content_hash(&issue);

    issue.issue_type = IssueType::Feature;
    let hash_feature = content_hash(&issue);

    assert_ne!(hash_task, hash_bug, "Task vs Bug should differ");
    assert_ne!(hash_task, hash_feature, "Task vs Feature should differ");
    assert_ne!(hash_bug, hash_feature, "Bug vs Feature should differ");

    info!("proptest_hash_type: PASS - different types produce different hashes");
}

mod hex_encode_fuzz {
    use beads_rust::util::hex_encode;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn hex_encode_roundtrip(bytes in proptest::collection::vec(any::<u8>(), 0..=256)) {
            let hex = hex_encode(&bytes);

            prop_assert_eq!(
                hex.len(), bytes.len() * 2,
                "output length must be exactly 2 * input length for {} bytes",
                bytes.len()
            );
            prop_assert!(
                hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "output must be lowercase hex: {hex}"
            );

            let decoded: Vec<u8> = (0..bytes.len())
                .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
                .collect();
            prop_assert_eq!(&decoded, &bytes, "roundtrip decode must match original bytes");
        }
    }
}
