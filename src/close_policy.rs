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
//! - Typed structured references in close reasons
//!
//! Long-form closeout documents, signatures, and full observability are
//! intentionally out of scope.

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
///
/// Close-policy structs intentionally accept unknown fields rather than
/// hard-failing the parse: a single typo in `policy.yaml` used to take
/// down `br close` for every operator on the project, with no recovery
/// path (even `--bypass-policy` couldn't help because the parse fires
/// before bypass logic runs). See beads_rust#302.
///
/// Unknown fields surface via [`load_for_beads_dir`], which emits a
/// `tracing::warn!` listing every unknown key it discovered. Operators
/// who want strict parsing back can wire up a future `--strict-policy`
/// flag (out of scope for the #302 fix).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PolicyDocument {
    /// Close-time gates.
    pub close_policy: ClosePolicy,
    /// Status-workflow policy (issue #311). Optional; when the `workflow:`
    /// section is absent the default is permissive (no enforcement), so
    /// existing repos are unaffected.
    pub workflow: Workflow,
    /// When `false`, the `--bypass-policy` CLI flag is rejected. Defaults to
    /// `true` so projects retain the standard escape hatch.
    #[serde(default = "default_true")]
    pub allow_bypass: bool,
}

impl Default for PolicyDocument {
    fn default() -> Self {
        Self {
            close_policy: ClosePolicy::default(),
            workflow: Workflow::default(),
            allow_bypass: default_true(),
        }
    }
}

const fn default_true() -> bool {
    true
}

/// Close-time policy gates.
///
/// Unknown fields are tolerated and surfaced via `tracing::warn!` at load
/// time (see `PolicyDocument` doc-comment and beads_rust#302).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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
    /// Reject closes when the issue has a `blocks` edge to a dependent that
    /// is currently in `deferred` status (beads_rust#303). Closing a prereq
    /// is the natural touch-point that forces the closer to confront every
    /// deferred dependent before the prereq disappears from the graph.
    pub forbid_close_with_deferred_dependents: ToggleGate,
    /// Tier 1 attribution capture (default: off).
    pub attribution: Attribution,
    /// Typed structured references gate (capability #3 of #274). When
    /// `enabled`, the close `--reason` must contain at least one
    /// `kind:value` reference matching one of `required_kinds` (e.g.
    /// `commit:`, `pr:`, `reviewer:`, `investigation:`). Unknown kinds
    /// satisfy the gate only when explicitly listed in `required_kinds`.
    pub require_typed_references: RequireTypedReferences,
}

impl ClosePolicy {
    /// True when at least one gate is enabled. Used to short-circuit work for
    /// projects that have no policy.yaml or have all gates disabled.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.require_close_reason.enabled
            || self.require_acceptance_criteria_satisfied.enabled
            || self.forbid_self_close_after_in_progress.enabled
            || self.forbid_close_with_deferred_dependents.enabled
            || self.require_typed_references.enabled
            || self.attribution.tier != AttributionTier::Off
    }
}

/// Status-workflow policy (issue #311).
///
/// The `workflow:` namespace owns everything about the allowed status set and
/// (in the future, issue #312) transitions and per-transition gates. It is
/// deliberately a self-contained section so #312's `transitions:` /
/// per-transition gate config can hang off the same block without reworking
/// the schema:
///
/// ```yaml
/// workflow:
///   strict: true
///   statuses: [open, in_progress, blocked, deferred, draft, closed]
///   transitions:                       # issue #312, layer 1
///     open: [in_progress, deferred, closed]
///     in_progress: [in_review, blocked, open]
///     in_review: [closed, in_progress]
///     blocked: [open, in_progress]
///     deferred: [open]
///     # initial: [open, draft]         # statuses allowed when current is unknown
///     # any: [closed]                  # to-statuses allowed from EVERY from-status
/// ```
///
/// When the section is absent the default (`strict: false`, empty `statuses`,
/// empty `transitions`) means no enforcement, matching pre-#311 behavior
/// exactly.
///
/// Transition enforcement (issue #312, layer 1) is opt-in via a non-empty
/// `transitions:` map and is independently gated on `strict`: when
/// `strict: true` *and* `transitions` is non-empty, a status change whose
/// `from -> to` pair is not listed is rejected on `br update`. Absent or
/// empty `transitions` leaves transition behavior exactly as before.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Workflow {
    /// When `true` *and* `statuses` is non-empty, a status outside the
    /// configured set is rejected on `create`/`update` and flagged by
    /// `br doctor`. When `false`, `statuses` is advisory only (no
    /// enforcement). The same flag gates transition enforcement (see
    /// `transitions`).
    pub strict: bool,
    /// The allowed status set. Each entry is a canonical-or-custom status
    /// string (e.g. `open`, `in_progress`, `closed`, or a project-specific
    /// value). Empty disables enforcement even when `strict` is `true`.
    #[serde(default)]
    pub statuses: Vec<String>,
    /// Allowed status transitions (issue #312, layer 1). A map of
    /// `from-status -> [allowed to-statuses]`. Two reserved keys widen the
    /// rules:
    ///
    /// - `any` — its to-statuses are allowed *from every* from-status (a
    ///   wildcard source; e.g. allow `closed` from anywhere).
    /// - `initial` — the to-statuses allowed when there is no recorded
    ///   current status (e.g. a `create`, or an issue whose status the caller
    ///   could not resolve). Absent `initial` means any initial status is
    ///   accepted, since there is no `from` state to validate against.
    ///
    /// Comparison is case-insensitive, mirroring `statuses`. A no-op
    /// transition (`from == to`) is always allowed and never consults the
    /// map. Empty map disables transition enforcement entirely (backward
    /// compatible).
    #[serde(default)]
    pub transitions: std::collections::BTreeMap<String, Vec<String>>,
}

/// Reserved `transitions` key whose to-statuses are allowed from every
/// from-status (wildcard source).
pub const TRANSITION_ANY_FROM: &str = "any";
/// Reserved `transitions` key whose to-statuses are allowed when there is no
/// recorded current status (e.g. a create, or an unresolved current status).
pub const TRANSITION_INITIAL: &str = "initial";

impl Workflow {
    /// True when strict enforcement is configured: `strict` is on *and* at
    /// least one allowed status is listed. Enforcement and the doctor
    /// detector short-circuit on `false`.
    #[must_use]
    pub fn is_enforced(&self) -> bool {
        self.strict && !self.statuses.is_empty()
    }

    /// True when `status` (case-insensitively) is in the configured set.
    /// Comparison mirrors [`crate::model::Status`] parsing: canonical names
    /// are matched case-insensitively, so a config entry of `In_Progress`
    /// still admits the canonical `in_progress`.
    #[must_use]
    pub fn allows(&self, status: &str) -> bool {
        let target = status.to_lowercase();
        self.statuses
            .iter()
            .any(|allowed| allowed.to_lowercase() == target)
    }

    /// Comma-separated, source-order list of the allowed statuses for error
    /// messages. Empty string when nothing is configured.
    #[must_use]
    pub fn allowed_list(&self) -> String {
        self.statuses.join(", ")
    }

    /// Validate a target status against the workflow policy. Returns `Ok(())`
    /// when enforcement is off, the status set is empty, or the status is in
    /// the set. Returns a [`BeadsError::Validation`] naming the allowed values
    /// otherwise.
    ///
    /// # Errors
    ///
    /// Returns a validation error when strict enforcement is configured and
    /// `status` is not in the allowed set.
    pub fn validate_status(&self, status: &str) -> Result<()> {
        if !self.is_enforced() || self.allows(status) {
            return Ok(());
        }
        Err(BeadsError::validation(
            "status",
            format!(
                "status '{status}' is not permitted by the project workflow policy \
                 (.beads/policy.yaml workflow.strict). Allowed statuses: {}.",
                self.allowed_list()
            ),
        ))
    }

    /// True when transition enforcement is configured: `strict` is on *and*
    /// at least one `from -> [to...]` rule is listed. Enforcement
    /// short-circuits on `false`.
    #[must_use]
    pub fn transitions_enforced(&self) -> bool {
        self.strict && !self.transitions.is_empty()
    }

    /// Case-insensitive lookup of the to-statuses listed for `from`. Returns
    /// `None` when `from` has no entry in the map.
    fn transitions_from(&self, from: &str) -> Option<&Vec<String>> {
        let target = from.to_lowercase();
        self.transitions
            .iter()
            .find(|(key, _)| key.to_lowercase() == target)
            .map(|(_, tos)| tos)
    }

    /// Collect every to-status reachable from `from`, merging the explicit
    /// `from` entry with the wildcard `any` entry. Returns the source-order,
    /// de-duplicated list used for error messages and membership checks.
    #[must_use]
    pub fn allowed_targets_from(&self, from: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let push_unique = |list: &Vec<String>, out: &mut Vec<String>| {
            for value in list {
                if !out.iter().any(|seen| seen.eq_ignore_ascii_case(value)) {
                    out.push(value.clone());
                }
            }
        };
        if let Some(explicit) = self.transitions_from(from) {
            push_unique(explicit, &mut out);
        }
        if let Some(wildcard) = self.transitions_from(TRANSITION_ANY_FROM) {
            push_unique(wildcard, &mut out);
        }
        out
    }

    /// True when moving `from -> to` is permitted by the configured
    /// transition rules. A no-op (`from == to`, case-insensitive) is always
    /// allowed. Otherwise the target must appear either under the explicit
    /// `from` key or under the wildcard `any` key.
    #[must_use]
    pub fn allows_transition(&self, from: &str, to: &str) -> bool {
        if from.eq_ignore_ascii_case(to) {
            return true;
        }
        self.allowed_targets_from(from)
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(to))
    }

    /// True when `to` is permitted as an initial status (no recorded `from`).
    /// When no `initial` key is configured, any initial status is accepted —
    /// there is no prior state to validate against.
    #[must_use]
    pub fn allows_initial(&self, to: &str) -> bool {
        match self.transitions_from(TRANSITION_INITIAL) {
            None => true,
            Some(allowed) => allowed.iter().any(|s| s.eq_ignore_ascii_case(to)),
        }
    }

    /// Validate a status *change* against the workflow transition rules.
    ///
    /// `from` is the issue's current status, or `None` when there is no
    /// recorded current status (a create, or an unresolved current status —
    /// validated against the reserved `initial` key).
    ///
    /// Returns `Ok(())` when transition enforcement is off, the move is a
    /// no-op, or the move is permitted. Returns a [`BeadsError::Validation`]
    /// naming the current status, the attempted status, and the valid next
    /// statuses otherwise — mirroring the `validate_status` error style.
    ///
    /// # Errors
    ///
    /// Returns a validation error when transition enforcement is configured
    /// and the `from -> to` move is not in the allowed set.
    pub fn validate_transition(&self, from: Option<&str>, to: &str) -> Result<()> {
        if !self.transitions_enforced() {
            return Ok(());
        }

        match from {
            None => {
                if self.allows_initial(to) {
                    return Ok(());
                }
                let allowed = self
                    .transitions_from(TRANSITION_INITIAL)
                    .map(|tos| tos.join(", "))
                    .unwrap_or_default();
                Err(BeadsError::validation(
                    "status",
                    format!(
                        "initial status '{to}' is not permitted by the project workflow policy \
                         (.beads/policy.yaml workflow.transitions, key 'initial'). \
                         Allowed initial statuses: {allowed}."
                    ),
                ))
            }
            Some(from) => {
                if self.allows_transition(from, to) {
                    return Ok(());
                }
                let allowed = self.allowed_targets_from(from);
                let allowed_list = if allowed.is_empty() {
                    "(none)".to_string()
                } else {
                    allowed.join(", ")
                };
                Err(BeadsError::validation(
                    "status",
                    format!(
                        "transition '{from}' -> '{to}' is not permitted by the project workflow \
                         policy (.beads/policy.yaml workflow.transitions). \
                         Valid next statuses from '{from}': {allowed_list}."
                    ),
                ))
            }
        }
    }
}

/// Typed-references gate (capability #3 of issue #274).
///
/// When `enabled`, the close reason must contain at least one
/// `kind:value` token matching one of `required_kinds`. Built-in kinds are
/// `commit`, `pr`, `reviewer`, `investigation`, `agent-mail`, and `dashboard`;
/// projects can add custom kinds by listing them in `required_kinds`.
///
/// The matcher is lenient: the kind/value pair can appear anywhere
/// in the reason text (start, end, embedded in prose) as long as
/// it's a contiguous `kind:value` token with no whitespace in the
/// value. We deliberately don't require URL-like values — the point
/// is queryability, not URL validation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequireTypedReferences {
    pub enabled: bool,
    /// Reference kinds the close reason must contain (logical OR — any one
    /// satisfies the gate). When empty, the gate accepts any built-in
    /// reference kind; custom kinds must be listed explicitly.
    #[serde(default)]
    pub required_kinds: Vec<String>,
}

const BUILTIN_TYPED_REFERENCE_KINDS: &[&str] = &[
    "commit",
    "pr",
    "reviewer",
    "investigation",
    "agent-mail",
    "dashboard",
];

/// Bare on/off toggle.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToggleGate {
    pub enabled: bool,
}

/// Required close-reason gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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
#[serde(default)]
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

    if policy.require_typed_references.enabled {
        evaluate_typed_references(&policy.require_typed_references, evidence, &mut violations);
    }

    violations
}

/// Match `kind:value` tokens inside a close-reason string. We require
/// the value to be non-empty and not start with whitespace — this
/// rules out accidental matches like `note: foo` in prose. The kind
/// itself must start with a lowercase ASCII letter and may then contain
/// lowercase ASCII letters / hyphens / digits, which
/// matches the canonical `commit:` / `pr:` / `agent-mail:` shapes
/// the issue calls out.
fn extract_typed_references(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Find start of a candidate kind: alpha character preceded
        // either by start-of-string, whitespace, or a punctuation
        // boundary character (so we don't pick up `xyz` in `foo-xyz:`).
        let preceding_ok =
            i == 0 || matches!(bytes[i - 1], b' ' | b'\n' | b'\t' | b'(' | b'[' | b',');
        if !preceding_ok {
            i += 1;
            continue;
        }
        if !bytes[i].is_ascii_lowercase() {
            i += 1;
            continue;
        }
        let kind_start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_lowercase() || bytes[i] == b'-' || bytes[i].is_ascii_digit())
        {
            i += 1;
        }
        if i == kind_start || i >= bytes.len() || bytes[i] != b':' {
            i += 1;
            continue;
        }
        let kind = &text[kind_start..i];
        i += 1; // skip the ':'
        let value_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b',' {
            i += 1;
        }
        let value = &text[value_start..i];
        if !value.is_empty() && kind.len() >= 2 {
            out.push((kind.to_string(), value.to_string()));
        }
    }
    out
}

fn required_typed_reference_description(rule: &RequireTypedReferences) -> String {
    if rule.required_kinds.is_empty() {
        BUILTIN_TYPED_REFERENCE_KINDS.join(", ")
    } else {
        rule.required_kinds.join(", ")
    }
}

fn typed_reference_kind_satisfies_rule(kind: &str, rule: &RequireTypedReferences) -> bool {
    if rule.required_kinds.is_empty() {
        return BUILTIN_TYPED_REFERENCE_KINDS.contains(&kind);
    }
    rule.required_kinds.iter().any(|required| required == kind)
}

fn evaluate_typed_references(
    rule: &RequireTypedReferences,
    evidence: &CloseEvidence<'_>,
    out: &mut Vec<PolicyViolation>,
) {
    let reason_text = evidence.close_reason.unwrap_or("");
    let refs = extract_typed_references(reason_text);
    let required_description = required_typed_reference_description(rule);

    if refs.is_empty() {
        out.push(PolicyViolation {
            gate: "typed_references_required".to_string(),
            message: format!(
                "close_reason has no typed references; policy requires at least one of: {}",
                required_description
            ),
            detail: Some(serde_json::json!({
                "required_kinds": &rule.required_kinds,
                "issue_id": evidence.issue_id,
            })),
        });
        return;
    }

    let found_kinds: Vec<&str> = refs
        .iter()
        .map(|(kind, _)| kind.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let satisfied = found_kinds
        .iter()
        .any(|kind| typed_reference_kind_satisfies_rule(kind, rule));
    if !satisfied {
        out.push(PolicyViolation {
            gate: "typed_references_required_kind_missing".to_string(),
            message: format!(
                "close_reason has typed refs ({}) but none satisfy the required kinds: {}",
                found_kinds.join(", "),
                required_description
            ),
            detail: Some(serde_json::json!({
                "required_kinds": &rule.required_kinds,
                "found_kinds": found_kinds,
                "issue_id": evidence.issue_id,
            })),
        });
    }
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

/// Gate identifier emitted by [`deferred_dependents_violation`] and recorded
/// in close metadata. Mirrors the `policy.yaml` key for grep-ability.
pub const GATE_FORBID_CLOSE_WITH_DEFERRED_DEPENDENTS: &str =
    "forbid_close_with_deferred_dependents";

/// Build the policy violation for the `forbid_close_with_deferred_dependents`
/// gate (beads_rust#303), given the issue being closed and the IDs of its
/// dependents (issues with a `blocks` edge *from* `issue_id`) that are
/// currently in `deferred` status.
///
/// Returns `None` when there are no deferred dependents (gate satisfied).
/// The storage-backed dependent lookup lives in the close command — this
/// function is the pure, unit-testable message/shape builder so the gate's
/// error contract is covered without a database.
///
/// The error names every offending dependent ID and instructs the closer to
/// either reopen each (`br update <dep> --status=open`) or close-as-superseded
/// with a `duplicate_of` edge before closing the prereq.
#[must_use]
pub fn deferred_dependents_violation(
    issue_id: &str,
    deferred_dependent_ids: &[String],
) -> Option<PolicyViolation> {
    if deferred_dependent_ids.is_empty() {
        return None;
    }

    // Stable, de-duplicated ordering so the message and detail are
    // deterministic regardless of query/iteration order.
    let mut ids: Vec<String> = deferred_dependent_ids.to_vec();
    ids.sort();
    ids.dedup();

    let id_list = ids.join(", ");
    let message = format!(
        "deferred-dependents policy: cannot close {issue_id}: it has {count} deferred dependent(s): {id_list}. \
         Reopen each (`br update <dep> --status=open`) or close-as-superseded with a duplicate_of edge \
         before closing {issue_id}.",
        count = ids.len(),
    );

    Some(PolicyViolation {
        gate: GATE_FORBID_CLOSE_WITH_DEFERRED_DEPENDENTS.to_string(),
        message,
        detail: Some(serde_json::json!({
            "issue_id": issue_id,
            "deferred_dependents": ids,
            "deferred_dependent_count": ids.len(),
        })),
    })
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
    let has_any_header = has_markdown_heading_outside_fences(body);
    if !has_any_header {
        in_section = true;
    }

    let mut fence_marker = None;
    for line in body.lines() {
        if update_code_fence(line, &mut fence_marker) || fence_marker.is_some() {
            continue;
        }
        let trimmed = line.trim_start();
        if let Some(header_text) = markdown_heading_text(trimmed) {
            // Determine if this header opens / closes the acceptance criteria block.
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

fn has_markdown_heading_outside_fences(body: &str) -> bool {
    let mut fence_marker = None;
    for line in body.lines() {
        if update_code_fence(line, &mut fence_marker) || fence_marker.is_some() {
            continue;
        }
        if markdown_heading_text(line).is_some() {
            return true;
        }
    }
    false
}

fn update_code_fence(line: &str, fence_marker: &mut Option<char>) -> bool {
    let trimmed = line.trim_start();
    let Some(marker @ ('`' | '~')) = trimmed.chars().next() else {
        return false;
    };
    let marker_len = trimmed.chars().take_while(|ch| *ch == marker).count();
    if marker_len < 3 {
        return false;
    }

    if fence_marker.is_some_and(|open_marker| open_marker == marker) {
        *fence_marker = None;
    } else if fence_marker.is_none() {
        *fence_marker = Some(marker);
    }
    true
}

fn markdown_heading_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let level = trimmed
        .as_bytes()
        .iter()
        .take_while(|byte| **byte == b'#')
        .count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = trimmed.get(level..)?;
    if rest.chars().next().is_some_and(|ch| !ch.is_whitespace()) {
        return None;
    }
    Some(rest.trim())
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
/// # Unknown fields (beads_rust#302)
///
/// Close-policy structs deliberately accept unknown fields rather than
/// hard-failing the parse. A typo or project-local experimental gate
/// previously took down `br close` for every operator on the project,
/// with `--bypass-policy` powerless to recover because the parse fires
/// before bypass logic runs. We now warn instead of erroring; the trade
/// is loss of typo-at-parse-time detection, but the cost (full project
/// close-pathway outage from one typo) was much worse.
///
/// Unknown fields are surfaced exactly once per load via
/// [`detect_unknown_policy_fields`] and emitted as a `tracing::warn!`
/// event. The warning lists every unknown path with a dotted scope
/// (e.g. `close_policy.require_new_experimental_field`) so operators
/// can find typos without re-reading the file.
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

    // Re-parse the raw YAML into a free-form value tree so we can diff it
    // against the typed schema and surface unknown fields without failing
    // the load. Failure to re-parse as a `Value` is impossible here (the
    // typed parse above already succeeded), but if it ever did, we'd
    // rather skip the warning than spurious-error the load — that's the
    // whole point of #302.
    if let Ok(raw_value) = serde_yml::from_str::<serde_yml::Value>(&raw) {
        let unknown = detect_unknown_policy_fields(&raw_value);
        if !unknown.is_empty() {
            tracing::warn!(
                policy_path = %path.display(),
                unknown_fields = ?unknown,
                "policy.yaml contains {} unknown field(s) under close_policy structs; \
                 these were ignored (beads_rust#302). Check for typos: {}",
                unknown.len(),
                unknown.join(", "),
            );
        }
    }

    Ok(document)
}

/// Walk a parsed `policy.yaml` value tree and collect dotted paths to any
/// keys not recognized by the typed close-policy schema.
///
/// We use a hard-coded recursive walk (rather than `serde(flatten)` with
/// `extras` fields on every struct) so the typed public API stays simple
/// and the extras maps don't leak into round-trip serialization. Adding
/// a new canonical field becomes a one-line update in [`PolicyNode`].
///
/// Returns a sorted, de-duplicated list of dotted paths
/// (e.g. `["close_policy.require_new_experimental_field"]`). Empty when
/// the document only uses canonical fields.
#[must_use]
pub fn detect_unknown_policy_fields(root: &serde_yml::Value) -> Vec<String> {
    let mut unknown = Vec::new();
    walk_policy_node(root, PolicyNode::Document, "", &mut unknown);
    unknown.sort();
    unknown.dedup();
    unknown
}

/// Schema-tree node used by [`detect_unknown_policy_fields`] to recognise
/// which keys are canonical at each depth of `policy.yaml`. Leaves (`Scalar`)
/// terminate the walk; mappings descend per the `key -> child-node` table.
#[derive(Clone, Copy, Debug)]
enum PolicyNode {
    /// Top-level `policy.yaml` mapping.
    Document,
    /// `close_policy:` block.
    ClosePolicy,
    /// `close_policy.require_close_reason:` block.
    RequireCloseReason,
    /// Any plain `{enabled: bool}` toggle gate.
    ToggleGate,
    /// `close_policy.attribution:` block.
    Attribution,
    /// `close_policy.require_typed_references:` block.
    RequireTypedReferences,
    /// `workflow:` block (issue #311).
    Workflow,
    /// Terminal scalar / list — descent stops here.
    Scalar,
}

impl PolicyNode {
    /// Canonical keys at this depth, plus the child node each key descends
    /// into. Keys absent from this table are reported as unknown.
    const fn child_table(self) -> &'static [(&'static str, Self)] {
        match self {
            Self::Document => &[
                ("close_policy", Self::ClosePolicy),
                ("workflow", Self::Workflow),
                ("allow_bypass", Self::Scalar),
            ],
            Self::ClosePolicy => &[
                ("require_close_reason", Self::RequireCloseReason),
                ("require_acceptance_criteria_satisfied", Self::ToggleGate),
                ("forbid_self_close_after_in_progress", Self::ToggleGate),
                ("forbid_close_with_deferred_dependents", Self::ToggleGate),
                ("attribution", Self::Attribution),
                ("require_typed_references", Self::RequireTypedReferences),
            ],
            Self::RequireCloseReason => &[
                ("enabled", Self::Scalar),
                ("min_length", Self::Scalar),
                ("regex", Self::Scalar),
            ],
            Self::ToggleGate => &[("enabled", Self::Scalar)],
            Self::Attribution => &[("tier", Self::Scalar), ("fields", Self::Scalar)],
            Self::RequireTypedReferences => {
                &[("enabled", Self::Scalar), ("required_kinds", Self::Scalar)]
            }
            Self::Workflow => &[
                ("strict", Self::Scalar),
                ("statuses", Self::Scalar),
                ("transitions", Self::Scalar),
            ],
            Self::Scalar => &[],
        }
    }
}

fn walk_policy_node(
    value: &serde_yml::Value,
    node: PolicyNode,
    scope: &str,
    out: &mut Vec<String>,
) {
    if matches!(node, PolicyNode::Scalar) {
        return;
    }
    let Some(map) = value.as_mapping() else {
        return;
    };
    let table = node.child_table();
    for (key, sub) in map {
        let Some(key_str) = key.as_str() else {
            continue;
        };
        let path = if scope.is_empty() {
            key_str.to_string()
        } else {
            format!("{scope}.{key_str}")
        };
        match table.iter().find(|(k, _)| *k == key_str) {
            Some((_, child)) => walk_policy_node(sub, *child, &path, out),
            None => out.push(path),
        }
    }
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
    fn acceptance_criteria_does_not_treat_hash_references_as_section_headers() {
        let mut policy = ClosePolicy::default();
        policy.require_acceptance_criteria_satisfied.enabled = true;
        let body =
            "## Acceptance Criteria\n#123 tracks the rollout\n- [ ] Finish after the reference\n";
        let mut evidence = evidence_with_reason("done done done done done", "bd-1");
        evidence.description = Some(body);

        let violations = evaluate(&policy, &evidence);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].gate, "acceptance_criteria_unchecked");
        assert!(violations[0].message.contains("Finish after the reference"));
    }

    #[test]
    fn acceptance_criteria_ignores_section_headers_inside_fenced_code() {
        let mut policy = ClosePolicy::default();
        policy.require_acceptance_criteria_satisfied.enabled = true;
        let body = "## Notes\n```markdown\n## Acceptance Criteria\n- [ ] Example only\n```\n## Acceptance Criteria\n- [x] Real criterion\n";
        let mut evidence = evidence_with_reason("done done done done done", "bd-1");
        evidence.description = Some(body);

        assert!(
            evaluate(&policy, &evidence).is_empty(),
            "unchecked boxes inside fenced examples should not block close"
        );
    }

    #[test]
    fn acceptance_criteria_ignores_unchecked_boxes_inside_fenced_code() {
        let mut policy = ClosePolicy::default();
        policy.require_acceptance_criteria_satisfied.enabled = true;
        let body = "## Acceptance Criteria\n- [x] Real criterion\n```sh\n- [ ] Example only\n```\n";
        let mut evidence = evidence_with_reason("done done done done done", "bd-1");
        evidence.description = Some(body);

        assert!(
            evaluate(&policy, &evidence).is_empty(),
            "unchecked boxes inside fenced examples should not block close"
        );
    }

    #[test]
    fn acceptance_criteria_without_markdown_headers_scans_hash_prefixed_body() {
        let mut policy = ClosePolicy::default();
        policy.require_acceptance_criteria_satisfied.enabled = true;
        let mut evidence = evidence_with_reason("done done done done done", "bd-1");
        evidence.acceptance_criteria = Some("#123 follow-up\n- [ ] Finish referenced work\n");

        let violations = evaluate(&policy, &evidence);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].gate, "acceptance_criteria_unchecked");
        assert!(violations[0].message.contains("Finish referenced work"));
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

    // =========================================================================
    // Deferred-dependents gate (beads_rust#303)
    // =========================================================================

    #[test]
    fn deferred_dependents_gate_default_off() {
        let policy = ClosePolicy::default();
        assert!(!policy.forbid_close_with_deferred_dependents.enabled);
        assert!(!policy.is_active());
    }

    #[test]
    fn deferred_dependents_gate_makes_policy_active_when_enabled() {
        let policy = ClosePolicy {
            forbid_close_with_deferred_dependents: ToggleGate { enabled: true },
            ..Default::default()
        };
        assert!(policy.is_active());
    }

    #[test]
    fn deferred_dependents_violation_none_when_empty() {
        assert!(deferred_dependents_violation("bd-1", &[]).is_none());
    }

    #[test]
    fn deferred_dependents_violation_names_offending_ids() {
        let violation =
            deferred_dependents_violation("bd-1", &["bd-3".to_string(), "bd-2".to_string()])
                .expect("violation expected");
        assert_eq!(violation.gate, GATE_FORBID_CLOSE_WITH_DEFERRED_DEPENDENTS);
        // IDs are sorted deterministically and both are named.
        assert!(violation.message.contains("bd-2"), "{}", violation.message);
        assert!(violation.message.contains("bd-3"), "{}", violation.message);
        assert!(
            violation.message.contains("2 deferred dependent"),
            "{}",
            violation.message
        );
        // Remediation guidance is present.
        assert!(
            violation.message.contains("br update <dep> --status=open"),
            "{}",
            violation.message
        );
        assert!(
            violation.message.contains("duplicate_of"),
            "{}",
            violation.message
        );

        let detail = violation.detail.as_ref().unwrap();
        assert_eq!(detail["issue_id"], "bd-1");
        assert_eq!(detail["deferred_dependent_count"], 2);
        assert_eq!(
            detail["deferred_dependents"],
            serde_json::json!(["bd-2", "bd-3"])
        );
    }

    #[test]
    fn deferred_dependents_violation_dedupes_ids() {
        let violation = deferred_dependents_violation(
            "bd-1",
            &["bd-2".to_string(), "bd-2".to_string(), "bd-3".to_string()],
        )
        .expect("violation expected");
        let detail = violation.detail.as_ref().unwrap();
        assert_eq!(detail["deferred_dependent_count"], 2);
        assert!(violation.message.contains("2 deferred dependent"));
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
  require_typed_references:
    enabled: true
    required_kinds: ["commit", "reviewer"]
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
        assert!(policy.close_policy.require_typed_references.enabled);
        assert_eq!(
            policy.close_policy.require_typed_references.required_kinds,
            vec!["commit".to_string(), "reviewer".to_string()]
        );
        assert_eq!(
            policy.close_policy.attribution.tier,
            AttributionTier::Capture
        );
        assert!(!policy.allow_bypass);
        assert!(policy.close_policy.is_active());
    }

    /// beads_rust#302: unknown fields used to hard-fail and take down `br
    /// close` project-wide. They are now tolerated — the parse succeeds and
    /// the unknown keys surface via [`detect_unknown_policy_fields`].
    #[test]
    fn loader_tolerates_unknown_top_level_keys() {
        let dir = tempfile::tempdir().expect("tempdir");
        let yaml = "unknown_key: 1\nclose_policy:\n  require_close_reason:\n    enabled: true\n";
        std::fs::write(dir.path().join(POLICY_FILE_NAME), yaml).unwrap();
        let policy = load_for_beads_dir(dir.path()).expect("load must succeed");
        assert!(
            policy.close_policy.require_close_reason.enabled,
            "known fields must still parse"
        );

        let raw: serde_yml::Value = serde_yml::from_str(yaml).unwrap();
        let unknown = detect_unknown_policy_fields(&raw);
        assert_eq!(unknown, vec!["unknown_key".to_string()]);
    }

    /// beads_rust#302: unknown fields nested under `close_policy:` (the
    /// regression class the issue specifically called out — a typo or
    /// experimental gate) must also be tolerated and surfaced as a dotted
    /// path so operators can find them.
    #[test]
    fn loader_tolerates_unknown_field_under_close_policy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let yaml = r"
close_policy:
  require_close_reason:
    enabled: true
    min_length: 20
  require_new_experimental_field:
    enabled: true
";
        std::fs::write(dir.path().join(POLICY_FILE_NAME), yaml).unwrap();
        let policy = load_for_beads_dir(dir.path()).expect("load must succeed");
        assert!(policy.close_policy.require_close_reason.enabled);
        assert_eq!(policy.close_policy.require_close_reason.min_length, 20);

        let raw: serde_yml::Value = serde_yml::from_str(yaml).unwrap();
        let unknown = detect_unknown_policy_fields(&raw);
        assert_eq!(
            unknown,
            vec!["close_policy.require_new_experimental_field".to_string()]
        );
    }

    /// Typos buried deeper (under known sub-blocks) also surface with
    /// their full dotted path.
    #[test]
    fn detect_unknown_policy_fields_walks_nested_structs() {
        let yaml = r#"
close_policy:
  require_close_reason:
    enabled: true
    min_lenght: 20            # typo: should be min_length
  attribution:
    tier: capture
    fileds: ["agent_name"]    # typo: should be fields
"#;
        let raw: serde_yml::Value = serde_yml::from_str(yaml).unwrap();
        let unknown = detect_unknown_policy_fields(&raw);
        assert_eq!(
            unknown,
            vec![
                "close_policy.attribution.fileds".to_string(),
                "close_policy.require_close_reason.min_lenght".to_string(),
            ]
        );
    }

    /// A fully-canonical document produces no unknown-field reports.
    #[test]
    fn detect_unknown_policy_fields_is_empty_for_canonical_doc() {
        let yaml = r#"
allow_bypass: false
close_policy:
  require_close_reason:
    enabled: true
    min_length: 30
    regex: "^Fix: "
  require_acceptance_criteria_satisfied:
    enabled: true
  forbid_self_close_after_in_progress:
    enabled: true
  require_typed_references:
    enabled: true
    required_kinds: ["commit", "reviewer"]
  attribution:
    tier: capture
    fields: ["agent_name", "harness", "model"]
"#;
        let raw: serde_yml::Value = serde_yml::from_str(yaml).unwrap();
        assert!(detect_unknown_policy_fields(&raw).is_empty());
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

    // =========================================================================
    // Typed-references gate (capability #3 of issue #274)
    // =========================================================================

    #[test]
    fn extract_typed_references_finds_kind_value_pairs() {
        let refs = extract_typed_references("Fixed in commit:abc123 per reviewer:bob");
        assert_eq!(
            refs,
            vec![
                ("commit".to_string(), "abc123".to_string()),
                ("reviewer".to_string(), "bob".to_string()),
            ]
        );
    }

    #[test]
    fn extract_typed_references_handles_hyphenated_kinds() {
        let refs = extract_typed_references("see agent-mail:thread-xyz for context");
        assert_eq!(
            refs,
            vec![("agent-mail".to_string(), "thread-xyz".to_string())]
        );
    }

    #[test]
    fn extract_typed_references_skips_prose_with_colons() {
        // `note:` followed by whitespace is prose, not a typed reference.
        let refs = extract_typed_references("note: this is a regular sentence");
        assert!(refs.is_empty(), "got {refs:?}");
    }

    #[test]
    fn extract_typed_references_requires_letter_start() {
        let refs = extract_typed_references("bad refs: 12:abc and -kind:value");
        assert!(refs.is_empty(), "got {refs:?}");
    }

    #[test]
    fn typed_references_gate_rejects_when_none_present() {
        let policy = ClosePolicy {
            require_typed_references: RequireTypedReferences {
                enabled: true,
                required_kinds: vec![],
            },
            ..Default::default()
        };
        let evidence = evidence_with_reason("just plain prose with no refs at all", "bd-1");
        let violations = evaluate(&policy, &evidence);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].gate, "typed_references_required");
    }

    #[test]
    fn typed_references_gate_empty_required_kinds_accepts_builtin_kind() {
        let policy = ClosePolicy {
            require_typed_references: RequireTypedReferences {
                enabled: true,
                required_kinds: vec![],
            },
            ..Default::default()
        };
        let evidence = evidence_with_reason("Fixed in reviewer:bob", "bd-1");
        assert!(evaluate(&policy, &evidence).is_empty());
    }

    #[test]
    fn typed_references_gate_empty_required_kinds_rejects_unknown_kind() {
        let policy = ClosePolicy {
            require_typed_references: RequireTypedReferences {
                enabled: true,
                required_kinds: vec![],
            },
            ..Default::default()
        };
        let evidence = evidence_with_reason("Captured in tracker:ABC-123", "bd-1");
        let violations = evaluate(&policy, &evidence);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].gate, "typed_references_required_kind_missing");
    }

    #[test]
    fn typed_references_gate_empty_required_kinds_rejects_bare_url() {
        let policy = ClosePolicy {
            require_typed_references: RequireTypedReferences {
                enabled: true,
                required_kinds: vec![],
            },
            ..Default::default()
        };
        let evidence = evidence_with_reason("Evidence: https://example.invalid/report", "bd-1");
        let violations = evaluate(&policy, &evidence);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].gate, "typed_references_required_kind_missing");
    }

    #[test]
    fn typed_references_gate_accepts_when_kind_matches() {
        let policy = ClosePolicy {
            require_typed_references: RequireTypedReferences {
                enabled: true,
                required_kinds: vec!["commit".to_string()],
            },
            ..Default::default()
        };
        let evidence = evidence_with_reason("Fixed in commit:abc12345", "bd-1");
        assert!(evaluate(&policy, &evidence).is_empty());
    }

    #[test]
    fn typed_references_gate_rejects_wrong_kind() {
        let policy = ClosePolicy {
            require_typed_references: RequireTypedReferences {
                enabled: true,
                required_kinds: vec!["commit".to_string()],
            },
            ..Default::default()
        };
        let evidence = evidence_with_reason("see investigation:linear-XYZ-42 for details", "bd-1");
        let violations = evaluate(&policy, &evidence);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].gate, "typed_references_required_kind_missing");
    }

    /// Drift guard for beads_rust#302: `PolicyNode::child_table()` is a
    /// hand-maintained mirror of the typed close-policy struct fields. If a
    /// new field is added to one of the structs without also being added
    /// to the table, `detect_unknown_policy_fields` will fire a
    /// **false-positive** "unknown field" warning on every canonical
    /// document containing that field. The owner explicitly acknowledged
    /// this sync hazard in the commit message — this test makes the drift
    /// impossible to ship: it serialises a `Default` instance of every
    /// close-policy struct and asserts the produced key set is a subset of
    /// the corresponding `PolicyNode`'s `child_table()` keys.
    ///
    /// We assert "subset" rather than "equality" because table keys may
    /// intentionally list `Option<T>` fields that serialise to nothing in
    /// the default form (none today, but future-proofing).
    #[test]
    fn policy_node_child_table_covers_every_typed_struct_field() {
        fn field_names_of<T: serde::Serialize + Default>() -> Vec<String> {
            let value =
                serde_yml::to_value(T::default()).expect("default struct must serialise to value");
            let mapping = value
                .as_mapping()
                .expect("default struct must serialise as a mapping");
            mapping
                .iter()
                .filter_map(|(k, _)| k.as_str().map(String::from))
                .collect()
        }

        fn assert_table_covers(node: PolicyNode, struct_fields: &[String], struct_name: &str) {
            let table_keys: std::collections::HashSet<&'static str> =
                node.child_table().iter().map(|(k, _)| *k).collect();
            for field in struct_fields {
                assert!(
                    table_keys.contains(field.as_str()),
                    "PolicyNode::{node:?}::child_table() is missing key `{field}` declared on \
                     struct `{struct_name}`. `detect_unknown_policy_fields` would emit a \
                     FALSE-POSITIVE 'unknown field' warning on every canonical policy.yaml that \
                     uses this field. Add the entry to `child_table()` (see beads_rust#302).",
                );
            }
        }

        assert_table_covers(
            PolicyNode::Document,
            &field_names_of::<PolicyDocument>(),
            "PolicyDocument",
        );
        assert_table_covers(
            PolicyNode::ClosePolicy,
            &field_names_of::<ClosePolicy>(),
            "ClosePolicy",
        );
        assert_table_covers(
            PolicyNode::RequireCloseReason,
            &field_names_of::<RequireCloseReason>(),
            "RequireCloseReason",
        );
        assert_table_covers(
            PolicyNode::ToggleGate,
            &field_names_of::<ToggleGate>(),
            "ToggleGate",
        );
        assert_table_covers(
            PolicyNode::Attribution,
            &field_names_of::<Attribution>(),
            "Attribution",
        );
        assert_table_covers(
            PolicyNode::RequireTypedReferences,
            &field_names_of::<RequireTypedReferences>(),
            "RequireTypedReferences",
        );
        assert_table_covers(
            PolicyNode::Workflow,
            &field_names_of::<Workflow>(),
            "Workflow",
        );
    }

    /// Inverse drift guard: every key listed in `PolicyNode::child_table()`
    /// must correspond to an actual field on the typed struct. Otherwise a
    /// stale entry would silently SUPPRESS the unknown-field warning for a
    /// field that no longer exists (false negative: typo in YAML matches a
    /// dead table entry → no warning, but the field is also not honoured by
    /// the typed parse).
    ///
    /// `regex: Option<String>` is in the default serialised mapping as a
    /// null entry, so it counts as "present" for this check.
    #[test]
    fn policy_node_child_table_has_no_stale_entries() {
        fn field_names_of<T: serde::Serialize + Default>() -> std::collections::HashSet<String> {
            let value =
                serde_yml::to_value(T::default()).expect("default struct must serialise to value");
            let mapping = value
                .as_mapping()
                .expect("default struct must serialise as a mapping");
            mapping
                .iter()
                .filter_map(|(k, _)| k.as_str().map(String::from))
                .collect()
        }

        fn assert_no_stale(
            node: PolicyNode,
            struct_fields: &std::collections::HashSet<String>,
            struct_name: &str,
        ) {
            for (key, _) in node.child_table() {
                assert!(
                    struct_fields.contains(*key),
                    "PolicyNode::{node:?}::child_table() lists key `{key}` that does not exist \
                     on struct `{struct_name}`. A typo of this key in policy.yaml would NOT be \
                     reported as unknown even though it is silently ignored by the typed parse \
                     (see beads_rust#302).",
                );
            }
        }

        assert_no_stale(
            PolicyNode::Document,
            &field_names_of::<PolicyDocument>(),
            "PolicyDocument",
        );
        assert_no_stale(
            PolicyNode::ClosePolicy,
            &field_names_of::<ClosePolicy>(),
            "ClosePolicy",
        );
        assert_no_stale(
            PolicyNode::RequireCloseReason,
            &field_names_of::<RequireCloseReason>(),
            "RequireCloseReason",
        );
        assert_no_stale(
            PolicyNode::ToggleGate,
            &field_names_of::<ToggleGate>(),
            "ToggleGate",
        );
        assert_no_stale(
            PolicyNode::Attribution,
            &field_names_of::<Attribution>(),
            "Attribution",
        );
        assert_no_stale(
            PolicyNode::RequireTypedReferences,
            &field_names_of::<RequireTypedReferences>(),
            "RequireTypedReferences",
        );
        assert_no_stale(
            PolicyNode::Workflow,
            &field_names_of::<Workflow>(),
            "Workflow",
        );
    }

    // =========================================================================
    // Status-workflow policy (issue #311)
    // =========================================================================

    fn strict_workflow() -> Workflow {
        Workflow {
            strict: true,
            statuses: vec![
                "open".to_string(),
                "in_progress".to_string(),
                "closed".to_string(),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn workflow_default_is_not_enforced() {
        let workflow = Workflow::default();
        assert!(!workflow.is_enforced());
        // With enforcement off every status is permitted.
        assert!(workflow.validate_status("anything-at-all").is_ok());
    }

    #[test]
    fn workflow_strict_but_empty_statuses_is_not_enforced() {
        let workflow = Workflow {
            strict: true,
            statuses: vec![],
            ..Default::default()
        };
        assert!(!workflow.is_enforced());
        assert!(workflow.validate_status("bogus").is_ok());
    }

    #[test]
    fn workflow_rejects_status_outside_the_set() {
        let workflow = strict_workflow();
        let err = workflow
            .validate_status("completed")
            .expect_err("out-of-set status must be rejected");
        let message = err.to_string();
        assert!(message.contains("completed"), "{message}");
        // The error names the allowed values so the user can self-correct.
        assert!(message.contains("open"), "{message}");
        assert!(message.contains("in_progress"), "{message}");
        assert!(message.contains("closed"), "{message}");
    }

    #[test]
    fn workflow_allows_status_in_the_set() {
        let workflow = strict_workflow();
        assert!(workflow.validate_status("open").is_ok());
        assert!(workflow.validate_status("in_progress").is_ok());
        assert!(workflow.validate_status("closed").is_ok());
    }

    #[test]
    fn workflow_status_match_is_case_insensitive() {
        let workflow = Workflow {
            strict: true,
            statuses: vec!["In_Progress".to_string()],
            ..Default::default()
        };
        assert!(workflow.allows("in_progress"));
        assert!(workflow.validate_status("in_progress").is_ok());
    }

    #[test]
    fn workflow_supports_custom_statuses() {
        let workflow = Workflow {
            strict: true,
            statuses: vec!["open".to_string(), "in_review".to_string()],
            ..Default::default()
        };
        assert!(workflow.validate_status("in_review").is_ok());
        assert!(workflow.validate_status("blocked").is_err());
    }

    #[test]
    fn loader_parses_workflow_section() {
        let dir = tempfile::tempdir().expect("tempdir");
        let yaml = r#"
workflow:
  strict: true
  statuses: ["open", "in_progress", "closed"]
"#;
        std::fs::write(dir.path().join(POLICY_FILE_NAME), yaml).unwrap();
        let policy = load_for_beads_dir(dir.path()).expect("load");
        assert!(policy.workflow.is_enforced());
        assert_eq!(
            policy.workflow.statuses,
            vec![
                "open".to_string(),
                "in_progress".to_string(),
                "closed".to_string()
            ]
        );
        assert!(policy.workflow.validate_status("open").is_ok());
        assert!(policy.workflow.validate_status("completed").is_err());
    }

    #[test]
    fn loader_absent_workflow_section_is_permissive() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A policy.yaml that configures close gates but NO workflow section
        // must not enforce any status set.
        let yaml = "close_policy:\n  forbid_self_close_after_in_progress:\n    enabled: true\n";
        std::fs::write(dir.path().join(POLICY_FILE_NAME), yaml).unwrap();
        let policy = load_for_beads_dir(dir.path()).expect("load");
        assert!(!policy.workflow.is_enforced());
        assert!(policy.workflow.validate_status("whatever").is_ok());
    }

    #[test]
    fn detect_unknown_policy_fields_walks_workflow_typos() {
        let yaml = r#"
workflow:
  strict: true
  statusses: ["open"]   # typo: should be statuses
"#;
        let raw: serde_yml::Value = serde_yml::from_str(yaml).unwrap();
        let unknown = detect_unknown_policy_fields(&raw);
        assert_eq!(unknown, vec!["workflow.statusses".to_string()]);
    }

    #[test]
    fn detect_unknown_policy_fields_accepts_canonical_workflow() {
        let yaml = r#"
workflow:
  strict: true
  statuses: ["open", "closed"]
"#;
        let raw: serde_yml::Value = serde_yml::from_str(yaml).unwrap();
        assert!(detect_unknown_policy_fields(&raw).is_empty());
    }

    // =========================================================================
    // Status-transition rules (issue #312, layer 1)
    // =========================================================================

    fn transition_workflow() -> Workflow {
        let mut transitions = std::collections::BTreeMap::new();
        transitions.insert(
            "open".to_string(),
            vec!["in_progress".to_string(), "closed".to_string()],
        );
        transitions.insert(
            "in_progress".to_string(),
            vec!["in_review".to_string(), "blocked".to_string()],
        );
        transitions.insert("in_review".to_string(), vec!["closed".to_string()]);
        Workflow {
            strict: true,
            statuses: vec![],
            transitions,
        }
    }

    #[test]
    fn transitions_default_is_not_enforced() {
        let workflow = Workflow::default();
        assert!(!workflow.transitions_enforced());
        // With enforcement off every transition is permitted.
        assert!(workflow.validate_transition(Some("open"), "bogus").is_ok());
        assert!(workflow.validate_transition(None, "bogus").is_ok());
    }

    #[test]
    fn transitions_present_but_not_strict_is_not_enforced() {
        let mut workflow = transition_workflow();
        workflow.strict = false;
        assert!(!workflow.transitions_enforced());
        // Not strict => transitions advisory only, nothing rejected.
        assert!(
            workflow
                .validate_transition(Some("open"), "deferred")
                .is_ok()
        );
    }

    #[test]
    fn transitions_strict_but_empty_map_is_not_enforced() {
        let workflow = Workflow {
            strict: true,
            statuses: vec![],
            transitions: std::collections::BTreeMap::new(),
        };
        assert!(!workflow.transitions_enforced());
        assert!(
            workflow
                .validate_transition(Some("open"), "blocked")
                .is_ok()
        );
    }

    #[test]
    fn transition_valid_move_is_allowed() {
        let workflow = transition_workflow();
        assert!(workflow.allows_transition("open", "in_progress"));
        assert!(
            workflow
                .validate_transition(Some("open"), "in_progress")
                .is_ok()
        );
        assert!(
            workflow
                .validate_transition(Some("in_progress"), "in_review")
                .is_ok()
        );
        assert!(
            workflow
                .validate_transition(Some("in_review"), "closed")
                .is_ok()
        );
    }

    #[test]
    fn transition_invalid_move_is_rejected_with_actionable_error() {
        let workflow = transition_workflow();
        let err = workflow
            .validate_transition(Some("open"), "in_review")
            .expect_err("open -> in_review is not configured");
        let message = err.to_string();
        // Names current, attempted, and the valid next statuses.
        assert!(message.contains("'open'"), "{message}");
        assert!(message.contains("'in_review'"), "{message}");
        assert!(message.contains("in_progress"), "{message}");
        assert!(message.contains("closed"), "{message}");
        assert!(message.contains("workflow.transitions"), "{message}");
    }

    #[test]
    fn transition_from_status_with_no_rule_rejects_with_none_targets() {
        let workflow = transition_workflow();
        // `blocked` has no entry and there is no `any` wildcard.
        let err = workflow
            .validate_transition(Some("blocked"), "open")
            .expect_err("blocked has no allowed targets");
        let message = err.to_string();
        assert!(message.contains("(none)"), "{message}");
    }

    #[test]
    fn transition_no_op_is_always_allowed() {
        let workflow = transition_workflow();
        // Same status, even when there is no explicit self-loop rule.
        assert!(workflow.validate_transition(Some("open"), "open").is_ok());
        // Even for a status with no rule at all.
        assert!(
            workflow
                .validate_transition(Some("blocked"), "blocked")
                .is_ok()
        );
    }

    #[test]
    fn transition_is_case_insensitive() {
        let workflow = transition_workflow();
        assert!(
            workflow
                .validate_transition(Some("OPEN"), "In_Progress")
                .is_ok()
        );
    }

    #[test]
    fn transition_any_wildcard_allows_target_from_every_status() {
        let mut workflow = transition_workflow();
        workflow
            .transitions
            .insert("any".to_string(), vec!["deferred".to_string()]);
        // `deferred` now allowed from any from-status, including ones with
        // their own rules and ones with none.
        assert!(
            workflow
                .validate_transition(Some("open"), "deferred")
                .is_ok()
        );
        assert!(
            workflow
                .validate_transition(Some("blocked"), "deferred")
                .is_ok()
        );
        // Wildcard targets are merged into the error message's "valid next".
        let err = workflow
            .validate_transition(Some("open"), "bogus")
            .expect_err("bogus is not reachable");
        assert!(err.to_string().contains("deferred"), "{err}");
    }

    #[test]
    fn transition_initial_key_restricts_creates() {
        let mut workflow = transition_workflow();
        workflow.transitions.insert(
            "initial".to_string(),
            vec!["open".to_string(), "draft".to_string()],
        );
        // No prior status => validated against `initial`.
        assert!(workflow.validate_transition(None, "open").is_ok());
        assert!(workflow.validate_transition(None, "draft").is_ok());
        let err = workflow
            .validate_transition(None, "in_progress")
            .expect_err("in_progress is not an allowed initial status");
        let message = err.to_string();
        assert!(message.contains("initial"), "{message}");
        assert!(message.contains("'in_progress'"), "{message}");
        assert!(message.contains("open"), "{message}");
        assert!(message.contains("draft"), "{message}");
    }

    #[test]
    fn transition_absent_initial_key_accepts_any_initial_status() {
        // `transition_workflow()` has no `initial` key — any starting status
        // is accepted since there is no prior state to validate against.
        let workflow = transition_workflow();
        assert!(workflow.validate_transition(None, "open").is_ok());
        assert!(workflow.validate_transition(None, "anything").is_ok());
    }

    #[test]
    fn allowed_targets_from_dedupes_and_merges_wildcard() {
        let mut workflow = transition_workflow();
        // Overlap between explicit `open` targets and the wildcard.
        workflow.transitions.insert(
            "any".to_string(),
            vec!["closed".to_string(), "deferred".to_string()],
        );
        let targets = workflow.allowed_targets_from("open");
        // explicit: in_progress, closed; wildcard adds deferred (closed deduped).
        assert_eq!(
            targets,
            vec![
                "in_progress".to_string(),
                "closed".to_string(),
                "deferred".to_string()
            ]
        );
    }

    #[test]
    fn loader_parses_workflow_transitions_section() {
        let dir = tempfile::tempdir().expect("tempdir");
        let yaml = r"
workflow:
  strict: true
  transitions:
    open: [in_progress, deferred, closed]
    in_progress: [in_review, blocked, open]
    any: [closed]
    initial: [open, draft]
";
        std::fs::write(dir.path().join(POLICY_FILE_NAME), yaml).unwrap();
        let policy = load_for_beads_dir(dir.path()).expect("load");
        let workflow = &policy.workflow;
        assert!(workflow.transitions_enforced());
        assert!(
            workflow
                .validate_transition(Some("open"), "in_progress")
                .is_ok()
        );
        assert!(
            workflow
                .validate_transition(Some("open"), "in_review")
                .is_err()
        );
        // `any: [closed]` allows close from a from-status without an explicit rule.
        assert!(
            workflow
                .validate_transition(Some("blocked"), "closed")
                .is_ok()
        );
        // `initial` gates creates.
        assert!(workflow.validate_transition(None, "open").is_ok());
        assert!(workflow.validate_transition(None, "closed").is_err());
    }

    #[test]
    fn loader_absent_transitions_is_not_enforced() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Strict status enforcement but NO transitions map: status moves are
        // unconstrained (backward compatible with #311-only configs).
        let yaml = "workflow:\n  strict: true\n  statuses: [open, in_progress, closed]\n";
        std::fs::write(dir.path().join(POLICY_FILE_NAME), yaml).unwrap();
        let policy = load_for_beads_dir(dir.path()).expect("load");
        assert!(policy.workflow.is_enforced());
        assert!(!policy.workflow.transitions_enforced());
        assert!(
            policy
                .workflow
                .validate_transition(Some("closed"), "open")
                .is_ok()
        );
    }

    #[test]
    fn detect_unknown_policy_fields_accepts_canonical_transitions() {
        let yaml = r"
workflow:
  strict: true
  transitions:
    open: [in_progress]
";
        let raw: serde_yml::Value = serde_yml::from_str(yaml).unwrap();
        assert!(detect_unknown_policy_fields(&raw).is_empty());
    }
}
