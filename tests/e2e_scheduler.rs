mod common;

use common::cli::{BrWorkspace, extract_json_payload, run_br};
use serde_json::Value;

fn parse_created_id(stdout: &str) -> String {
    let line = stdout.lines().next().unwrap_or("");
    let normalized = line.strip_prefix("✓ ").unwrap_or(line);
    normalized
        .strip_prefix("Created ")
        .and_then(|rest| rest.split(':').next())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn create_issue(workspace: &BrWorkspace, title: &str, priority: &str) -> String {
    let result = run_br(
        workspace,
        ["create", title, "-p", priority, "-t", "task"],
        "create_issue",
    );
    assert!(result.status.success(), "create failed: {}", result.stderr);
    parse_created_id(&result.stdout)
}

#[test]
fn scheduler_json_ranks_ready_bottlenecks_with_evidence() {
    let _log = common::test_log("scheduler_json_ranks_ready_bottlenecks_with_evidence");
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let foundation = create_issue(&workspace, "Foundation task", "1");
    let ui = create_issue(&workspace, "Independent UI task", "1");
    let follow_on = create_issue(&workspace, "Depends on foundation", "2");

    let label_foundation = run_br(
        &workspace,
        ["update", &foundation, "--add-label", "core"],
        "label_foundation",
    );
    assert!(
        label_foundation.status.success(),
        "label foundation failed: {}",
        label_foundation.stderr
    );
    let label_ui = run_br(&workspace, ["update", &ui, "--add-label", "ui"], "label_ui");
    assert!(
        label_ui.status.success(),
        "label ui failed: {}",
        label_ui.stderr
    );

    let dep = run_br(
        &workspace,
        ["dep", "add", &follow_on, &foundation],
        "add_dependency",
    );
    assert!(dep.status.success(), "dep add failed: {}", dep.stderr);

    let result = run_br(
        &workspace,
        ["scheduler", "--json", "--limit", "2"],
        "scheduler_json",
    );
    assert!(
        result.status.success(),
        "scheduler failed: {}",
        result.stderr
    );
    let payload = extract_json_payload(&result.stdout);
    let json: Value = serde_json::from_str(&payload).expect("scheduler json");
    let recommendations = json["recommendations"].as_array().expect("recommendations");

    assert_eq!(json["schema"], "br.scheduler.v1");
    assert_eq!(json["candidate_count"], 2);
    assert_eq!(recommendations.len(), 2);
    assert_eq!(recommendations[0]["issue"]["id"], foundation);
    assert_eq!(
        recommendations[0]["evidence"]["dependency_impact"]["dependent_count"],
        1
    );
    assert!(
        recommendations[0]["score"].as_i64().unwrap()
            > recommendations[1]["score"].as_i64().unwrap(),
        "dependency impact should break same-priority fallback ties"
    );
    assert_eq!(
        json["fallback_policy"]["sort"],
        "priority ASC, created_at ASC, id ASC"
    );
}

#[test]
fn scheduler_candidate_limit_refills_after_external_blockers() {
    let _log = common::test_log("scheduler_candidate_limit_refills_after_external_blockers");
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let blocked_one = create_issue(&workspace, "Blocked external one", "0");
    let blocked_two = create_issue(&workspace, "Blocked external two", "0");
    let free_one = create_issue(&workspace, "Free local one", "1");
    let free_two = create_issue(&workspace, "Free local two", "1");

    for (label, issue_id) in [("block_one", &blocked_one), ("block_two", &blocked_two)] {
        let dep = run_br(
            &workspace,
            ["dep", "add", issue_id, "external:missing:capability"],
            label,
        );
        assert!(
            dep.status.success(),
            "external dep add failed: {}",
            dep.stderr
        );
    }

    let result = run_br(
        &workspace,
        [
            "scheduler",
            "--json",
            "--candidate-limit",
            "2",
            "--limit",
            "2",
        ],
        "scheduler_external_candidate_limit",
    );
    assert!(
        result.status.success(),
        "scheduler failed: {}",
        result.stderr
    );
    let payload = extract_json_payload(&result.stdout);
    let json: Value = serde_json::from_str(&payload).expect("scheduler json");
    let recommendations = json["recommendations"].as_array().expect("recommendations");

    assert_eq!(json["candidate_count"], 2);
    assert_eq!(recommendations.len(), 2);
    let recommended_ids = recommendations
        .iter()
        .map(|item| item["issue"]["id"].as_str().expect("issue id"))
        .collect::<Vec<_>>();
    assert!(recommended_ids.contains(&free_one.as_str()));
    assert!(recommended_ids.contains(&free_two.as_str()));
    assert!(!recommended_ids.contains(&blocked_one.as_str()));
    assert!(!recommended_ids.contains(&blocked_two.as_str()));
}

#[test]
fn scheduler_alias_emits_text_recommendations() {
    let _log = common::test_log("scheduler_alias_emits_text_recommendations");
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);
    let _id = create_issue(&workspace, "Standalone task", "2");

    let result = run_br(&workspace, ["schedule", "--limit", "1"], "schedule_text");
    assert!(
        result.status.success(),
        "schedule failed: {}",
        result.stderr
    );
    assert!(
        result.stdout.contains("Scheduler recommendations"),
        "unexpected scheduler text output: {}",
        result.stdout
    );
}
