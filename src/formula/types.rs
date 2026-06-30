//! Core types for the Formula Language (workflow-as-code engine).
//!
//! Ported from Go beads /internal/formula/types.go.
//! Formulas are high-level workflow templates that compile down to issues.
//! They support variable definitions, step hierarchies, composition via
//! bond points, and async gates for inter-agent coordination.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Formula types
// ---------------------------------------------------------------------------

/// Categorizes formulas by their purpose.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FormulaType {
    /// Standard workflow template (sequence of steps).
    Workflow,
    /// Macro that expands into multiple steps (test + lint + build).
    Expansion,
    /// Cross-cutting concern applied to other formulas (logging, approval gates).
    Aspect,
    /// Multi-agent workflow coordinating parallel workers (code review, design review).
    Convoy,
}

impl FormulaType {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Workflow | Self::Expansion | Self::Aspect | Self::Convoy)
    }
}

impl Default for FormulaType {
    fn default() -> Self {
        Self::Workflow
    }
}

// ---------------------------------------------------------------------------
// Formula — root structure for .formula.json files
// ---------------------------------------------------------------------------

/// Root structure for `.formula.json` / `.formula.toml` files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Formula {
    /// Unique identifier/name for this formula (convention: mol-<name>).
    pub formula: String,

    /// Human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Schema version (currently 1).
    #[serde(default = "default_version")]
    pub version: i32,

    /// Categorizes the formula.
    #[serde(default)]
    pub r#type: FormulaType,

    /// Parent formulas to inherit from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extends: Vec<String>,

    /// Template variables with defaults and validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vars: Option<Vec<VarDef>>,

    /// Steps defining the work items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<Step>>,

    /// Expansion template steps (for TypeExpansion formulas).
    /// Uses {target} and {target.description} placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<Vec<Step>>,

    /// Composition / bonding rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compose: Option<ComposeRules>,

    /// Step transformations (before/after/around).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advice: Option<Vec<AdviceRule>>,

    /// Target patterns for aspect formulas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointcuts: Option<Vec<Pointcut>>,

    /// Recommended instantiation phase: "liquid" (pour) or "vapor" (wisp).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,

    /// Whether steps are materialized as individual child issues.
    #[serde(default)]
    pub pour: bool,

    /// Source file path (set by parser, not serialized).
    #[serde(skip)]
    pub source: Option<String>,
}

const fn default_version() -> i32 {
    1
}

// ---------------------------------------------------------------------------
// VarDef — template variable definitions
// ---------------------------------------------------------------------------

/// Defines a template variable with optional validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VarDef {
    /// Variable name (set from map key, not serialized).
    #[serde(skip)]
    pub name: String,

    /// Human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Default value. Nil means no default (must be provided if referenced).
    /// Non-nil means the variable has an explicit default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,

    /// Indicates the variable must be provided (no default).
    #[serde(default)]
    pub required: bool,

    /// Allowed values (if non-empty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub r#enum: Vec<String>,

    /// Regex pattern the value must match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,

    /// Expected value type: string (default), int, bool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
}

// ---------------------------------------------------------------------------
// Step — a work item defined in a formula
// ---------------------------------------------------------------------------

/// Defines a work item to create when the formula is instantiated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Unique identifier within this formula (used for dependency refs and bond points).
    pub id: String,

    /// Issue title (supports {{variable}} substitution).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Issue description (supports substitution).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Additional notes (supports substitution).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,

    /// Issue type: task, bug, feature, epic, chore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,

    /// Issue priority (0-4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,

    /// Labels applied to the created issue.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,

    /// Metadata carried through to the created issue's metadata field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,

    /// Step IDs this step blocks on (within the formula).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,

    /// Simpler alias for DependsOn — sibling step IDs that must complete first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<String>,

    /// Fanout gate type: "all-children" or "any-children".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waits_for: Option<String>,

    /// Default assignee (supports substitution).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,

    /// References an expansion formula to inline here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expand: Option<String>,

    /// Variable overrides for the expansion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expand_vars: Option<std::collections::HashMap<String, String>>,

    /// Condition making this step optional based on a variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,

    /// Nested steps (for creating epic hierarchies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Step>>,

    /// Async wait condition for this step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,

    /// Loop iteration spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#loop: Option<LoopSpec>,

    /// Actions triggered when this step completes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_complete: Option<OnCompleteSpec>,

    /// Source formula name (internal, set during parsing).
    #[serde(skip)]
    pub source_formula: Option<String>,

    /// Source location path (internal, set during parsing).
    #[serde(skip)]
    pub source_location: Option<String>,
}

// ---------------------------------------------------------------------------
// Gate — async wait condition
// ---------------------------------------------------------------------------

/// Defines an async wait condition for formula steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gate {
    /// Condition type: gh:run, gh:pr, timer, human, mail.
    pub r#type: String,

    /// Condition identifier (e.g., workflow name for gh:run).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Runtime condition identifier (maps to Issue.AwaitID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub await_id: Option<String>,

    /// Timeout (e.g., "1h", "24h").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
}

// ---------------------------------------------------------------------------
// LoopSpec — iteration specification
// ---------------------------------------------------------------------------

/// Defines iteration over a body of steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopSpec {
    /// Fixed number of iterations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,

    /// Condition that ends the loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,

    /// Maximum iterations for conditional loops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i32>,

    /// Computed range ("1..10", "{start}..{count}").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,

    /// Variable name exposed to body steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var: Option<String>,

    /// Steps to repeat.
    pub body: Vec<Step>,
}

// ---------------------------------------------------------------------------
// OnCompleteSpec — runtime expansion
// ---------------------------------------------------------------------------

/// Defines actions triggered when a step completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnCompleteSpec {
    /// Path to the iterable collection in step output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub for_each: Option<String>,

    /// Formula to instantiate for each item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bond: Option<String>,

    /// Variable bindings for each iteration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vars: Option<std::collections::HashMap<String, String>>,

    /// Run all bonded molecules concurrently.
    #[serde(default)]
    pub parallel: bool,

    /// Run bonded molecules one at a time.
    #[serde(default)]
    pub sequential: bool,
}

// ---------------------------------------------------------------------------
// ComposeRules — formula composition and bonding
// ---------------------------------------------------------------------------

/// Defines how formulas can be bonded together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeRules {
    /// Named locations where other formulas can attach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bond_points: Option<Vec<BondPoint>>,

    /// Automatic attachments triggered by labels or conditions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<Vec<Hook>>,

    /// Apply expansion template to a single target step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expand: Option<Vec<ExpandRule>>,

    /// Apply expansion template to all steps matching a pattern.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<Vec<MapRule>>,

    /// Fork-join parallel execution patterns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<Vec<BranchRule>>,

    /// Conditional waits before steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Vec<GateRule>>,

    /// Aspect formula names to apply to this formula.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspects: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// BondPoint — named attachment site
// ---------------------------------------------------------------------------

/// A named attachment site for composition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondPoint {
    /// Unique identifier for this bond point.
    pub id: String,

    /// Description of what should be attached here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Step ID after which to attach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_step: Option<String>,

    /// Step ID before which to attach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_step: Option<String>,

    /// Make attached steps run in parallel with the anchor step.
    #[serde(default)]
    pub parallel: bool,
}

// ---------------------------------------------------------------------------
// Hook — automatic formula attachment
// ---------------------------------------------------------------------------

/// Defines automatic formula attachment based on conditions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    /// Trigger condition: "label:security", "type:bug", "priority:0-1".
    pub trigger: String,

    /// Formula to attach when triggered.
    pub attach: String,

    /// Bond point to attach at (default: end).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,

    /// Variable overrides for the attached formula.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vars: Option<std::collections::HashMap<String, String>>,
}

// ---------------------------------------------------------------------------
// ExpandRule — single target expansion
// ---------------------------------------------------------------------------

/// Applies an expansion template to a single target step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpandRule {
    /// Step ID to expand.
    pub target: String,

    /// Name of the expansion formula to apply.
    pub with: String,

    /// Variable overrides for the expansion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vars: Option<std::collections::HashMap<String, String>>,
}

// ---------------------------------------------------------------------------
// MapRule — pattern-match expansion
// ---------------------------------------------------------------------------

/// Applies an expansion template to all steps matching a pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapRule {
    /// Glob pattern matching step IDs to expand (e.g., "*.implement").
    pub select: String,

    /// Name of the expansion formula to apply.
    pub with: String,

    /// Variable overrides for the expansion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vars: Option<std::collections::HashMap<String, String>>,
}

// ---------------------------------------------------------------------------
// BranchRule — fork-join parallel execution
// ---------------------------------------------------------------------------

/// Defines parallel execution paths that rejoin (fork-join pattern).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchRule {
    /// Step ID that precedes the parallel paths.
    pub from: String,

    /// Step IDs that run in parallel.
    pub steps: Vec<String>,

    /// Step ID that follows all parallel paths.
    pub join: String,
}

// ---------------------------------------------------------------------------
// GateRule — condition-based wait
// ---------------------------------------------------------------------------

/// Defines a condition that must be satisfied before a step proceeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateRule {
    /// Step ID that the gate applies to.
    pub before: String,

    /// Expression to evaluate.
    pub condition: String,
}

// ---------------------------------------------------------------------------
// Pointcut — step matching for aspect formulas
// ---------------------------------------------------------------------------

/// Defines a target pattern for advice application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pointcut {
    /// Glob pattern to match step IDs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob: Option<String>,

    /// Match steps by type field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,

    /// Match steps having a specific label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

// ---------------------------------------------------------------------------
// AdviceRule — step transformation
// ---------------------------------------------------------------------------

/// Defines a step transformation rule (before/after/around).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdviceRule {
    /// Glob pattern matching step IDs to apply advice to.
    pub target: String,

    /// Insert a step before the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<AdviceStep>,

    /// Insert a step after the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<AdviceStep>,

    /// Wrap the target with before and after steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub around: Option<AroundAdvice>,
}

// ---------------------------------------------------------------------------
// AdviceStep — step to insert via advice
// ---------------------------------------------------------------------------

/// Defines a step to insert via advice operators.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdviceStep {
    /// Step identifier (supports {step.id} substitution).
    pub id: String,

    /// Step title (supports {step.id} substitution).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Step description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Issue type (task, bug, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,

    /// Additional context passed to the step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<std::collections::HashMap<String, String>>,

    /// Expected outputs from this step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<std::collections::HashMap<String, String>>,
}

// ---------------------------------------------------------------------------
// AroundAdvice — target wrapping
// ---------------------------------------------------------------------------

/// Wraps a target with before and after steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AroundAdvice {
    /// Steps to insert before the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Vec<AdviceStep>>,

    /// Steps to insert after the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Vec<AdviceStep>>,
}
