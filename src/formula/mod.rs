//! Formula Language — workflow-as-code engine.
//!
//! This module ports the Go beads formula package, providing:
//! - Core types for `.formula.json` / `.formula.toml` files
//! - Parser with caching, extends resolution, and cycle detection
//! - Validation (duplicate IDs, dependency references, variable consistency)
//! - Variable substitution for step expansion
//! - CLI integration via `br formula apply`
//!
//! Formula types: Workflow (standard steps), Expansion (macro template),
//! Aspect (cross-cutting), Convoy (multi-agent).
//!
//! See `/tmp/beads-go/internal/formula/` for the Go reference implementation.

pub mod controlflow;
pub mod expand;
pub mod parser;
pub mod types;

/// Re-export key types at module level.
pub use parser::Parser;
pub use types::{
    AdviceRule, AdviceStep, AroundAdvice, BondPoint, BranchRule, ComposeRules, ExpandRule, Formula,
    FormulaType, Gate, GateRule, Hook, LoopSpec, MapRule, OnCompleteSpec, Pointcut, Step, VarDef,
};

/// Re-export key expansion and control flow functions.
pub use controlflow::{apply_branches, apply_control_flow, apply_gates, apply_loops};
pub use expand::{
    DEFAULT_MAX_EXPANSION_DEPTH, apply_expansions, apply_inline_expansions, materialize_expansion,
};

/// Simple glob matching for step IDs, usable from both expand.rs and externally.
/// Supports:
///   - "exact" - exact match
///   - "*.suffix" - ends with .suffix
///   - "prefix.*" - starts with prefix.
///   - "*" - matches everything
///   - "prefix.*.suffix" - starts with prefix. and ends with .suffix
pub fn match_glob(pattern: &str, step_id: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    // *.suffix pattern (e.g., "*.implement")
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return step_id.ends_with(suffix);
    }

    // prefix.* pattern (e.g., "shiny.*")
    if let Some(prefix) = pattern.strip_suffix(".*") {
        if prefix.is_empty() {
            return true; // ".*" matches everything
        }
        // Check for prefix.*.suffix pattern
        if let Some(dot_star_pos) = prefix.rfind(".*") {
            let inner_prefix = &prefix[..dot_star_pos];
            let suffix = &prefix[dot_star_pos + 2..];
            return step_id.starts_with(inner_prefix)
                && step_id[inner_prefix.len()..].ends_with(suffix);
        }
        return step_id.starts_with(prefix) && step_id[prefix.len()..].starts_with('.');
    }

    // For simple patterns without wildcards, do exact match
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern == step_id;
    }

    // Simple wildcard matching
    simple_wildcard_match(pattern, step_id)
}

/// Simple wildcard matching (? for single char, * for any sequence).
fn simple_wildcard_match(pattern: &str, text: &str) -> bool {
    let p_chars: Vec<char> = pattern.chars().collect();
    let t_chars: Vec<char> = text.chars().collect();
    let (pl, tl) = (p_chars.len(), t_chars.len());
    let mut pi = 0usize;
    let mut ti = 0usize;
    let mut star_pi = None;
    let mut star_ti = 0usize;

    while ti < tl {
        if pi < pl && (p_chars[pi] == '?' || p_chars[pi] == t_chars[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pl && p_chars[pi] == '*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < pl && p_chars[pi] == '*' {
        pi += 1;
    }

    pi == pl
}

#[cfg(test)]
mod tests;
