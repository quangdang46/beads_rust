//! Formula expansion engine — macro-style step transformation.
//!
//! Ported from Go beads `/internal/formula/expand.go`.
//!
//! Expansion operators replace target steps with template-expanded steps.
//! Unlike advice operators which insert steps around targets, expansion
//! operators completely replace the target with the expansion template.
//!
//! Two operators are supported:
//!   - expand: Apply template to a single target step
//!   - map: Apply template to all steps matching a pattern
//!
//! Templates use {target} and {target.description} placeholders that are
//! substituted with the target step's values during expansion.
//!
//! A maximum expansion depth (default 5) prevents runaway nested expansions.

use std::collections::{HashMap, HashSet};

use crate::formula::match_glob;
use crate::formula::types::{ComposeRules, Formula, FormulaType, Step};

/// Default maximum expansion depth. Prevents runaway nested expansions.
pub const DEFAULT_MAX_EXPANSION_DEPTH: usize = 5;

// ---------------------------------------------------------------------------
// ApplyExpansions
// ---------------------------------------------------------------------------

/// Apply all expand and map rules to a formula's steps.
///
/// Returns a new steps vector with expansions applied.
/// The original steps vector is not modified.
///
/// The parser is used to load referenced expansion formulas by name.
/// If the compose rules have no expand/map rules, steps are returned as-is.
pub fn apply_expansions(
    steps: &[Step],
    compose: Option<&ComposeRules>,
    formulas: &[Formula],
) -> Result<Vec<Step>, String> {
    let compose = match compose {
        Some(c) => c,
        None => return Ok(steps.to_vec()),
    };

    if compose.expand.as_ref().map_or(true, Vec::is_empty)
        && compose.r#map.as_ref().map_or(true, Vec::is_empty)
    {
        return Ok(steps.to_vec());
    }

    // Build a map of step ID -> step for quick lookup
    let mut step_map = build_step_map(steps);

    // Track which steps have been expanded (to avoid double expansion)
    let mut expanded: HashSet<String> = HashSet::new();

    // Apply expand rules first (specific targets)
    let mut result = steps.to_vec();
    if let Some(expand_rules) = &compose.expand {
        for rule in expand_rules {
            let target_step = match step_map.get(&rule.target) {
                Some(s) => s,
                None => return Err(format!("expand: target step {:?} not found", rule.target)),
            };

            if expanded.contains(&rule.target) {
                continue; // Already expanded
            }

            // Load the expansion formula
            let exp_formula = lookup_formula(&rule.with, formulas)?;

            if exp_formula.r#type != FormulaType::Expansion {
                return Err(format!(
                    "expand: {:?} is not an expansion formula (type={:?})",
                    rule.with, exp_formula.r#type
                ));
            }

            let template = exp_formula
                .template
                .as_ref()
                .ok_or_else(|| format!("expand: {:?} has no template steps", rule.with))?;

            if template.is_empty() {
                return Err(format!("expand: {:?} has no template steps", rule.with));
            }

            // Merge formula default vars with rule overrides
            let vars = merge_vars(&exp_formula, rule.vars.as_ref());

            // Expand the target step (start at depth 0)
            let expanded_steps = expand_step(target_step, template, 0, &vars)?;

            // Propagate target step's dependencies to root steps of the expansion
            let mut expanded_steps = expanded_steps;
            propagate_target_deps(target_step, &mut expanded_steps);

            // Replace the target step with expanded steps
            let target_id = rule.target.clone();
            result = replace_step(&result, &target_id, &expanded_steps);
            expanded.insert(target_id.clone());

            // Update dependencies: any step that depended on the target should now
            // depend on the last step of the expansion
            if let Some(last_step) = expanded_steps.last() {
                result = update_dependencies_for_expansion(&result, &target_id, &last_step.id);
            }

            // Rebuild stepMap from result so subsequent iterations see resolved deps
            step_map = build_step_map(&result);
        }
    }

    // Apply map rules (pattern matching)
    if let Some(map_rules) = &compose.r#map {
        for rule in map_rules {
            // Load the expansion formula
            let exp_formula = lookup_formula(&rule.with, formulas)?;

            if exp_formula.r#type != FormulaType::Expansion {
                return Err(format!(
                    "map: {:?} is not an expansion formula (type={:?})",
                    rule.with, exp_formula.r#type
                ));
            }

            let template = exp_formula
                .template
                .as_ref()
                .ok_or_else(|| format!("map: {:?} has no template steps", rule.with))?;

            if template.is_empty() {
                return Err(format!("map: {:?} has no template steps", rule.with));
            }

            // Merge formula default vars with rule overrides
            let vars = merge_vars(&exp_formula, rule.vars.as_ref());

            // Find all matching steps (including nested children)
            // Rebuild stepMap to capture any changes from previous expansions
            step_map = build_step_map(&result);
            let mut to_expand: Vec<(String, Step)> = Vec::new();
            for (id, step) in &step_map {
                if matches_glob(&rule.select, id) && !expanded.contains(id) {
                    to_expand.push((id.clone(), step.clone()));
                }
            }

            // Expand each matching step
            for (target_id, target_step) in &to_expand {
                let mut expanded_steps = expand_step(target_step, template, 0, &vars)?;

                // Propagate target step's dependencies to root steps of the expansion
                propagate_target_deps(target_step, &mut expanded_steps);

                result = replace_step(&result, target_id, &expanded_steps);
                expanded.insert(target_id.clone());

                // Update dependencies: any step that depended on the target should now
                // depend on the last step of the expansion
                if let Some(last_step) = expanded_steps.last() {
                    result = update_dependencies_for_expansion(&result, target_id, &last_step.id);
                }

                // Rebuild stepMap from result so subsequent iterations see resolved deps
                step_map = build_step_map(&result);
            }
        }
    }

    // Validate no duplicate step IDs after expansion
    let dups = find_duplicate_step_ids(&result);
    if !dups.is_empty() {
        return Err(format!("duplicate step IDs after expansion: {:?}", dups));
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Helper: lookup a formula by name from a formula list
// ---------------------------------------------------------------------------

fn lookup_formula<'a>(name: &str, formulas: &'a [Formula]) -> Result<&'a Formula, String> {
    formulas
        .iter()
        .find(|f| f.formula == name)
        .ok_or_else(|| format!("expansion formula {:?} not found", name))
}

// ---------------------------------------------------------------------------
// expandStep
// ---------------------------------------------------------------------------

/// Expand a target step using the given template.
/// Returns the expanded steps with placeholders substituted.
/// The depth parameter tracks recursion depth for children; if it exceeds
/// DEFAULT_MAX_EXPANSION_DEPTH, an error is returned.
fn expand_step(
    target: &Step,
    template: &[Step],
    depth: usize,
    vars: &HashMap<String, String>,
) -> Result<Vec<Step>, String> {
    if depth > DEFAULT_MAX_EXPANSION_DEPTH {
        return Err(format!(
            "expansion depth limit exceeded: max {} levels (currently at {}) - step {:?}",
            DEFAULT_MAX_EXPANSION_DEPTH, depth, target.id
        ));
    }

    let mut result: Vec<Step> = Vec::with_capacity(template.len());

    for tmpl in template {
        let mut expanded = Step {
            id: substitute_vars(&substitute_target_placeholders(&tmpl.id, target), vars),
            title: substitute_opt(&tmpl.title, target, vars),
            description: substitute_opt(&tmpl.description, target, vars),
            notes: tmpl.notes.clone(),
            r#type: tmpl.r#type.clone(),
            priority: tmpl.priority,
            assignee: tmpl.assignee.clone(),
            labels: substitute_labels(&tmpl.labels, target, vars),
            depends_on: substitute_deps(&tmpl.depends_on, target, vars),
            needs: substitute_deps(&tmpl.needs, target, vars),
            condition: tmpl.condition.clone(),
            ..Default::default()
        };

        // Handle children recursively with depth tracking
        if let Some(children) = &tmpl.children {
            if !children.is_empty() {
                let children_expanded = expand_step(target, children, depth + 1, vars)?;
                expanded.children = Some(children_expanded);
            }
        }

        result.push(expanded);
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Placeholder substitution
// ---------------------------------------------------------------------------

/// Replace {target} and {target.*} placeholders with target step values.
fn substitute_target_placeholders(s: &str, target: &Step) -> String {
    if s.is_empty() {
        return s.to_string();
    }

    let mut result = s.to_string();

    // Replace {target} with target step ID
    result = result.replace("{target}", &target.id);

    // Replace {target.id} with target step ID
    result = result.replace("{target.id}", &target.id);

    // Replace {target.title} with target step title
    if let Some(ref title) = target.title {
        result = result.replace("{target.title}", title);
    }

    // Replace {target.description} with target step description
    if let Some(ref desc) = target.description {
        result = result.replace("{target.description}", desc);
    }

    result
}

/// Substitute {{variable}} placeholders in a string with actual values.
fn substitute_vars(s: &str, vars: &HashMap<String, String>) -> String {
    let mut result = s.to_string();
    for (key, value) in vars {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, value);
    }
    result
}

/// Apply both target placeholders and variable substitution to an Option<String>.
fn substitute_opt(
    s: &Option<String>,
    target: &Step,
    vars: &HashMap<String, String>,
) -> Option<String> {
    s.as_ref().map(|val| {
        let val = substitute_target_placeholders(val, target);
        substitute_vars(&val, vars)
    })
}

/// Apply placeholders to all labels.
fn substitute_labels(
    labels: &[String],
    target: &Step,
    vars: &HashMap<String, String>,
) -> Vec<String> {
    labels
        .iter()
        .map(|l| {
            let l = substitute_target_placeholders(l, target);
            substitute_vars(&l, vars)
        })
        .collect()
}

/// Apply placeholders to all dependency references.
fn substitute_deps(deps: &[String], target: &Step, vars: &HashMap<String, String>) -> Vec<String> {
    deps.iter()
        .map(|d| {
            let d = substitute_target_placeholders(d, target);
            substitute_vars(&d, vars)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// mergeVars: merge formula defaults with rule overrides
// ---------------------------------------------------------------------------

/// Merge formula default vars with rule overrides.
/// Override values take precedence over defaults.
fn merge_vars(
    formula: &Formula,
    overrides: Option<&HashMap<String, String>>,
) -> HashMap<String, String> {
    let mut result = HashMap::new();

    // Start with formula defaults
    if let Some(vars) = &formula.vars {
        for v in vars {
            if let Some(ref default) = v.default {
                result.insert(v.name.clone(), default.clone());
            }
        }
    }

    // Apply overrides (these win)
    if let Some(overrides) = overrides {
        for (name, value) in overrides {
            result.insert(name.clone(), value.clone());
        }
    }

    result
}

// ---------------------------------------------------------------------------
// buildStepMap
// ---------------------------------------------------------------------------

/// Create a map of step ID to step (recursive).
fn build_step_map(steps: &[Step]) -> HashMap<String, Step> {
    let mut result = HashMap::new();
    for step in steps {
        result.insert(step.id.clone(), step.clone());
        // Add children recursively
        if let Some(children) = &step.children {
            for (id, child) in build_step_map(children) {
                result.insert(id, child);
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// replaceStep
// ---------------------------------------------------------------------------

/// Replace a step with the given ID with a slice of new steps.
/// Searches recursively through children to find and replace the target.
fn replace_step(steps: &[Step], target_id: &str, replacement: &[Step]) -> Vec<Step> {
    let mut result = Vec::with_capacity(steps.len() + replacement.len().saturating_sub(1));

    for step in steps {
        if step.id == target_id {
            // Replace with expanded steps
            result.extend_from_slice(replacement);
        } else {
            // Keep the step, but check children
            if let Some(children) = &step.children {
                let mut cloned = step.clone();
                cloned.children = Some(replace_step(children, target_id, replacement));
                result.push(cloned);
            } else {
                result.push(step.clone());
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// updateDependenciesForExpansion
// ---------------------------------------------------------------------------

/// Update dependency references after expansion.
/// When step X is expanded into X.draft, X.refine-1, etc., any step that
/// depended on X should now depend on the last step in the expansion.
fn update_dependencies_for_expansion(
    steps: &[Step],
    expanded_id: &str,
    last_expanded_step_id: &str,
) -> Vec<Step> {
    let mut result: Vec<Step> = Vec::with_capacity(steps.len());

    for step in steps {
        let mut cloned = step.clone();

        // Update DependsOn references
        for dep in &mut cloned.depends_on {
            if dep == expanded_id {
                *dep = last_expanded_step_id.to_string();
            }
        }

        // Update Needs references
        for need in &mut cloned.needs {
            if need == expanded_id {
                *need = last_expanded_step_id.to_string();
            }
        }

        // Handle children recursively
        if let Some(children) = &step.children {
            cloned.children = Some(update_dependencies_for_expansion(
                children,
                expanded_id,
                last_expanded_step_id,
            ));
        }

        result.push(cloned);
    }

    result
}

// ---------------------------------------------------------------------------
// propagateTargetDeps
// ---------------------------------------------------------------------------

/// Copy the target step's Needs and DependsOn to the root steps of an expansion.
///
/// Root steps are those whose existing dependencies only reference other steps
/// within the expansion (i.e., they have no external deps from the template).
/// This preserves cross-expansion dependency chains that would otherwise be
/// lost when the target step is replaced.
fn propagate_target_deps(target: &Step, expanded_steps: &mut [Step]) {
    if target.needs.is_empty() && target.depends_on.is_empty() {
        return;
    }

    let expanded_ids: HashSet<String> = expanded_steps.iter().map(|s| s.id.clone()).collect();

    for s in expanded_steps.iter_mut() {
        let is_root = !s.needs.iter().any(|n| expanded_ids.contains(n))
            && !s.depends_on.iter().any(|d| expanded_ids.contains(d));

        if is_root {
            // Prepend target's deps (new vec to avoid aliasing)
            if !target.needs.is_empty() {
                let mut new_needs = target.needs.clone();
                new_needs.extend(s.needs.clone());
                s.needs = new_needs;
            }
            if !target.depends_on.is_empty() {
                let mut new_deps = target.depends_on.clone();
                new_deps.extend(s.depends_on.clone());
                s.depends_on = new_deps;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Duplicate detection
// ---------------------------------------------------------------------------

/// Return any duplicate step IDs found in the steps slice.
fn find_duplicate_step_ids(steps: &[Step]) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    count_step_ids(steps, &mut seen);

    let mut dups: Vec<String> = seen
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(id, _)| id)
        .collect();
    dups.sort();
    dups
}

/// Count occurrences of each step ID recursively.
fn count_step_ids(steps: &[Step], counts: &mut HashMap<String, usize>) {
    for step in steps {
        *counts.entry(step.id.clone()).or_insert(0) += 1;
        if let Some(children) = &step.children {
            count_step_ids(children, counts);
        }
    }
}

// ---------------------------------------------------------------------------
// matchGlob (delegates to the shared module-level function)
// ---------------------------------------------------------------------------

fn matches_glob(pattern: &str, step_id: &str) -> bool {
    match_glob(pattern, step_id)
}

// ---------------------------------------------------------------------------
// MaterializeExpansion
// ---------------------------------------------------------------------------

/// Convert a standalone expansion formula into cookable form by expanding
/// its Template into Steps. A synthetic target step is created using targetID
/// as the step ID and the formula's own name/description for {target.title}
/// and {target.description} placeholders.
///
/// No-op if the formula is not an expansion type, has no Template, or already
/// has Steps.
pub fn materialize_expansion(
    formula: &mut Formula,
    target_id: &str,
    vars: &HashMap<String, String>,
) -> Result<(), String> {
    if formula.r#type != FormulaType::Expansion
        || formula.steps.as_ref().is_some_and(|s| !s.is_empty())
    {
        return Ok(());
    }

    let template = match &formula.template {
        Some(t) if !t.is_empty() => t.clone(),
        _ => return Ok(()),
    };

    let target = Step {
        id: target_id.to_string(),
        title: Some(formula.formula.clone()),
        description: formula.description.clone(),
        ..Default::default()
    };

    let expanded_steps = expand_step(&target, &template, 0, vars)?;

    formula.steps = Some(expanded_steps);
    Ok(())
}

// ---------------------------------------------------------------------------
// ApplyInlineExpansions
// ---------------------------------------------------------------------------

/// Apply Step.Expand fields to inline expansions.
/// Steps with the Expand field set are replaced by the referenced expansion template.
/// The step's ExpandVars are passed as variable overrides to the expansion.
///
/// This differs from compose.Expand in that the expansion is declared inline on the
/// step itself rather than in a central compose section.
///
/// Returns a new steps vector with inline expansions applied.
pub fn apply_inline_expansions(steps: &[Step], formulas: &[Formula]) -> Result<Vec<Step>, String> {
    apply_inline_expansions_recursive(steps, formulas, 0)
}

/// Handle inline expansions for a slice of steps recursively.
fn apply_inline_expansions_recursive(
    steps: &[Step],
    formulas: &[Formula],
    depth: usize,
) -> Result<Vec<Step>, String> {
    if depth > DEFAULT_MAX_EXPANSION_DEPTH {
        return Err(format!(
            "inline expansion depth limit exceeded: max {} levels",
            DEFAULT_MAX_EXPANSION_DEPTH
        ));
    }

    let mut result: Vec<Step> = Vec::with_capacity(steps.len());

    for step in steps {
        // Check if this step has an inline expansion
        if let Some(ref expand_name) = step.expand {
            // Load the expansion formula
            let exp_formula = lookup_formula(expand_name, formulas)?;

            if exp_formula.r#type != FormulaType::Expansion {
                return Err(format!(
                    "inline expand on step {:?}: {:?} is not an expansion formula (type={:?})",
                    step.id, expand_name, exp_formula.r#type
                ));
            }

            let template = exp_formula.template.as_ref().ok_or_else(|| {
                format!(
                    "inline expand on step {:?}: {:?} has no template steps",
                    step.id, expand_name
                )
            })?;

            if template.is_empty() {
                return Err(format!(
                    "inline expand on step {:?}: {:?} has no template steps",
                    step.id, expand_name
                ));
            }

            // Merge formula default vars with step's ExpandVars overrides
            let vars = merge_vars(exp_formula, step.expand_vars.as_ref());

            // Expand the step using the template
            let mut expanded_steps = expand_step(step, template, 0, &vars)?;

            // Propagate the original step's dependencies to root steps of the expansion
            propagate_target_deps(step, &mut expanded_steps);

            // Recursively process expanded steps for nested inline expansions
            let processed =
                apply_inline_expansions_recursive(&expanded_steps, formulas, depth + 1)?;

            result.extend(processed);
        } else {
            // No inline expansion - keep the step, but process children recursively
            let mut cloned = step.clone();

            if let Some(children) = &step.children {
                let processed_children =
                    apply_inline_expansions_recursive(children, formulas, depth)?;
                cloned.children = Some(processed_children);
            }

            result.push(cloned);
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formula::types::*;

    fn make_target() -> Step {
        Step {
            id: "implement".to_string(),
            title: Some("Implement the feature".to_string()),
            description: Some("Write the code for the feature".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_substitute_target_placeholders_basic() {
        let target = make_target();
        let result = substitute_target_placeholders("{target}.draft", &target);
        assert_eq!(result, "implement.draft");
    }

    #[test]
    fn test_substitute_target_placeholders_id() {
        let target = make_target();
        let result = substitute_target_placeholders("{target.id}.refine", &target);
        assert_eq!(result, "implement.refine");
    }

    #[test]
    fn test_substitute_target_placeholders_title() {
        let target = make_target();
        let result = substitute_target_placeholders("Working on: {target.title}", &target);
        assert_eq!(result, "Working on: Implement the feature");
    }

    #[test]
    fn test_substitute_target_placeholders_description() {
        let target = make_target();
        let result = substitute_target_placeholders("Task: {target.description}", &target);
        assert_eq!(result, "Task: Write the code for the feature");
    }

    #[test]
    fn test_substitute_target_placeholders_multiple() {
        let target = make_target();
        let result = substitute_target_placeholders("{target}: {target.description}", &target);
        assert_eq!(result, "implement: Write the code for the feature");
    }

    #[test]
    fn test_substitute_target_placeholders_no_placeholders() {
        let target = make_target();
        let result = substitute_target_placeholders("plain text", &target);
        assert_eq!(result, "plain text");
    }

    #[test]
    fn test_substitute_target_placeholders_empty() {
        let target = make_target();
        let result = substitute_target_placeholders("", &target);
        assert_eq!(result, "");
    }

    #[test]
    fn test_expand_step_basic() {
        let target = make_target();
        let template = vec![
            Step {
                id: "{target}.draft".to_string(),
                title: Some("Draft: {target.title}".to_string()),
                description: Some("Initial attempt at: {target.description}".to_string()),
                ..Default::default()
            },
            Step {
                id: "{target}.refine".to_string(),
                title: Some("Refine: {target.title}".to_string()),
                needs: vec!["{target}.draft".to_string()],
                ..Default::default()
            },
        ];

        let vars = HashMap::new();
        let result = expand_step(&target, &template, 0, &vars).unwrap();

        assert_eq!(result.len(), 2);

        // Check first step
        assert_eq!(result[0].id, "implement.draft");
        assert_eq!(
            result[0].title.as_deref(),
            Some("Draft: Implement the feature")
        );
        assert_eq!(
            result[0].description.as_deref(),
            Some("Initial attempt at: Write the code for the feature")
        );

        // Check second step
        assert_eq!(result[1].id, "implement.refine");
        assert_eq!(result[1].needs, vec!["implement.draft"]);
    }

    #[test]
    fn test_expand_step_depth_limit() {
        let target = Step {
            id: "root".to_string(),
            title: Some("Root step".to_string()),
            ..Default::default()
        };

        // Create a deeply nested template (depth 6 levels)
        let mut deepest = Step {
            id: "level-6".to_string(),
            title: Some("Level 6".to_string()),
            ..Default::default()
        };
        for i in (0..6).rev() {
            deepest = Step {
                id: format!("level-{}", i),
                title: Some(format!("Level {}", i)),
                children: Some(vec![deepest]),
                ..Default::default()
            };
        }

        let template = vec![deepest];
        let vars = HashMap::new();

        // With depth 0 start, going to level 6 means 7 levels total (0-6)
        // DefaultMaxExpansionDepth is 5, so this should fail
        let err = expand_step(&target, &template, 0, &vars).unwrap_err();
        assert!(
            err.contains("expansion depth limit exceeded"),
            "Expected depth limit error, got: {}",
            err
        );
    }

    #[test]
    fn test_expand_step_within_depth_limit() {
        let target = Step {
            id: "root".to_string(),
            title: Some("Root step".to_string()),
            ..Default::default()
        };

        // Build a 5-level deep template (levels 0-4, which is exactly at the limit)
        let mut shallow = Step {
            id: "level-4".to_string(),
            title: Some("Level 4".to_string()),
            ..Default::default()
        };
        for i in (0..4).rev() {
            shallow = Step {
                id: format!("level-{}", i),
                title: Some(format!("Level {}", i)),
                children: Some(vec![shallow]),
                ..Default::default()
            };
        }

        let template = vec![shallow];
        let vars = HashMap::new();
        let result = expand_step(&target, &template, 0, &vars).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_replace_step_simple() {
        let steps = vec![
            Step {
                id: "design".to_string(),
                title: Some("Design".to_string()),
                ..Default::default()
            },
            Step {
                id: "implement".to_string(),
                title: Some("Implement".to_string()),
                ..Default::default()
            },
            Step {
                id: "test".to_string(),
                title: Some("Test".to_string()),
                ..Default::default()
            },
        ];

        let replacement = vec![
            Step {
                id: "implement.draft".to_string(),
                title: Some("Draft".to_string()),
                ..Default::default()
            },
            Step {
                id: "implement.refine".to_string(),
                title: Some("Refine".to_string()),
                ..Default::default()
            },
        ];

        let result = replace_step(&steps, "implement", &replacement);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].id, "design");
        assert_eq!(result[1].id, "implement.draft");
        assert_eq!(result[2].id, "implement.refine");
        assert_eq!(result[3].id, "test");
    }

    #[test]
    fn test_build_step_map() {
        let steps = vec![
            Step {
                id: "parent".to_string(),
                title: Some("Parent".to_string()),
                children: Some(vec![
                    Step {
                        id: "child1".to_string(),
                        title: Some("Child 1".to_string()),
                        ..Default::default()
                    },
                    Step {
                        id: "child2".to_string(),
                        title: Some("Child 2".to_string()),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            },
            Step {
                id: "sibling".to_string(),
                title: Some("Sibling".to_string()),
                ..Default::default()
            },
        ];

        let map = build_step_map(&steps);
        assert_eq!(map.len(), 4);
        assert!(map.contains_key("parent"));
        assert!(map.contains_key("child1"));
        assert!(map.contains_key("child2"));
        assert!(map.contains_key("sibling"));
    }

    #[test]
    fn test_propagate_target_deps() {
        let target = Step {
            id: "implement".to_string(),
            title: Some("Implement".to_string()),
            needs: vec!["design".to_string()],
            ..Default::default()
        };

        let mut expanded = vec![
            Step {
                id: "implement.draft".to_string(),
                title: Some("Draft".to_string()),
                ..Default::default()
            },
            Step {
                id: "implement.refine".to_string(),
                title: Some("Refine".to_string()),
                needs: vec!["implement.draft".to_string()],
                ..Default::default()
            },
        ];

        propagate_target_deps(&target, &mut expanded);

        // implement.draft is root (only has deps to other expanded steps? no, it has none initially)
        // So it should get target's needs
        assert!(
            expanded[0].needs.contains(&"design".to_string()),
            "root step should get target's needs, got: {:?}",
            expanded[0].needs
        );

        // implement.refine has needs=["implement.draft"] which is an internal dep,
        // so it should NOT get target's needs
        assert_eq!(
            expanded[1].needs,
            vec!["implement.draft"],
            "non-root step should not get target's needs"
        );
    }

    #[test]
    fn test_update_dependencies_for_expansion() {
        let steps = vec![
            Step {
                id: "design".to_string(),
                title: Some("Design".to_string()),
                ..Default::default()
            },
            Step {
                id: "test".to_string(),
                title: Some("Test".to_string()),
                needs: vec!["implement".to_string()],
                ..Default::default()
            },
        ];

        let result = update_dependencies_for_expansion(&steps, "implement", "implement.refine");
        assert_eq!(result[1].needs, vec!["implement.refine"]);
    }

    #[test]
    fn test_merge_vars() {
        let formula = Formula {
            formula: "exp-test".to_string(),
            r#type: FormulaType::Expansion,
            vars: Some(vec![
                VarDef {
                    name: "component".to_string(),
                    default: Some("api".to_string()),
                    ..Default::default()
                },
                VarDef {
                    name: "count".to_string(),
                    default: Some("3".to_string()),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };

        let mut overrides = HashMap::new();
        overrides.insert("component".to_string(), "auth".to_string());

        let result = merge_vars(&formula, Some(&overrides));
        assert_eq!(result.get("component").map(String::as_str), Some("auth"));
        assert_eq!(result.get("count").map(String::as_str), Some("3"));
    }

    #[test]
    fn test_find_duplicate_step_ids() {
        let steps = vec![
            Step {
                id: "a".to_string(),
                title: Some("A".to_string()),
                ..Default::default()
            },
            Step {
                id: "b".to_string(),
                title: Some("B".to_string()),
                ..Default::default()
            },
            Step {
                id: "a".to_string(),
                title: Some("A again".to_string()),
                ..Default::default()
            },
        ];

        let dups = find_duplicate_step_ids(&steps);
        assert_eq!(dups, vec!["a"]);
    }

    #[test]
    fn test_apply_expansions_no_compose() {
        let steps = vec![Step {
            id: "step1".to_string(),
            title: Some("Step 1".to_string()),
            ..Default::default()
        }];

        let result = apply_expansions(&steps, None, &[]).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_apply_expansions_no_rules() {
        let steps = vec![Step {
            id: "step1".to_string(),
            title: Some("Step 1".to_string()),
            ..Default::default()
        }];

        let compose = ComposeRules::default();
        let result = apply_expansions(&steps, Some(&compose), &[]).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_matches_glob_exact() {
        assert!(matches_glob("design", "design"));
        assert!(!matches_glob("design", "implement"));
    }

    #[test]
    fn test_matches_glob_suffix() {
        assert!(matches_glob("*.implement", "shiny.implement"));
        assert!(matches_glob("*.implement", "core.implement"));
        assert!(!matches_glob("*.implement", "implement"));
    }

    #[test]
    fn test_matches_glob_prefix() {
        assert!(matches_glob("shiny.*", "shiny.design"));
        assert!(!matches_glob("shiny.*", "core.design"));
    }

    #[test]
    fn test_matches_glob_wildcard() {
        assert!(matches_glob("*", "anything"));
    }
}
