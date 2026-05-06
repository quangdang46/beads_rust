//! Closure-time policy gates (issue #274 — Phase 1).
//!
//! Loads `.beads/policy.yaml` (when present) and evaluates configured gates
//! against a candidate close. Every gate is opt-in. With no policy file,
//! [`ClosePolicy::is_active`] returns `false` and `br close` behaves exactly
//! as it did before this module existed.
//!
//! # Phase 1 scope
//!
//! - Required-field validation (close reason min length / regex; unchecked
//!   acceptance criteria boxes)
//! - Actor constraints (forbid self-close after `in_progress` was set by the
//!   same actor)
//! - Agent attribution Tier 1 (capture, never reject) via env vars + CLI flags
//!
//! Phases 2-4 (typed references, closeout documents, signatures, full
//! observability) are intentionally out of scope.

use crate::error::{BeadsError, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Default file name for the policy document inside `.beads/`.
pub const POLICY_FILE_NAME: &str = "policy.yaml";

/// Environment variable for agent name (Tier 1 attribution).
pub const ENV_AGENT_NAME: &str = "BR_AGENT_NAME";
/// Environment variable for harness identifier.
pub const ENV_HARNESS: &str = "BR_HARNESS";
/// Environment variable for model identifier.
pub const ENV_MODEL: &str = "BR_MODEL";

/// Top-level policy document loaded from `.beads/policy.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PolicyDocument {
    /// Close-time gates.
    pub close_policy: ClosePolicy,
    /// When `false`, the `--bypass-policy` CLI flag is rejected. Defaults to
    /// `true` so projects retain the standard escape hatch.
    #[serde(default = "default_true")]
    pub allow_bypass: bool,
}

impl Default for PolicyDocument {
    fn default() -> Self {
        Self {
            close_policy: ClosePolicy::default(),
            allow_bypass: default_true(),
        }
    }
}

const fn default_true() -> bool {
    true
}

/// Close-time policy gates.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClosePolicy {
    /// Reject closes whose `--reason` text is shorter than `min_length` or
    /// fails the optional anchored regex.
    pub require_close_reason: RequireCloseReason,
    /// Reject closes when the issue body's `## Acceptance Criteria` section
    /// still has unchecked `- [ ]` items.
    pub require_acceptance_criteria_satisfied: ToggleGate,
    /// Reject closes when the closing actor matches the actor who last set
    /// status to `in_progress`.
    pub forbid_self_close_after_in_progress: ToggleGate,
    /// Tier 1 attribution capture (default: off).
    pub attribution: Attribution,
}

impl ClosePolicy {
    /// True when at least one gate is enabled. Used to short-circuit work for
    /// projects that have no policy.yaml or have all gates disabled.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.require_close_reason.enabled
            || self.require_acceptance_criteria_satisfied.enabled
            || self.forbid_self_close_after_in_progress.enabled
            || self.attribution.tier != AttributionTier::Off
    }
}

/// Bare on/off toggle.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ToggleGate {
    pub enabled: bool,
}

/// Required close-reason gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RequireCloseReason {
    pub enabled: bool,
    /// Minimum trimmed character count. `0` disables length checking.
    pub min_length: usize,
    /// Optional regex (anchored as the user wrote it). `None` disables.
    pub regex: Option<String>,
}

impl Default for RequireCloseReason {
    fn default() -> Self {
        Self {
            enabled: false,
            min_length: 20,
            regex: None,
        }
    }
}

/// Agent-attribution capture configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Attribution {
    pub tier: AttributionTier,
    /// Optional list of fields the project wants captured. When omitted, all
    /// known fields are captured. Phase 1 only honours
    /// `agent_name` / `harness` / `model`; unknown values are tolerated to
    /// keep forward compatibility for Tier 2/3 fields landing later.
    #[serde(default)]
    pub fields: Vec<String>,
}

/// Attribution tier. Phase 1 ships `Off` and `Capture` only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttributionTier {
    #[default]
    Off,
    Capture,
}

/// Agent attribution values resolved from CLI flags + env vars.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributionValues {
    pub agent_name: Option<String>,
    pub harness: Option<String>,
    pub model: Option<String>,
}

impl AttributionValues {
    /// Resolve attribution values using the precedence: CLI flag > env var > absent.
    #[must_use]
    pub fn resolve(
        cli_agent_name: Option<&str>,
        cli_harness: Option<&str>,
        cli_model: Option<&str>,
        env_lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Self {
        Self {
            agent_name: cli_agent_name
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .or_else(|| env_lookup(ENV_AGENT_NAME).filter(|s| !s.trim().is_empty())),
            harness: cli_harness
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .or_else(|| env_lookup(ENV_HARNESS).filter(|s| !s.trim().is_empty())),
            model: cli_model
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .or_else(|| env_lookup(ENV_MODEL).filter(|s| !s.trim().is_empty())),
        }
    }

    /// Resolve from process env. Convenience wrapper.
    #[must_use]
    pub fn resolve_from_env(
        cli_agent_name: Option<&str>,
        cli_harness: Option<&str>,
        cli_model: Option<&str>,
    ) -> Self {
        Self::resolve(cli_agent_name, cli_harness, cli_model, &|key| {
            std::env::var(key).ok()
        })
    }

    /// True when all attribution fields are absent.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.agent_name.is_none() && self.harness.is_none() && self.model.is_none()
    }
}

/// A single policy violation discovered while evaluating gates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyViolation {
    /// Stable machine identifier for the gate (e.g. `close_reason_min_length`).
    pub gate: String,
    /// Human-readable explanation. Always present.
    pub message: String,
    /// Optional structured detail (counts, items, expected vs actual).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

/// Issue-level evidence that gates evaluate against. Carved into a struct so
/// the evaluator stays pure and easy to test.
#[derive(Debug, Clone, Default)]
pub struct CloseEvidence<'a> {
    pub issue_id: &'a str,
    /// The reason text supplied via `--reason`. `None` when the user did not
    /// pass `--reason`.
    pub close_reason: Option<&'a str>,
    /// Issue body sections we scan for unchecked acceptance criteria.
    pub description: Option<&'a str>,
    pub design: Option<&'a str>,
    pub acceptance_criteria: Option<&'a str>,
    pub notes: Option<&'a str>,
    /// The actor performing the close.
    pub close_actor: &'a str,
    /// The actor who last set status to `in_progress` (if any).
    pub in_progress_actor: Option<&'a str>,
}

/// Evaluate every enabled gate against the supplied evidence.
///
/// Tier 1 attribution is intentionally NOT evaluated here: capture happens
/// regardless of any rejection logic, and Phase 1 explicitly never rejects
/// on missing attribution.
#[must_use]
pub fn evaluate(policy: &ClosePolicy, evidence: &CloseEvidence<'_>) -> Vec<PolicyViolation> {
    let mut violations = Vec::new();

    if policy.require_close_reason.enabled {
        evaluate_close_reason(&policy.require_close_reason, evidence, &mut violations);
    }

    if policy.require_acceptance_criteria_satisfied.enabled {
        evaluate_acceptance_criteria(evidence, &mut violations);
    }

    if policy.forbid_self_close_after_in_progress.enabled {
        evaluate_self_close(evidence, &mut violations);
    }

    violations
}

fn evaluate_close_reason(
    rule: &RequireCloseReason,
    evidence: &CloseEvidence<'_>,
    out: &mut Vec<PolicyViolation>,
) {
    let reason_text = evidence.close_reason.map(str::trim).unwrap_or("");
    let actual_len = reason_text.chars().count();

    if rule.min_length > 0 && actual_len < rule.min_length {
        out.push(PolicyViolation {
            gate: "close_reason_min_length".to_string(),
            message: format!(
                "close_reason fails policy: minimum length is {} chars (got {})",
                rule.min_length, actual_len
            ),
            detail: Some(serde_json::json!({
                "min_length": rule.min_length,
                "actual_length": actual_len,
                "issue_id": evidence.issue_id,
            })),
        });
    }

    if let Some(pattern) = rule.regex.as_deref() {
        match Regex::new(pattern) {
            Ok(re) => {
                if !re.is_match(reason_text) {
                    out.push(PolicyViolation {
                        gate: "close_reason_regex".to_string(),
                        message: format!(
                            "close_reason fails policy: must match pattern '{pattern}'"
                        ),
                        detail: Some(serde_json::json!({
                            "pattern": pattern,
                            "issue_id": evidence.issue_id,
                        })),
                    });
                }
            }
            Err(err) => {
                out.push(PolicyViolation {
                    gate: "close_reason_regex_invalid".to_string(),
                    message: format!(
                        "policy.yaml close_reason regex is invalid ('{pattern}'): {err}"
                    ),
                    detail: Some(serde_json::json!({
                        "pattern": pattern,
                        "parse_error": err.to_string(),
                    })),
                });
            }
        }
    }
}

fn evaluate_acceptance_criteria(evidence: &CloseEvidence<'_>, out: &mut Vec<PolicyViolation>) {
    // Pull from every body field so users with criteria in description (legacy)
    // are still gated. acceptance_criteria column is the canonical home.
    let candidates = [
        evidence.acceptance_criteria,
        evidence.description,
        evidence.design,
        evidence.notes,
    ];

    let mut unchecked: Vec<String> = Vec::new();
    for body in candidates.into_iter().flatten() {
        unchecked.extend(find_unchecked_acceptance_criteria(body));
    }
    // De-dupe across fields: copy/paste between description and
    // acceptance_criteria shouldn't double-count.
    unchecked.sort();
    unchecked.dedup();

    if !unchecked.is_empty() {
        let preview: Vec<String> = unchecked.iter().take(3).cloned().collect();
        let suffix = if unchecked.len() > preview.len() {
            format!(" (+{} more)", unchecked.len() - preview.len())
        } else {
            String::new()
        };
        out.push(PolicyViolation {
            gate: "acceptance_criteria_unchecked".to_string(),
            message: format!(
                "acceptance criteria policy: {} unchecked criteria remain: {}{}",
                unchecked.len(),
                preview.join("; "),
                suffix
            ),
            detail: Some(serde_json::json!({
                "unchecked_count": unchecked.len(),
                "unchecked_items": unchecked,
                "issue_id": evidence.issue_id,
            })),
        });
    }
}

fn evaluate_self_close(evidence: &CloseEvidence<'_>, out: &mut Vec<PolicyViolation>) {
    let Some(in_progress_actor) = evidence.in_progress_actor else {
        // No in_progress transition recorded — gate is satisfied by definition
        // (open → closed is unconstrained for this rule).
        return;
    };
    if in_progress_actor.is_empty() || evidence.close_actor.is_empty() {
        return;
    }
    if in_progress_actor == evidence.close_actor {
        out.push(PolicyViolation {
            gate: "forbid_self_close_after_in_progress".to_string(),
            message: format!(
                "actor policy: close.actor '{}' matches the actor who set in_progress; cross-validation required",
                evidence.close_actor
            ),
            detail: Some(serde_json::json!({
                "close_actor": evidence.close_actor,
                "in_progress_actor": in_progress_actor,
                "issue_id": evidence.issue_id,
            })),
        });
    }
}

/// Locate unchecked `- [ ]` items inside the canonical acceptance criteria
/// section. Recognises both an `## Acceptance Criteria` header (any heading
/// level) and a body that is itself the acceptance-criteria block (the case
/// when `acceptance_criteria` column carries the list directly).
#[must_use]
pub fn find_unchecked_acceptance_criteria(body: &str) -> Vec<String> {
    let body = body.trim();
    if body.is_empty() {
        return Vec::new();
    }

    let mut in_section = false;
    let mut found_header = false;
    let mut out: Vec<String> = Vec::new();

    // If the body has no markdown headers, treat the whole body as the
    // acceptance criteria section. acceptance_criteria column flow.
    let has_any_header = body.lines().any(|line| line.trim_start().starts_with('#'));
    if !has_any_header {
        in_section = true;
    }

    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            // Determine if this header opens / closes the acceptance criteria block.
            let header_text = rest.trim_start_matches('#').trim();
            let lower = header_text.to_ascii_lowercase();
            // Match a header that contains "acceptance criteria" (allows variants
            // like "Acceptance Criteria (Phase 1)").
            if lower.contains("acceptance criteria") {
                in_section = true;
                found_header = true;
                continue;
            }
            // Any other header closes the section once we have entered it.
            if found_header {
                in_section = false;
            }
            continue;
        }

        if !in_section {
            continue;
        }

        if let Some(item) = parse_unchecked_box(trimmed) {
            out.push(item);
        }
    }

    out
}

/// Parse a single line for an unchecked checkbox. Accepts `- [ ]`, `* [ ]`,
/// `+ [ ]`, optional leading whitespace, and any space (or lack thereof)
/// between the marker and the bracket.
fn parse_unchecked_box(line: &str) -> Option<String> {
    let mut chars = line.chars().peekable();
    let bullet = chars.next()?;
    if !matches!(bullet, '-' | '*' | '+') {
        return None;
    }
    // Skip whitespace between bullet and `[`.
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    if chars.next()? != '[' {
        return None;
    }
    // Inner char must be whitespace/empty for "unchecked".
    let inner = chars.next()?;
    let inner_is_unchecked = inner.is_whitespace() || inner == ' ';
    if !inner_is_unchecked {
        return None;
    }
    if chars.next()? != ']' {
        return None;
    }
    let rest: String = chars.collect();
    let rest = rest.trim().to_string();
    Some(rest)
}

/// Load the policy document from `.beads/policy.yaml`. Returns the default
/// (all gates off) when the file does not exist. Returns an error only if the
/// file exists but cannot be read or parsed — never silently downgrades a
/// broken config to "permissive."
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or parsed.
pub fn load_for_beads_dir(beads_dir: &Path) -> Result<PolicyDocument> {
    let path = beads_dir.join(POLICY_FILE_NAME);
    if !path.exists() {
        return Ok(PolicyDocument::default());
    }
    let raw = fs::read_to_string(&path).map_err(BeadsError::from)?;
    let document: PolicyDocument = serde_yml::from_str(&raw).map_err(|err| {
        BeadsError::Config(format!("failed to parse {}: {}", path.display(), err))
    })?;
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence_with_reason<'a>(reason: &'a str, issue_id: &'a str) -> CloseEvidence<'a> {
        CloseEvidence {
            issue_id,
            close_reason: Some(reason),
            close_actor: "alice",
            ..Default::default()
        }
    }

    #[test]
    fn default_policy_is_inactive() {
        let policy = ClosePolicy::default();
        assert!(!policy.is_active());
        let evidence = evidence_with_reason("anything", "bd-1");
        assert!(evaluate(&policy, &evidence).is_empty());
    }

    #[test]
    fn close_reason_min_length_rejects_short_reason() {
        let mut policy = ClosePolicy::default();
        policy.require_close_reason.enabled = true;
        policy.require_close_reason.min_length = 20;

        let violations = evaluate(&policy, &evidence_with_reason("too short", "bd-1"));
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].gate, "close_reason_min_length");
        assert!(violations[0].message.contains("minimum length is 20"));
        assert!(violations[0].message.contains("got 9"));
    }

    #[test]
    fn close_reason_min_length_counts_unicode_chars_not_bytes() {
        let mut policy = ClosePolicy::default();
        policy.require_close_reason.enabled = true;
        policy.require_close_reason.min_length = 4;

        // 4 emoji = 4 chars (16 bytes UTF-8). Must satisfy a 4-char minimum.
        let violations = evaluate(&policy, &evidence_with_reason("😀😀😀😀", "bd-1"));
        assert!(violations.is_empty(), "{:?}", violations);
    }

    #[test]
    fn close_reason_min_length_zero_disables_length_check() {
        let mut policy = ClosePolicy::default();
        policy.require_close_reason.enabled = true;
        policy.require_close_reason.min_length = 0;

        let violations = evaluate(&policy, &evidence_with_reason("", "bd-1"));
        assert!(violations.is_empty());
    }

    #[test]
    fn close_reason_min_length_treats_missing_reason_as_empty() {
        let mut policy = ClosePolicy::default();
        policy.require_close_reason.enabled = true;
        policy.require_close_reason.min_length = 5;

        let evidence = CloseEvidence {
            issue_id: "bd-1",
            close_reason: None,
            close_actor: "alice",
            ..Default::default()
        };
        let violations = evaluate(&policy, &evidence);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].gate, "close_reason_min_length");
    }

    #[test]
    fn close_reason_regex_rejects_non_match() {
        let mut policy = ClosePolicy::default();
        policy.require_close_reason.enabled = true;
        policy.require_close_reason.min_length = 0;
        policy.require_close_reason.regex = Some(r"^[A-Z][a-z]+: ".to_string());

        let violations = evaluate(&policy, &evidence_with_reason("done", "bd-1"));
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].gate, "close_reason_regex");
    }

    #[test]
    fn close_reason_regex_accepts_match() {
        let mut policy = ClosePolicy::default();
        policy.require_close_reason.enabled = true;
        policy.require_close_reason.min_length = 0;
        policy.require_close_reason.regex = Some(r"^Fix: ".to_string());

        let violations = evaluate(&policy, &evidence_with_reason("Fix: race in foo", "bd-1"));
        assert!(violations.is_empty());
    }

    #[test]
    fn close_reason_invalid_regex_surfaces_a_violation() {
        let mut policy = ClosePolicy::default();
        policy.require_close_reason.enabled = true;
        policy.require_close_reason.min_length = 0;
        policy.require_close_reason.regex = Some("(unclosed".to_string());

        let violations = evaluate(&policy, &evidence_with_reason("anything goes here", "bd-1"));
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].gate, "close_reason_regex_invalid");
    }

    #[test]
    fn acceptance_criteria_unchecked_blocks_close() {
        let mut policy = ClosePolicy::default();
        policy.require_acceptance_criteria_satisfied.enabled = true;
        let body = "## Acceptance Criteria\n- [x] First\n- [ ] Second\n- [ ] Third\n";
        let mut evidence = evidence_with_reason("done done done done done", "bd-1");
        evidence.description = Some(body);

        let violations = evaluate(&policy, &evidence);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].gate, "acceptance_criteria_unchecked");
        assert!(
            violations[0].message.contains("2 unchecked"),
            "{}",
            violations[0].message
        );
    }

    #[test]
    fn acceptance_criteria_passes_when_all_checked() {
        let mut policy = ClosePolicy::default();
        policy.require_acceptance_criteria_satisfied.enabled = true;
        let body = "## Acceptance Criteria\n- [x] First\n- [X] Second\n";
        let mut evidence = evidence_with_reason("done done done done done", "bd-1");
        evidence.description = Some(body);

        assert!(evaluate(&policy, &evidence).is_empty());
    }

    #[test]
    fn acceptance_criteria_only_scans_section_under_header() {
        let mut policy = ClosePolicy::default();
        policy.require_acceptance_criteria_satisfied.enabled = true;
        let body = "## Notes\n- [ ] random todo not under AC\n## Acceptance Criteria\n- [x] all good\n## Out of section\n- [ ] this is ignored\n";
        let mut evidence = evidence_with_reason("done done done done done", "bd-1");
        evidence.description = Some(body);

        assert!(
            evaluate(&policy, &evidence).is_empty(),
            "TODO outside AC section should NOT block close"
        );
    }

    #[test]
    fn acceptance_criteria_dedupes_across_fields() {
        let mut policy = ClosePolicy::default();
        policy.require_acceptance_criteria_satisfied.enabled = true;
        // Same item in description AND acceptance_criteria column — should
        // count once, not twice.
        let mut evidence = evidence_with_reason("done done done done done", "bd-1");
        evidence.description = Some("## Acceptance Criteria\n- [ ] First\n");
        evidence.acceptance_criteria = Some("- [ ] First\n");

        let violations = evaluate(&policy, &evidence);
        assert_eq!(violations.len(), 1);
        let detail = violations[0].detail.as_ref().unwrap();
        assert_eq!(detail["unchecked_count"], 1);
    }

    #[test]
    fn acceptance_criteria_handles_acceptance_criteria_column_without_header() {
        let mut policy = ClosePolicy::default();
        policy.require_acceptance_criteria_satisfied.enabled = true;
        let mut evidence = evidence_with_reason("done done done done done", "bd-1");
        // The acceptance_criteria column is dedicated — there's no `##` header.
        evidence.acceptance_criteria = Some("- [x] First\n- [ ] Second\n");

        let violations = evaluate(&policy, &evidence);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("Second"));
    }

    #[test]
    fn forbid_self_close_blocks_when_actors_match() {
        let mut policy = ClosePolicy::default();
        policy.forbid_self_close_after_in_progress.enabled = true;
        let mut evidence = evidence_with_reason("done done done done done", "bd-1");
        evidence.close_actor = "alice";
        evidence.in_progress_actor = Some("alice");

        let violations = evaluate(&policy, &evidence);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].gate, "forbid_self_close_after_in_progress");
    }

    #[test]
    fn forbid_self_close_passes_when_actors_differ() {
        let mut policy = ClosePolicy::default();
        policy.forbid_self_close_after_in_progress.enabled = true;
        let mut evidence = evidence_with_reason("done done done done done", "bd-1");
        evidence.close_actor = "alice";
        evidence.in_progress_actor = Some("bob");

        assert!(evaluate(&policy, &evidence).is_empty());
    }

    #[test]
    fn forbid_self_close_passes_when_no_in_progress_recorded() {
        let mut policy = ClosePolicy::default();
        policy.forbid_self_close_after_in_progress.enabled = true;
        let mut evidence = evidence_with_reason("done done done done done", "bd-1");
        evidence.close_actor = "alice";
        evidence.in_progress_actor = None;

        assert!(evaluate(&policy, &evidence).is_empty());
    }

    #[test]
    fn attribution_resolve_prefers_cli_over_env() {
        let env = |key: &str| match key {
            ENV_AGENT_NAME => Some("env-agent".to_string()),
            ENV_HARNESS => Some("env-harness".to_string()),
            ENV_MODEL => Some("env-model".to_string()),
            _ => None,
        };
        let values = AttributionValues::resolve(Some("cli-agent"), Some("cli-harness"), None, &env);
        assert_eq!(values.agent_name.as_deref(), Some("cli-agent"));
        assert_eq!(values.harness.as_deref(), Some("cli-harness"));
        // Falls back to env when CLI absent.
        assert_eq!(values.model.as_deref(), Some("env-model"));
    }

    #[test]
    fn attribution_resolve_treats_blank_strings_as_absent() {
        let env = |key: &str| {
            if key == ENV_HARNESS {
                Some("   ".to_string())
            } else {
                None
            }
        };
        let values = AttributionValues::resolve(Some(""), None, None, &env);
        assert!(values.agent_name.is_none());
        assert!(
            values.harness.is_none(),
            "blank env var should not populate"
        );
        assert!(values.model.is_none());
        assert!(values.is_empty());
    }

    #[test]
    fn multiple_gates_aggregate_violations() {
        let mut policy = ClosePolicy::default();
        policy.require_close_reason.enabled = true;
        policy.require_close_reason.min_length = 50;
        policy.forbid_self_close_after_in_progress.enabled = true;
        policy.require_acceptance_criteria_satisfied.enabled = true;

        let body = "## Acceptance Criteria\n- [ ] Outstanding\n";
        let mut evidence = evidence_with_reason("short", "bd-1");
        evidence.description = Some(body);
        evidence.close_actor = "alice";
        evidence.in_progress_actor = Some("alice");

        let violations = evaluate(&policy, &evidence);
        assert_eq!(violations.len(), 3);
        let gates: Vec<&str> = violations.iter().map(|v| v.gate.as_str()).collect();
        assert!(gates.contains(&"close_reason_min_length"));
        assert!(gates.contains(&"acceptance_criteria_unchecked"));
        assert!(gates.contains(&"forbid_self_close_after_in_progress"));
    }

    #[test]
    fn loader_returns_default_when_file_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let policy = load_for_beads_dir(dir.path()).expect("load");
        assert_eq!(policy, PolicyDocument::default());
        assert!(!policy.close_policy.is_active());
        assert!(policy.allow_bypass);
    }

    #[test]
    fn loader_parses_full_document() {
        let dir = tempfile::tempdir().expect("tempdir");
        let yaml = r#"
close_policy:
  require_close_reason:
    enabled: true
    min_length: 30
    regex: "^Fix: "
  require_acceptance_criteria_satisfied:
    enabled: true
  forbid_self_close_after_in_progress:
    enabled: true
  attribution:
    tier: capture
    fields: ["agent_name", "harness", "model"]
allow_bypass: false
"#;
        std::fs::write(dir.path().join(POLICY_FILE_NAME), yaml).unwrap();
        let policy = load_for_beads_dir(dir.path()).expect("load");
        assert!(policy.close_policy.require_close_reason.enabled);
        assert_eq!(policy.close_policy.require_close_reason.min_length, 30);
        assert_eq!(
            policy.close_policy.require_close_reason.regex.as_deref(),
            Some("^Fix: ")
        );
        assert!(
            policy
                .close_policy
                .require_acceptance_criteria_satisfied
                .enabled
        );
        assert!(
            policy
                .close_policy
                .forbid_self_close_after_in_progress
                .enabled
        );
        assert_eq!(
            policy.close_policy.attribution.tier,
            AttributionTier::Capture
        );
        assert!(!policy.allow_bypass);
        assert!(policy.close_policy.is_active());
    }

    #[test]
    fn loader_rejects_unknown_top_level_keys() {
        let dir = tempfile::tempdir().expect("tempdir");
        let yaml = "unknown_key: 1\n";
        std::fs::write(dir.path().join(POLICY_FILE_NAME), yaml).unwrap();
        let err = load_for_beads_dir(dir.path()).unwrap_err();
        match err {
            BeadsError::Config(msg) => assert!(msg.contains("unknown_key"), "{msg}"),
            other => panic!("expected Config error, got {other:?}"),
        }
    }

    #[test]
    fn loader_accepts_empty_document() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(POLICY_FILE_NAME), "{}\n").unwrap();
        let policy = load_for_beads_dir(dir.path()).expect("load");
        assert_eq!(policy, PolicyDocument::default());
    }

    #[test]
    fn parse_unchecked_box_recognises_variants() {
        assert_eq!(
            parse_unchecked_box("- [ ] todo item").as_deref(),
            Some("todo item")
        );
        assert_eq!(
            parse_unchecked_box("* [ ] starred").as_deref(),
            Some("starred")
        );
        assert_eq!(parse_unchecked_box("+ [ ] plus").as_deref(), Some("plus"));
        assert!(parse_unchecked_box("- [x] checked").is_none());
        assert!(parse_unchecked_box("- [X] checked").is_none());
        assert!(parse_unchecked_box("plain text").is_none());
        assert!(parse_unchecked_box("- not a box").is_none());
    }
}
