use beads_rust::model::{Issue, IssueType, Priority, Status};
use beads_rust::storage::SqliteStorage;
use beads_rust::sync::{auto_flush, history::HistoryConfig};
use chrono::Utc;
use std::fs;
use tempfile::TempDir;

fn make_issue(id: &str) -> Issue {
    Issue {
        id: id.to_string(),
        title: "Test Issue".to_string(),
        status: Status::Open,
        priority: Priority::MEDIUM,
        issue_type: IssueType::Task,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        ..Default::default()
    }
}

// `update_issue` always advances `updated_at`, so a change-then-revert pair is
// still a real JSONL content change. This remains ignored until the integration
// surface has a supported way to reproduce a byte-identical dirty marker.
#[test]
#[ignore = "integration test cannot mark an issue dirty without changing JSONL bytes"]
fn test_auto_flush_optimizes_no_content_change() {
    let temp_dir = TempDir::new().unwrap();
    let beads_dir = temp_dir.path().join(".beads");
    fs::create_dir(&beads_dir).unwrap();
    let db_path = beads_dir.join("beads.db");
    let jsonl_path = beads_dir.join("issues.jsonl");

    let mut storage = SqliteStorage::open(&db_path).unwrap();

    // 1. Create an issue
    let issue = make_issue("bd-1");
    storage.create_issue(&issue, "tester").unwrap();

    // 2. First auto-flush (should export)
    let result = auto_flush(
        &mut storage,
        &beads_dir,
        &jsonl_path,
        false,
        HistoryConfig::default(),
    )
    .unwrap();
    assert!(result.flushed, "First flush should happen");
    assert_eq!(result.exported_count, 1);

    // 3. Mark issue dirty effectively WITHOUT changing content
    // We do this by changing it and changing it back.
    // NOTE: This relies on the fact that we haven't exported the intermediate state.

    // Change title
    let update_change = beads_rust::storage::IssueUpdate {
        title: Some("Changed Title".to_string()),
        ..Default::default()
    };
    storage
        .update_issue("bd-1", &update_change, "tester")
        .unwrap();

    // Revert title
    let update_revert = beads_rust::storage::IssueUpdate {
        title: Some("Test Issue".to_string()),
        ..Default::default()
    };
    storage
        .update_issue("bd-1", &update_revert, "tester")
        .unwrap();

    // Verify it is dirty
    let dirty_ids = storage.get_dirty_issue_ids().unwrap();
    assert_eq!(dirty_ids.len(), 1, "Issue should be dirty after updates");

    // 4. Second auto-flush should now skip the rewrite because the exported JSONL
    // would be byte-identical.
    let result = auto_flush(
        &mut storage,
        &beads_dir,
        &jsonl_path,
        false,
        HistoryConfig::default(),
    )
    .unwrap();

    assert!(
        !result.flushed,
        "Auto-flush should skip a no-op rewrite when the JSONL would be unchanged"
    );

    // And dirty flags should be cleared
    let dirty_ids = storage.get_dirty_issue_ids().unwrap();
    assert!(dirty_ids.is_empty(), "Dirty flags should be cleared");
}

#[test]
fn test_auto_flush_flush_on_label_change() {
    let temp_dir = TempDir::new().unwrap();
    let beads_dir = temp_dir.path().join(".beads");
    fs::create_dir(&beads_dir).unwrap();
    let db_path = beads_dir.join("beads.db");
    let jsonl_path = beads_dir.join("issues.jsonl");

    let mut storage = SqliteStorage::open(&db_path).unwrap();

    // 1. Create an issue
    let issue = make_issue("bd-1");
    storage.create_issue(&issue, "tester").unwrap();

    // 2. First auto-flush
    let result = auto_flush(
        &mut storage,
        &beads_dir,
        &jsonl_path,
        false,
        HistoryConfig::default(),
    )
    .unwrap();
    assert!(result.flushed);

    // 3. Add a label
    storage.add_label("bd-1", "bug", "tester").unwrap();

    // Verify dirty
    let dirty_ids = storage.get_dirty_issue_ids().unwrap();
    assert_eq!(dirty_ids.len(), 1);

    // 4. Second auto-flush - SHOULD FLUSH because label was added
    let result = auto_flush(
        &mut storage,
        &beads_dir,
        &jsonl_path,
        false,
        HistoryConfig::default(),
    )
    .unwrap();

    // This assertion will FAIL if my optimization is active and flawed
    assert!(result.flushed, "Should flush when label is added");
}

#[test]
fn test_auto_flush_uses_resolved_jsonl_path() {
    let temp_dir = TempDir::new().unwrap();
    let beads_dir = temp_dir.path().join(".beads");
    fs::create_dir(&beads_dir).unwrap();
    let db_path = beads_dir.join("beads.db");
    let custom_jsonl_path = temp_dir.path().join("custom-issues.jsonl");

    let mut storage = SqliteStorage::open(&db_path).unwrap();
    storage.create_issue(&make_issue("bd-1"), "tester").unwrap();

    // A JSONL path outside `.beads/` requires `allow_external_jsonl = true`
    // — otherwise export refuses with "Path is outside the beads directory"
    // as a safety check against wayward writes during refactors.
    let result = auto_flush(
        &mut storage,
        &beads_dir,
        &custom_jsonl_path,
        true,
        HistoryConfig::default(),
    )
    .unwrap();

    assert!(result.flushed);
    assert!(custom_jsonl_path.exists());
    assert!(!beads_dir.join("issues.jsonl").exists());
}

#[test]
fn test_auto_flush_preserves_unrelated_existing_jsonl_lines() {
    let temp_dir = TempDir::new().unwrap();
    let beads_dir = temp_dir.path().join(".beads");
    fs::create_dir(&beads_dir).unwrap();
    let db_path = beads_dir.join("beads.db");
    let jsonl_path = beads_dir.join("issues.jsonl");

    let mut storage = SqliteStorage::open(&db_path).unwrap();
    storage.create_issue(&make_issue("bd-1"), "tester").unwrap();
    auto_flush(
        &mut storage,
        &beads_dir,
        &jsonl_path,
        false,
        HistoryConfig::default(),
    )
    .unwrap();

    let extra_issue = make_issue("bd-extra");
    let mut contents = fs::read_to_string(&jsonl_path).unwrap();
    contents.push_str(&format!(
        "{}\n",
        serde_json::to_string(&extra_issue).unwrap()
    ));
    fs::write(&jsonl_path, contents).unwrap();

    storage
        .update_issue(
            "bd-1",
            &beads_rust::storage::IssueUpdate {
                title: Some("Updated".to_string()),
                ..Default::default()
            },
            "tester",
        )
        .unwrap();

    let result = auto_flush(
        &mut storage,
        &beads_dir,
        &jsonl_path,
        false,
        HistoryConfig::default(),
    )
    .unwrap();
    assert!(result.flushed);

    let issues = beads_rust::sync::read_issues_from_jsonl(&jsonl_path).unwrap();
    let ids = issues
        .iter()
        .map(|issue| issue.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["bd-1", "bd-extra"]);
    assert_eq!(
        issues
            .iter()
            .find(|issue| issue.id == "bd-1")
            .unwrap()
            .title,
        "Updated"
    );
}
