//! E2E tests for the schema command.
//!
//! Validates that `br schema` works without an initialized workspace and
//! produces machine-parseable output in both JSON and TOON modes.

mod common;

#[cfg(feature = "self_update")]
use common::cli::parse_created_id;
use common::cli::{BrWorkspace, extract_json_payload, run_br};
use serde_json::Value;
#[cfg(feature = "self_update")]
use std::{fs, path::PathBuf};
use toon_rust::try_decode as parse_toon;

#[cfg(feature = "self_update")]
const UPDATE_AGENT_BASELINE_ENV: &str = "UPDATE_AGENT_BASELINE";

#[test]
fn e2e_schema_json_issue() {
    let _log = common::test_log("e2e_schema_json_issue");
    let workspace = BrWorkspace::new();

    let run = run_br(
        &workspace,
        ["schema", "issue", "--format", "json"],
        "schema_issue_json",
    );
    assert!(
        run.status.success(),
        "schema issue json failed: {}",
        run.stderr
    );

    let payload = extract_json_payload(&run.stdout);
    let json: Value = serde_json::from_str(&payload).expect("valid JSON output");

    assert_eq!(json["tool"], "br");
    assert!(json.get("generated_at").is_some(), "missing generated_at");
    assert!(json.get("schemas").is_some(), "missing schemas");
    assert!(
        json["schemas"].get("Issue").is_some(),
        "schemas should include Issue"
    );
}

#[test]
fn e2e_schema_toon_decodes() {
    let _log = common::test_log("e2e_schema_toon_decodes");
    let workspace = BrWorkspace::new();

    let run = run_br(
        &workspace,
        ["schema", "issue-details", "--format", "toon"],
        "schema_issue_details_toon",
    );
    assert!(
        run.status.success(),
        "schema issue-details toon failed: {}",
        run.stderr
    );

    let toon = run.stdout.trim();
    assert!(!toon.is_empty(), "TOON output should be non-empty");

    let decoded = parse_toon(toon, None).expect("valid TOON");
    let json = Value::from(decoded);

    assert_eq!(json["tool"], "br");
    assert!(json.get("generated_at").is_some(), "missing generated_at");
    // TOON output uses key folding, so nested map keys may appear as dotted keys.
    let has_nested = json
        .get("schemas")
        .and_then(|schemas| schemas.get("IssueDetails"))
        .is_some();
    let has_folded = json.get("schemas.IssueDetails").is_some();
    assert!(
        has_nested || has_folded,
        "expected IssueDetails schema (nested or folded), got keys: {:?}",
        json.as_object().map(|o| o.keys().collect::<Vec<_>>())
    );
}

#[test]
fn e2e_capabilities_json_no_workspace() {
    let _log = common::test_log("e2e_capabilities_json_no_workspace");
    let workspace = BrWorkspace::new();

    let run = run_br(
        &workspace,
        ["capabilities", "--format", "json"],
        "capabilities_json",
    );
    assert!(
        run.status.success(),
        "capabilities json failed: {}",
        run.stderr
    );

    let payload = extract_json_payload(&run.stdout);
    let json: Value = serde_json::from_str(&payload).expect("valid JSON output");

    assert_eq!(json["tool"], "br");
    assert_eq!(json["contract_version"], "br.capabilities.v1");
    assert!(
        json["features"].as_array().is_some_and(|features| {
            features
                .iter()
                .any(|feature| feature["name"] == "agent_machine_output")
        }),
        "missing agent_machine_output feature: {json}"
    );
    assert!(
        json["commands"].as_array().is_some_and(|commands| {
            commands
                .iter()
                .any(|command| command["name"] == "capabilities")
                && commands
                    .iter()
                    .any(|command| command["name"] == "robot-docs")
        }),
        "missing new agent commands: {json}"
    );
    assert!(
        json["exit_codes"].as_array().is_some_and(|codes| {
            codes
                .iter()
                .any(|code| code["code"] == 4 && code["category"] == "validation")
        }),
        "missing exit-code contract: {json}"
    );
}

#[test]
fn e2e_robot_docs_guide_text_is_concise() {
    let _log = common::test_log("e2e_robot_docs_guide_text_is_concise");
    let workspace = BrWorkspace::new();

    let run = run_br(&workspace, ["robot-docs", "guide"], "robot_docs_guide_text");
    assert!(
        run.status.success(),
        "robot-docs guide failed: {}",
        run.stderr
    );

    let lines = run.stdout.lines().count();
    assert!(lines <= 80, "guide should stay concise, got {lines} lines");
    assert!(run.stdout.contains("br capabilities --format json"));
    assert!(run.stdout.contains("br ready --json"));
    assert!(run.stdout.contains("br never runs git"));
}

#[test]
fn e2e_robot_docs_guide_json_no_workspace() {
    let _log = common::test_log("e2e_robot_docs_guide_json_no_workspace");
    let workspace = BrWorkspace::new();

    let run = run_br(
        &workspace,
        ["robot-docs", "guide", "--format", "json"],
        "robot_docs_guide_json",
    );
    assert!(
        run.status.success(),
        "robot-docs guide json failed: {}",
        run.stderr
    );

    let payload = extract_json_payload(&run.stdout);
    let json: Value = serde_json::from_str(&payload).expect("valid JSON output");

    assert_eq!(json["tool"], "br");
    assert_eq!(json["contract_version"], "br.robot_docs.v1");
    assert!(
        json["line_count"].as_u64().is_some_and(|count| count <= 80),
        "guide should report <=80 lines: {json}"
    );
    assert!(
        json["canonical_commands"]
            .as_array()
            .is_some_and(|commands| {
                commands
                    .iter()
                    .any(|command| command["command"] == "br coordination status --json")
            }),
        "missing canonical coordination command: {json}"
    );
}

#[cfg(feature = "self_update")]
#[test]
fn agent_baseline_snapshots_match_current_binary() {
    let _log = common::test_log("agent_baseline_snapshots_match_current_binary");
    let workspace = BrWorkspace::new();

    compare_agent_baseline_help(&workspace);
    compare_agent_baseline_schemas(&workspace);
    let id_two = seed_agent_baseline_workspace(&workspace);
    compare_agent_baseline_examples(&workspace, &id_two);
    compare_agent_baseline_error(&workspace);
}

#[cfg(feature = "self_update")]
fn compare_agent_baseline_help(workspace: &BrWorkspace) {
    compare_text_baseline(
        "help/br_help.txt",
        &run_success(workspace, ["--help"], "baseline_help"),
    );
    compare_text_baseline(
        "help/br_list_help.txt",
        &run_success(workspace, ["list", "--help"], "baseline_list_help"),
    );
    compare_text_baseline(
        "help/br_schema_help.txt",
        &run_success(workspace, ["schema", "--help"], "baseline_schema_help"),
    );
}

#[cfg(feature = "self_update")]
fn compare_agent_baseline_schemas(workspace: &BrWorkspace) {
    compare_json_baseline(
        "schemas/schema_all.json",
        &run_success(
            workspace,
            ["schema", "all", "--format", "json"],
            "baseline_schema_all",
        ),
        normalize_schema_snapshot,
    );
    compare_json_baseline(
        "schemas/schema_error.json",
        &run_success(
            workspace,
            ["schema", "error", "--format", "json"],
            "baseline_schema_error",
        ),
        normalize_schema_snapshot,
    );
    compare_json_baseline(
        "schemas/schema_issue_details.json",
        &run_success(
            workspace,
            ["schema", "issue-details", "--format", "json"],
            "baseline_schema_issue_details",
        ),
        normalize_schema_snapshot,
    );
}

#[cfg(feature = "self_update")]
fn seed_agent_baseline_workspace(workspace: &BrWorkspace) -> String {
    let init = run_br(workspace, ["init", "--prefix", "bd"], "baseline_init");
    assert!(init.status.success(), "init failed: {}", init.stderr);
    let create_one = run_br(
        workspace,
        [
            "create",
            "One",
            "--type",
            "task",
            "--priority",
            "2",
            "--description",
            "Short desc",
        ],
        "baseline_create_one",
    );
    assert!(
        create_one.status.success(),
        "create one failed: {}",
        create_one.stderr
    );
    let create_two = run_br(
        workspace,
        ["create", "Two", "--type", "bug", "--priority", "0"],
        "baseline_create_two",
    );
    assert!(
        create_two.status.success(),
        "create two failed: {}",
        create_two.stderr
    );
    let id_two = parse_created_id(&create_two.stdout);
    let create_three = run_br(
        workspace,
        ["create", "Three", "--type", "feature", "--priority", "1"],
        "baseline_create_three",
    );
    assert!(
        create_three.status.success(),
        "create three failed: {}",
        create_three.stderr
    );
    id_two
}

#[cfg(feature = "self_update")]
fn compare_agent_baseline_examples(workspace: &BrWorkspace, id_two: &str) {
    compare_json_baseline(
        "examples/list_limit3.json",
        &run_success(
            workspace,
            ["list", "--format", "json", "--limit", "3"],
            "baseline_list_limit3_json",
        ),
        normalize_issue_example_snapshot,
    );
    compare_toon_baseline(
        "examples/list_limit3.toon",
        &run_success(
            workspace,
            ["list", "--format", "toon", "--limit", "3"],
            "baseline_list_limit3_toon",
        ),
    );
    compare_json_baseline(
        "examples/ready.json",
        &run_success(
            workspace,
            ["ready", "--format", "json"],
            "baseline_ready_json",
        ),
        normalize_issue_example_snapshot,
    );
    compare_toon_baseline(
        "examples/ready.toon",
        &run_success(
            workspace,
            ["ready", "--format", "toon"],
            "baseline_ready_toon",
        ),
    );
    compare_json_baseline(
        "examples/show_one.json",
        &run_success(
            workspace,
            ["show", id_two, "--format", "json"],
            "baseline_show_one_json",
        ),
        normalize_issue_example_snapshot,
    );
    compare_toon_baseline(
        "examples/show_one.toon",
        &run_success(
            workspace,
            ["show", id_two, "--format", "toon"],
            "baseline_show_one_toon",
        ),
    );
    compare_json_baseline(
        "examples/version.json",
        &run_success(workspace, ["version", "--json"], "baseline_version_json"),
        normalize_version_snapshot,
    );
}

#[cfg(feature = "self_update")]
fn compare_agent_baseline_error(workspace: &BrWorkspace) {
    let missing = run_br(
        workspace,
        ["show", "bd-NOTEXIST", "--json"],
        "baseline_show_not_found",
    );
    assert_eq!(
        missing.status.code(),
        Some(3),
        "unexpected status: {missing:?}"
    );
    compare_json_baseline(
        "errors/show_not_found.json",
        &missing.stderr,
        normalize_noop,
    );
}

#[cfg(feature = "self_update")]
fn run_success<const N: usize>(workspace: &BrWorkspace, args: [&str; N], label: &str) -> String {
    let run = run_br(workspace, args, label);
    assert!(
        run.status.success(),
        "{label} failed: stdout={} stderr={}",
        run.stdout,
        run.stderr
    );
    run.stdout
}

#[cfg(feature = "self_update")]
fn compare_text_baseline(relative_path: &str, actual: &str) {
    let path = baseline_path(relative_path);
    let actual = normalize_text_snapshot(actual);
    if should_update_agent_baseline() {
        fs::write(&path, actual).expect("update agent baseline text snapshot");
        return;
    }

    let expected = fs::read_to_string(&path).expect("read agent baseline text snapshot");
    let expected = normalize_text_snapshot(&expected);
    assert_eq!(
        expected, actual,
        "agent_baseline/{relative_path} is stale; rerun with {UPDATE_AGENT_BASELINE_ENV}=1"
    );
}

#[cfg(feature = "self_update")]
fn compare_json_baseline(relative_path: &str, actual: &str, normalize: fn(&mut Value)) {
    let path = baseline_path(relative_path);
    let actual_payload = extract_json_payload(actual);
    let mut actual: Value =
        serde_json::from_str(&actual_payload).expect("valid generated JSON for agent baseline");
    normalize(&mut actual);

    if should_update_agent_baseline() {
        let pretty = serde_json::to_string_pretty(&actual)
            .expect("serialize normalized agent baseline JSON snapshot");
        fs::write(&path, with_trailing_newline(&pretty))
            .expect("update agent baseline JSON snapshot");
        return;
    }

    let expected_raw = fs::read_to_string(&path).expect("read agent baseline JSON snapshot");
    let mut expected: Value =
        serde_json::from_str(&expected_raw).expect("valid agent baseline JSON snapshot");
    normalize(&mut expected);

    assert_eq!(
        expected, actual,
        "agent_baseline/{relative_path} is stale; rerun with {UPDATE_AGENT_BASELINE_ENV}=1"
    );
}

#[cfg(feature = "self_update")]
fn compare_toon_baseline(relative_path: &str, actual: &str) {
    let path = baseline_path(relative_path);
    let actual = with_trailing_newline(actual.trim_end());
    if should_update_agent_baseline() {
        fs::write(&path, actual).expect("update agent baseline TOON snapshot");
        return;
    }

    let expected_raw = fs::read_to_string(&path).expect("read agent baseline TOON snapshot");
    let mut expected =
        Value::from(parse_toon(&expected_raw, None).expect("valid agent baseline TOON snapshot"));
    let mut actual =
        Value::from(parse_toon(&actual, None).expect("valid generated TOON for agent baseline"));
    normalize_issue_example_snapshot(&mut expected);
    normalize_issue_example_snapshot(&mut actual);

    assert_eq!(
        expected, actual,
        "agent_baseline/{relative_path} is stale; rerun with {UPDATE_AGENT_BASELINE_ENV}=1"
    );
}

#[cfg(feature = "self_update")]
fn baseline_path(relative_path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("agent_baseline")
        .join(relative_path)
}

#[cfg(feature = "self_update")]
fn should_update_agent_baseline() -> bool {
    std::env::var_os(UPDATE_AGENT_BASELINE_ENV).is_some()
}

#[cfg(feature = "self_update")]
fn with_trailing_newline(text: &str) -> String {
    format!("{text}\n")
}

#[cfg(feature = "self_update")]
fn normalize_text_snapshot(text: &str) -> String {
    let mut normalized = text
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    normalized.push('\n');
    normalized
}

#[cfg(feature = "self_update")]
fn normalize_schema_snapshot(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "generated_at".to_string(),
            Value::String("<GENERATED_AT>".to_string()),
        );
    }
}

#[cfg(feature = "self_update")]
fn normalize_version_snapshot(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        for key in ["branch", "build", "commit", "rust_version", "target"] {
            if object.contains_key(key) {
                object.insert(key.to_string(), Value::String(format!("<{key}>")));
            }
        }
    }
}

#[cfg(feature = "self_update")]
fn normalize_issue_example_snapshot(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                normalize_issue_example_snapshot(item);
            }
        }
        Value::Object(object) => {
            for (key, child) in object {
                match key.as_str() {
                    "closed_at" | "created_at" | "updated_at" => {
                        *child = Value::String("<TIMESTAMP>".to_string());
                    }
                    "created_by" => {
                        *child = Value::String("<ACTOR>".to_string());
                    }
                    "source_repo" => {
                        *child = Value::String("<SOURCE_REPO>".to_string());
                    }
                    "depends_on_id" | "id" | "issue_id" => {
                        *child = Value::String("<ISSUE_ID>".to_string());
                    }
                    _ => normalize_issue_example_snapshot(child),
                }
            }
        }
        Value::Bool(_) | Value::Null | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(feature = "self_update")]
fn normalize_noop(_: &mut Value) {}
