//! Regression coverage for immutable GitHub Actions pins.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

const INVENTORY_PATH: &str = ".github/action-pins.jsonl";
const WORKFLOW_DIR: &str = ".github/workflows";
const WORKFLOW_DIR_PREFIX: &str = ".github/workflows/";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct InventoryKey {
    workflow: String,
    action: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InventoryRecord {
    workflow: String,
    action: String,
    #[serde(rename = "sha")]
    expected_revision: String,
    tag: String,
    source: String,
}

#[derive(Debug)]
struct InventoryEntry {
    expected_revision: String,
}

#[derive(Debug)]
struct WorkflowUse {
    key: InventoryKey,
    revision: String,
    line: usize,
}

#[test]
fn repository_workflow_action_pins_are_inventory_backed() -> Result<(), String> {
    verify_action_pins(Path::new("."), Path::new(INVENTORY_PATH))
        .map_err(|errors| errors.join("\n"))
}

#[test]
fn clean_fixture_passes() -> Result<(), String> {
    let fixture = PinFixture::new()?;
    fixture.write_workflow(&format!(
        r"
name: fixture
on: push
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@{PIN_A}
      - uses: ./local-action
"
    ))?;
    fixture.write_inventory(&[inventory_line(
        ".github/workflows/example.yml",
        "actions/checkout",
        PIN_A,
    )])?;

    verify_action_pins(fixture.root(), &fixture.inventory_path())
        .map_err(|errors| errors.join("\n"))
}

#[test]
fn rejects_mutable_action_ref() -> Result<(), String> {
    let fixture = PinFixture::new()?;
    fixture.write_workflow(
        r"
name: fixture
on: push
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
",
    )?;
    fixture.write_inventory(&[inventory_line(
        ".github/workflows/example.yml",
        "actions/checkout",
        PIN_A,
    )])?;

    let errors = expect_verification_errors(&fixture)?;
    require_error_contains(&errors, "not pinned to a 40-character SHA")
}

#[test]
fn rejects_missing_inventory_entry() -> Result<(), String> {
    let fixture = PinFixture::new()?;
    fixture.write_workflow(&format!(
        r"
name: fixture
on: push
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@{PIN_A}
"
    ))?;
    fixture.write_inventory(&[inventory_line(
        ".github/workflows/example.yml",
        "actions/setup-go",
        PIN_B,
    )])?;

    let errors = expect_verification_errors(&fixture)?;
    require_error_contains(&errors, "missing inventory entry")
}

#[test]
fn rejects_mismatched_sha() -> Result<(), String> {
    let fixture = PinFixture::new()?;
    fixture.write_workflow(&format!(
        r"
name: fixture
on: push
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@{PIN_A}
"
    ))?;
    fixture.write_inventory(&[inventory_line(
        ".github/workflows/example.yml",
        "actions/checkout",
        PIN_B,
    )])?;

    let errors = expect_verification_errors(&fixture)?;
    require_error_contains(&errors, "inventory SHA mismatch")
}

#[test]
fn rejects_malformed_inventory_sha() -> Result<(), String> {
    let fixture = PinFixture::new()?;
    fixture.write_workflow(&format!(
        r"
name: fixture
on: push
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@{PIN_A}
"
    ))?;
    fixture.write_inventory(&[inventory_line(
        ".github/workflows/example.yml",
        "actions/checkout",
        "v4",
    )])?;

    let errors = expect_verification_errors(&fixture)?;
    require_error_contains(&errors, "inventory SHA is not a 40-character hex value")
}

#[test]
fn rejects_duplicate_inventory_entry() -> Result<(), String> {
    let fixture = PinFixture::new()?;
    fixture.write_workflow(&format!(
        r"
name: fixture
on: push
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@{PIN_A}
"
    ))?;
    fixture.write_inventory(&[
        inventory_line(".github/workflows/example.yml", "actions/checkout", PIN_A),
        inventory_line(".github/workflows/example.yml", "actions/checkout", PIN_A),
    ])?;

    let errors = expect_verification_errors(&fixture)?;
    require_error_contains(&errors, "duplicate inventory entry")
}

#[test]
fn rejects_stale_inventory_entry() -> Result<(), String> {
    let fixture = PinFixture::new()?;
    fixture.write_workflow(&format!(
        r"
name: fixture
on: push
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@{PIN_A}
"
    ))?;
    fixture.write_inventory(&[
        inventory_line(".github/workflows/example.yml", "actions/checkout", PIN_A),
        inventory_line(".github/workflows/old.yml", "actions/setup-go", PIN_B),
    ])?;

    let errors = expect_verification_errors(&fixture)?;
    require_error_contains(&errors, "stale inventory entry")
}

#[test]
fn rejects_inventory_path_outside_workflow_dir() -> Result<(), String> {
    let fixture = PinFixture::new()?;
    fixture.write_workflow(&format!(
        r"
name: fixture
on: push
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@{PIN_A}
"
    ))?;
    fixture.write_inventory(&[inventory_line(
        ".github/workflows-old/example.yml",
        "actions/checkout",
        PIN_A,
    )])?;

    let errors = expect_verification_errors(&fixture)?;
    require_error_contains(&errors, "workflow must live under")
}

fn verify_action_pins(repo_root: &Path, inventory_path: &Path) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let inventory = match load_inventory(inventory_path) {
        Ok(inventory) => inventory,
        Err(mut inventory_errors) => {
            errors.append(&mut inventory_errors);
            BTreeMap::new()
        }
    };
    let workflow_uses = match scan_workflows(repo_root) {
        Ok(workflow_uses) => workflow_uses,
        Err(mut scan_errors) => {
            errors.append(&mut scan_errors);
            Vec::new()
        }
    };

    if !errors.is_empty() {
        return Err(errors);
    }

    let mut seen = BTreeSet::new();
    for workflow_use in workflow_uses {
        match inventory.get(&workflow_use.key) {
            Some(record) if record.expected_revision.as_str().eq(&workflow_use.revision) => {
                seen.insert(workflow_use.key);
            }
            Some(record) => errors.push(format!(
                "{}:{} {} inventory SHA mismatch: workflow has {}, inventory has {}",
                workflow_use.key.workflow,
                workflow_use.line,
                workflow_use.key.action,
                workflow_use.revision,
                record.expected_revision
            )),
            None => errors.push(format!(
                "{}:{} {} missing inventory entry",
                workflow_use.key.workflow, workflow_use.line, workflow_use.key.action
            )),
        }
    }

    for key in inventory.keys() {
        if !seen.contains(key) {
            errors.push(format!(
                "{} {} stale inventory entry",
                key.workflow, key.action
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn load_inventory(path: &Path) -> Result<BTreeMap<InventoryKey, InventoryEntry>, Vec<String>> {
    let content = fs::read_to_string(path)
        .map_err(|error| vec![format!("failed to read {}: {error}", path.display())])?;
    let mut errors = Vec::new();
    let mut records = BTreeMap::new();

    for (index, raw_line) in content.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        let record = match serde_json::from_str::<InventoryRecord>(line) {
            Ok(record) => record,
            Err(error) => {
                errors.push(format!(
                    "{}:{line_number} invalid inventory JSON: {error}",
                    path.display()
                ));
                continue;
            }
        };

        errors.extend(validate_inventory_record(path, line_number, &record));
        let InventoryRecord {
            workflow,
            action,
            expected_revision,
            tag: _,
            source: _,
        } = record;
        let key = InventoryKey { workflow, action };
        let inventory_entry = InventoryEntry { expected_revision };

        match records.entry(key) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(inventory_entry);
            }
            std::collections::btree_map::Entry::Occupied(entry) => errors.push(format!(
                "{}:{line_number} duplicate inventory entry for {} in {}",
                path.display(),
                entry.key().action,
                entry.key().workflow
            )),
        }
    }

    if records.is_empty() {
        errors.push(format!("{} has no action pin entries", path.display()));
    }

    if errors.is_empty() {
        Ok(records)
    } else {
        Err(errors)
    }
}

fn validate_inventory_record(
    path: &Path,
    line_number: usize,
    record: &InventoryRecord,
) -> Vec<String> {
    let mut errors = Vec::new();

    if !record.workflow.starts_with(WORKFLOW_DIR_PREFIX) {
        errors.push(format!(
            "{}:{line_number} workflow must live under {WORKFLOW_DIR_PREFIX}: {}",
            path.display(),
            record.workflow
        ));
    }
    if !is_workflow_file(Path::new(&record.workflow)) {
        errors.push(format!(
            "{}:{line_number} workflow must be a .yml or .yaml file: {}",
            path.display(),
            record.workflow
        ));
    }
    if record.action.is_empty()
        || record.action.contains('@')
        || record.action.starts_with('.')
        || !record.action.contains('/')
    {
        errors.push(format!(
            "{}:{line_number} action must be an external owner/repo action: {}",
            path.display(),
            record.action
        ));
    }
    if !is_sha40_hex(&record.expected_revision) {
        errors.push(format!(
            "{}:{line_number} inventory SHA is not a 40-character hex value: {}",
            path.display(),
            record.expected_revision
        ));
    }
    if record.tag.trim().is_empty() {
        errors.push(format!(
            "{}:{line_number} tag must not be empty",
            path.display()
        ));
    }
    if record.source.trim().is_empty() {
        errors.push(format!(
            "{}:{line_number} source must not be empty",
            path.display()
        ));
    }

    errors
}

fn scan_workflows(repo_root: &Path) -> Result<Vec<WorkflowUse>, Vec<String>> {
    let workflows_dir = repo_root.join(WORKFLOW_DIR);
    let workflow_files = collect_workflow_files(&workflows_dir)?;
    let mut errors = Vec::new();
    let mut workflow_uses = Vec::new();

    for workflow_file in workflow_files {
        let workflow = workflow_file
            .strip_prefix(repo_root)
            .unwrap_or(&workflow_file)
            .to_string_lossy()
            .replace('\\', "/");
        let content = match fs::read_to_string(&workflow_file) {
            Ok(content) => content,
            Err(error) => {
                errors.push(format!(
                    "failed to read {}: {error}",
                    workflow_file.display()
                ));
                continue;
            }
        };

        for (index, line) in content.lines().enumerate() {
            let line_number = index + 1;
            let Some(value) = uses_value(line) else {
                continue;
            };
            if is_local_action_ref(value) {
                continue;
            }

            match parse_external_action_ref(value) {
                Ok((action, sha)) => workflow_uses.push(WorkflowUse {
                    key: InventoryKey {
                        workflow: workflow.clone(),
                        action: action.to_owned(),
                    },
                    revision: sha.to_owned(),
                    line: line_number,
                }),
                Err(error) => errors.push(format!("{workflow}:{line_number} {error}: {value}")),
            }
        }
    }

    if errors.is_empty() {
        Ok(workflow_uses)
    } else {
        Err(errors)
    }
}

fn collect_workflow_files(workflows_dir: &Path) -> Result<Vec<PathBuf>, Vec<String>> {
    let entries = fs::read_dir(workflows_dir).map_err(|error| {
        vec![format!(
            "failed to read workflow directory {}: {error}",
            workflows_dir.display()
        )]
    })?;
    let mut files = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| {
            vec![format!(
                "failed to inspect workflow directory {}: {error}",
                workflows_dir.display()
            )]
        })?;
        let workflow_file = entry.path();
        if is_workflow_file(&workflow_file) {
            files.extend(std::iter::once(workflow_file));
        }
    }

    files.sort();
    Ok(files)
}

fn uses_value(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let value = trimmed
        .strip_prefix("- uses:")
        .or_else(|| trimmed.strip_prefix("uses:"))?
        .trim();
    let value = value
        .split_once('#')
        .map_or(value, |(before_comment, _)| before_comment);
    let value = value.split_whitespace().next().unwrap_or("").trim();

    Some(strip_matching_quotes(value))
}

fn strip_matching_quotes(value: &str) -> &str {
    if let Some(value) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    {
        return value;
    }
    if let Some(value) = value
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
    {
        return value;
    }
    value
}

fn is_local_action_ref(value: &str) -> bool {
    value.starts_with("./") || value.starts_with("../")
}

fn parse_external_action_ref(value: &str) -> Result<(&str, &str), &'static str> {
    let (action, reference) = value
        .rsplit_once('@')
        .ok_or("external action is missing an @ reference")?;

    if action.is_empty() || reference.is_empty() || !action.contains('/') {
        return Err("external action must use owner/repo@sha syntax");
    }
    if !is_sha40_hex(reference) {
        return Err("external action is not pinned to a 40-character SHA");
    }

    Ok((action, reference))
}

fn is_workflow_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(std::ffi::OsStr::to_str),
        Some("yml" | "yaml")
    )
}

fn is_sha40_hex(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
const PIN_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
#[cfg(test)]
const PIN_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[cfg(test)]
struct PinFixture {
    temp_dir: tempfile::TempDir,
}

#[cfg(test)]
impl PinFixture {
    fn new() -> Result<Self, String> {
        Ok(Self {
            temp_dir: tempfile::TempDir::new()
                .map_err(|error| format!("failed to create temp fixture: {error}"))?,
        })
    }

    fn root(&self) -> &Path {
        self.temp_dir.path()
    }

    fn inventory_path(&self) -> PathBuf {
        self.root().join(INVENTORY_PATH)
    }

    fn write_workflow(&self, content: &str) -> Result<(), String> {
        let workflow_path = self.root().join(".github/workflows/example.yml");
        let parent = workflow_path
            .parent()
            .ok_or_else(|| "workflow path has no parent".to_owned())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create workflow fixture directory: {error}"))?;
        fs::write(workflow_path, content)
            .map_err(|error| format!("failed to write workflow fixture: {error}"))
    }

    fn write_inventory(&self, lines: &[String]) -> Result<(), String> {
        let inventory_path = self.inventory_path();
        let parent = inventory_path
            .parent()
            .ok_or_else(|| "inventory path has no parent".to_owned())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create inventory fixture directory: {error}"))?;
        fs::write(inventory_path, format!("{}\n", lines.join("\n")))
            .map_err(|error| format!("failed to write inventory fixture: {error}"))
    }
}

#[cfg(test)]
fn inventory_line(workflow: &str, action: &str, sha: &str) -> String {
    serde_json::json!({
        "workflow": workflow,
        "action": action,
        "sha": sha,
        "tag": "fixture-tag",
        "source": "fixture-source"
    })
    .to_string()
}

#[cfg(test)]
fn expect_verification_errors(fixture: &PinFixture) -> Result<Vec<String>, String> {
    match verify_action_pins(fixture.root(), &fixture.inventory_path()) {
        Ok(()) => Err("fixture should fail verification".to_owned()),
        Err(errors) => Ok(errors),
    }
}

#[cfg(test)]
fn require_error_contains(errors: &[String], needle: &str) -> Result<(), String> {
    if errors.iter().any(|error| error.contains(needle)) {
        Ok(())
    } else {
        Err(format!(
            "expected an error containing {needle:?}, got {errors:#?}"
        ))
    }
}
