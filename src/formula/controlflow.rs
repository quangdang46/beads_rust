//! Formula control flow — loops, branches, and gates.
//!
//! Ported from Go beads `/internal/formula/controlflow.go`.
//!
//! Control flow operators enable:
//!   - loop: Repeat a body of steps (fixed count or conditional)
//!   - branch: Fork-join parallel execution patterns
//!   - gate: Conditional waits before steps proceed
//!
//! These operators are applied during formula cooking to transform
//! the step graph before creating the proto bead.

use std::collections::HashMap;

use crate::formula::types::{BranchRule, ComposeRules, LoopSpec, Step};

// ---------------------------------------------------------------------------
// ApplyLoops
// ---------------------------------------------------------------------------

/// Expand loop bodies in a formula's steps.
/// Fixed-count loops expand the body N times with indexed step IDs.
/// Conditional loops expand once and add runtime loop metadata.
/// Returns a new steps vector with loops expanded.
pub fn apply_loops(steps: &[Step]) -> Result<Vec<Step>, String> {
    let mut result: Vec<Step> = Vec::with_capacity(steps.len());

    for step in steps {
        if step.r#loop.is_none() {
            // No loop - recursively process children
            let mut cloned = step.clone();
            if let Some(children) = &step.children {
                let children = apply_loops(children)?;
                cloned.children = Some(children);
            }
            result.push(cloned);
            continue;
        }

        let loop_spec = step.r#loop.as_ref().unwrap();

        // Validate loop spec
        validate_loop_spec(loop_spec, &step.id)?;

        // Expand the loop
        let expanded = expand_loop(step, loop_spec)?;
        result.extend(expanded);
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// validateLoopSpec
// ---------------------------------------------------------------------------

/// Check that a loop spec is valid.
fn validate_loop_spec(loop_spec: &LoopSpec, step_id: &str) -> Result<(), String> {
    if loop_spec.body.is_empty() {
        return Err(format!("loop {:?}: body is required", step_id));
    }

    // Count the number of loop types specified
    let mut loop_types = 0u8;
    if loop_spec.count.is_some() {
        loop_types += 1;
    }
    if loop_spec.until.is_some() {
        loop_types += 1;
    }
    if loop_spec.range.is_some() {
        loop_types += 1;
    }

    if loop_types == 0 {
        return Err(format!(
            "loop {:?}: one of count, until, or range is required",
            step_id
        ));
    }
    if loop_types > 1 {
        return Err(format!(
            "loop {:?}: only one of count, until, or range can be specified",
            step_id
        ));
    }

    // For until loops, max is required
    if loop_spec.until.is_some() && loop_spec.max.is_none() {
        return Err(format!(
            "loop {:?}: max is required when until is set",
            step_id
        ));
    }

    if let Some(count) = loop_spec.count {
        if count < 0 {
            return Err(format!("loop {:?}: count must be positive", step_id));
        }
    }

    if let Some(max) = loop_spec.max {
        if max < 0 {
            return Err(format!("loop {:?}: max must be positive", step_id));
        }
    }

    // Validate range syntax
    if let Some(ref range) = loop_spec.range {
        if !range.contains("..") {
            return Err(format!(
                "loop {:?}: invalid range {:?}: expected start..end format",
                step_id, range
            ));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// expandLoop
// ---------------------------------------------------------------------------

/// Expand a loop step into its constituent steps.
fn expand_loop(step: &Step, loop_spec: &LoopSpec) -> Result<Vec<Step>, String> {
    expand_loop_with_vars(step, loop_spec, &HashMap::new())
}

/// Expand a loop step using the given variable context.
fn expand_loop_with_vars(
    step: &Step,
    loop_spec: &LoopSpec,
    vars: &HashMap<String, String>,
) -> Result<Vec<Step>, String> {
    match loop_spec.count {
        Some(count) if count > 0 => {
            // Fixed-count loop: expand body N times
            expand_fixed_count_loop(step, loop_spec, count, vars)
        }
        _ => {
            if let Some(ref range) = loop_spec.range {
                // Range loop: expand body for each value in the computed range
                expand_range_loop(step, loop_spec, range, vars)
            } else if loop_spec.until.is_some() {
                // Conditional loop: expand once with loop metadata
                expand_conditional_loop(step, loop_spec, vars)
            } else {
                // Shouldn't happen - validation catches this
                Err(format!("loop {:?}: no iteration method specified", step.id))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Fixed-count loop expansion
// ---------------------------------------------------------------------------

/// Expand a fixed-count loop. The body is repeated `count` times.
fn expand_fixed_count_loop(
    step: &Step,
    loop_spec: &LoopSpec,
    count: i32,
    _vars: &HashMap<String, String>,
) -> Result<Vec<Step>, String> {
    let mut result: Vec<Step> = Vec::new();

    for iter in 1..=count {
        let iter_steps = expand_loop_iteration(step, loop_spec, iter, &HashMap::new())?;
        result.extend(iter_steps);
    }

    // Recursively expand any nested loops FIRST
    result = apply_loops(&result)?;

    // THEN chain iterations on the expanded result
    if count > 1 {
        result = chain_expanded_iterations(&result, &step.id, count as usize);
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Range loop expansion
// ---------------------------------------------------------------------------

/// Expand a range-based loop.
fn expand_range_loop(
    step: &Step,
    loop_spec: &LoopSpec,
    range_expr: &str,
    _vars: &HashMap<String, String>,
) -> Result<Vec<Step>, String> {
    // Parse the range expression: "start..end"
    let (start_str, end_str) = range_expr
        .split_once("..")
        .ok_or_else(|| format!("loop {:?}: invalid range {:?}", step.id, range_expr))?;

    let start: i32 = start_str
        .trim()
        .parse()
        .map_err(|_| format!("loop {:?}: invalid range start {:?}", step.id, start_str))?;
    let end: i32 = end_str
        .trim()
        .parse()
        .map_err(|_| format!("loop {:?}: invalid range end {:?}", step.id, end_str))?;

    if end < start {
        return Err(format!(
            "loop {:?}: range end ({}) is less than start ({})",
            step.id, end, start
        ));
    }

    let mut result: Vec<Step> = Vec::new();
    let count = end - start + 1;
    let mut iter_num = 0i32;

    for val in start..=end {
        iter_num += 1;
        // Build iteration vars: include the loop variable if specified
        let mut iter_vars = HashMap::new();
        if let Some(ref var_name) = loop_spec.var {
            iter_vars.insert(var_name.clone(), val.to_string());
        }
        let iter_steps = expand_loop_iteration(step, loop_spec, iter_num, &iter_vars)?;
        result.extend(iter_steps);
    }

    // Recursively expand any nested loops FIRST
    result = apply_loops(&result)?;

    // THEN chain iterations
    if count > 1 {
        result = chain_expanded_iterations(&result, &step.id, count as usize);
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Conditional loop expansion
// ---------------------------------------------------------------------------

/// Expand a conditional loop. Expands body once with loop metadata for runtime.
fn expand_conditional_loop(
    step: &Step,
    loop_spec: &LoopSpec,
    _vars: &HashMap<String, String>,
) -> Result<Vec<Step>, String> {
    let until = loop_spec.until.as_deref().unwrap_or_default();
    let max = loop_spec.max.unwrap_or(1);

    let mut iter_steps = expand_loop_iteration(step, loop_spec, 1, &HashMap::new())?;

    // Add loop metadata to first step for runtime evaluation
    if let Some(first_step) = iter_steps.first_mut() {
        let loop_label = format!(
            "loop:{{\"until\":\"{}\",\"max\":{}}}",
            until.replace('"', "\\\""),
            max
        );
        first_step.labels.push(loop_label);
    }

    // Recursively expand any nested loops
    let result = apply_loops(&iter_steps)?;

    Ok(result)
}

// ---------------------------------------------------------------------------
// expandLoopIteration
// ---------------------------------------------------------------------------

/// Expand a single iteration of a loop.
/// The iteration index is used to generate unique step IDs.
fn expand_loop_iteration(
    step: &Step,
    loop_spec: &LoopSpec,
    iteration: i32,
    iter_vars: &HashMap<String, String>,
) -> Result<Vec<Step>, String> {
    let result_size = loop_spec.body.len();
    let mut result: Vec<Step> = Vec::with_capacity(result_size);

    // Build set of step IDs within the loop body (for dependency rewriting)
    let body_step_ids = collect_body_step_ids(&loop_spec.body);

    for body_step in &loop_spec.body {
        // Create unique ID for this iteration
        let iter_id = format!("{}.iter{}.{}", step.id, iteration, body_step.id);

        // Substitute loop variables in title and description
        let mut cloned = Step {
            id: iter_id,
            title: body_step.title.clone(),
            description: body_step.description.clone(),
            r#type: body_step.r#type.clone(),
            priority: body_step.priority,
            assignee: body_step.assignee.clone(),
            expand: body_step.expand.clone(),
            gate: body_step.gate.clone(),
            r#loop: clone_loop_spec(body_step.r#loop.as_ref()),
            on_complete: body_step.on_complete.clone(),
            condition: body_step.condition.clone(),
            source_formula: body_step.source_formula.clone(),
            ..Default::default()
        };

        // Substitute loop variables
        if let Some(ref title) = cloned.title {
            cloned.title = Some(substitute_loop_vars(
                title,
                &body_step.id,
                iteration,
                iter_vars,
            ));
        } else {
            cloned.title = body_step.title.clone();
        }
        if let Some(ref desc) = cloned.description {
            cloned.description = Some(substitute_loop_vars(
                desc,
                &body_step.id,
                iteration,
                iter_vars,
            ));
        }

        // Set source location with iteration info
        if let Some(ref loc) = body_step.source_location {
            cloned.source_location = Some(format!("{}.iter{}", loc, iteration));
        }

        // Clone ExpandVars if present, adding loop vars
        if body_step
            .expand_vars
            .as_ref()
            .map_or(false, |ev| !ev.is_empty())
            || !iter_vars.is_empty()
        {
            let mut ev = body_step.expand_vars.clone().unwrap_or_default();
            for (k, v) in iter_vars {
                ev.insert(k.clone(), v.clone());
            }
            cloned.expand_vars = if ev.is_empty() { None } else { Some(ev) };
        }

        // Clone labels
        cloned.labels = body_step.labels.clone();

        // Clone dependencies - only prefix references to steps WITHIN the loop body
        cloned.depends_on =
            rewrite_loop_dependencies(&body_step.depends_on, &step.id, iteration, &body_step_ids);
        cloned.needs =
            rewrite_loop_dependencies(&body_step.needs, &step.id, iteration, &body_step_ids);

        // Recursively handle children with proper dependency rewriting
        if let Some(children) = &body_step.children {
            if !children.is_empty() {
                cloned.children = Some(expand_loop_children(
                    children,
                    &step.id,
                    iteration,
                    &body_step_ids,
                ));
            }
        }

        result.push(cloned);
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// substituteLoopVars
// ---------------------------------------------------------------------------

/// Replace {varname} placeholders with values from vars map.
/// Also replaces {iter} with the iteration number and {step.id} with step ID.
fn substitute_loop_vars(
    s: &str,
    _step_id: &str,
    iteration: i32,
    vars: &HashMap<String, String>,
) -> String {
    let mut result = s.to_string();
    // Replace iteration variable
    result = result.replace("{iter}", &iteration.to_string());
    // Replace custom loop variables
    for (key, value) in vars {
        result = result.replace(&format!("{{{}}}", key), value);
    }
    result
}

// ---------------------------------------------------------------------------
// collectBodyStepIDs
// ---------------------------------------------------------------------------

/// Collect all step IDs within a loop body (including nested children).
fn collect_body_step_ids(body: &[Step]) -> Vec<String> {
    let mut ids = Vec::new();
    let mut stack: Vec<&Step> = body.iter().collect();
    while let Some(s) = stack.pop() {
        ids.push(s.id.clone());
        if let Some(children) = &s.children {
            for child in children {
                stack.push(child);
            }
        }
    }
    ids
}

// ---------------------------------------------------------------------------
// rewriteLoopDependencies
// ---------------------------------------------------------------------------

/// Rewrite dependency references for loop expansion.
/// Only dependencies referencing steps WITHIN the loop body are prefixed.
/// External dependencies are preserved as-is.
fn rewrite_loop_dependencies(
    deps: &[String],
    loop_id: &str,
    iteration: i32,
    body_step_ids: &[String],
) -> Vec<String> {
    if deps.is_empty() {
        return Vec::new();
    }

    deps.iter()
        .map(|dep| {
            if body_step_ids.contains(dep) {
                // Internal dependency - prefix with iteration context
                format!("{}.iter{}.{}", loop_id, iteration, dep)
            } else {
                // External dependency - preserve as-is
                dep.clone()
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// expandLoopChildren
// ---------------------------------------------------------------------------

/// Expand children within a loop iteration.
/// Rewrites IDs and dependencies appropriately.
fn expand_loop_children(
    children: &[Step],
    loop_id: &str,
    iteration: i32,
    body_step_ids: &[String],
) -> Vec<Step> {
    children
        .iter()
        .map(|child| {
            let mut cloned = child.clone();
            cloned.id = format!("{}.iter{}.{}", loop_id, iteration, child.id);
            cloned.depends_on =
                rewrite_loop_dependencies(&child.depends_on, loop_id, iteration, body_step_ids);
            cloned.needs =
                rewrite_loop_dependencies(&child.needs, loop_id, iteration, body_step_ids);

            // Recursively handle nested children
            if let Some(grandchildren) = &child.children {
                if !grandchildren.is_empty() {
                    cloned.children = Some(expand_loop_children(
                        grandchildren,
                        loop_id,
                        iteration,
                        body_step_ids,
                    ));
                }
            }

            cloned
        })
        .collect()
}

// ---------------------------------------------------------------------------
// chainExpandedIterations
// ---------------------------------------------------------------------------

/// Chain iterations AFTER nested loop expansion.
/// Each iteration's first step depends on the previous iteration's last step.
fn chain_expanded_iterations(steps: &[Step], loop_id: &str, count: usize) -> Vec<Step> {
    if steps.is_empty() || count < 2 {
        return steps.to_vec();
    }

    let mut result: Vec<Step> = steps.to_vec();

    // Find the first and last step index of each iteration
    // Iteration N has steps with ID prefix: {loop_id}.iterN.
    let mut iter_first_idx: HashMap<usize, usize> = HashMap::new();
    let mut iter_last_idx: HashMap<usize, usize> = HashMap::new();

    for (i, s) in steps.iter().enumerate() {
        for iter in 1..=count {
            let prefix = format!("{}.iter{}.", loop_id, iter);
            if s.id.starts_with(&prefix) {
                iter_first_idx.entry(iter).or_insert(i);
                iter_last_idx.insert(iter, i);
                break;
            }
        }
    }

    // Chain: first step of iteration N+1 depends on last step of iteration N
    for iter in 2..=count {
        if let (Some(&first_idx), Some(&prev_last_idx)) =
            (iter_first_idx.get(&iter), iter_last_idx.get(&(iter - 1)))
        {
            let last_step_id = result[prev_last_idx].id.clone();
            result[first_idx].needs.push(last_step_id);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// ApplyBranches
// ---------------------------------------------------------------------------

/// Wire fork-join dependency patterns.
/// For each branch rule:
///   - All branch steps depend on the 'from' step
///   - The 'join' step depends on all branch steps
///
/// Returns a new steps vector with dependencies added.
pub fn apply_branches(steps: &[Step], compose: Option<&ComposeRules>) -> Result<Vec<Step>, String> {
    let compose = match compose {
        Some(c) => c,
        None => return Ok(steps.to_vec()),
    };

    let branches = match &compose.branch {
        Some(b) if !b.is_empty() => b,
        _ => return Ok(steps.to_vec()),
    };

    // Clone steps to avoid mutating input
    let mut cloned: Vec<Step> = steps.to_vec();

    for branch in branches {
        // Validate the branch rule
        if branch.from.is_empty() {
            return Err("branch: from is required".to_string());
        }
        if branch.steps.is_empty() {
            return Err("branch: steps is required".to_string());
        }
        if branch.join.is_empty() {
            return Err("branch: join is required".to_string());
        }

        // Verify all steps exist
        if find_step_by_id(&cloned, &branch.from).is_none() {
            return Err(format!("branch: from step {:?} not found", branch.from));
        }
        if find_step_by_id(&cloned, &branch.join).is_none() {
            return Err(format!("branch: join step {:?} not found", branch.join));
        }
        for step_id in &branch.steps {
            if find_step_by_id(&cloned, step_id).is_none() {
                return Err(format!("branch: parallel step {:?} not found", step_id));
            }
        }

        // Add dependencies: branch steps depend on 'from'
        for step_id in &branch.steps {
            add_need_to_step(&mut cloned, step_id, &branch.from);
        }

        // Add dependencies: 'join' depends on all branch steps
        for step_id in &branch.steps {
            add_need_to_step(&mut cloned, &branch.join, step_id);
        }
    }

    Ok(cloned)
}

/// Find a step by ID, searching recursively through children.
fn find_step_by_id<'a>(steps: &'a [Step], target_id: &str) -> Option<&'a Step> {
    for step in steps {
        if step.id == target_id {
            return Some(step);
        }
        if let Some(children) = &step.children {
            if let found @ Some(_) = find_step_by_id(children, target_id) {
                return found;
            }
        }
    }
    None
}

/// Find a step by ID and add a "needs" dependency, searching recursively.
fn add_need_to_step(steps: &mut Vec<Step>, target_id: &str, need_id: &str) -> bool {
    for step in steps.iter_mut() {
        if step.id == target_id {
            if !step.needs.contains(&need_id.to_string()) {
                step.needs.push(need_id.to_string());
            }
            return true;
        }
        if let Some(children) = &mut step.children {
            if add_need_to_step(children, target_id, need_id) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// ApplyGates
// ---------------------------------------------------------------------------

/// Add gate conditions to steps.
/// For each gate rule:
///   - The target step gets a "gate:condition" label
///
/// Returns a new steps vector with gate labels added.
pub fn apply_gates(steps: &[Step], compose: Option<&ComposeRules>) -> Result<Vec<Step>, String> {
    let compose = match compose {
        Some(c) => c,
        None => return Ok(steps.to_vec()),
    };

    let gates = match &compose.gate {
        Some(g) if !g.is_empty() => g,
        _ => return Ok(steps.to_vec()),
    };

    // Clone steps to avoid mutating input
    let mut cloned: Vec<Step> = steps.to_vec();

    for gate in gates {
        if gate.before.is_empty() {
            return Err("gate: before is required".to_string());
        }
        if gate.condition.is_empty() {
            return Err("gate: condition is required".to_string());
        }

        // Find the target step (top-level or nested)
        let found = apply_gate_to_step(&mut cloned, &gate.before, &gate.condition);
        if !found {
            return Err(format!("gate: target step {:?} not found", gate.before));
        }
    }

    Ok(cloned)
}

/// Apply a gate label to a step by ID, searching recursively.
fn apply_gate_to_step(steps: &mut [Step], target_id: &str, condition: &str) -> bool {
    for step in steps.iter_mut() {
        if step.id == target_id {
            let gate_label = format!(
                "gate:{{\"condition\":\"{}\"}}",
                condition.replace('"', "\\\"")
            );
            if !step.labels.contains(&gate_label) {
                step.labels.push(gate_label);
            }
            return true;
        }
        if let Some(children) = &mut step.children {
            if apply_gate_to_step(children, target_id, condition) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// ApplyControlFlow
// ---------------------------------------------------------------------------

/// Apply all control flow operators in the correct order:
/// 1. Loops (expand iterations)
/// 2. Branches (wire fork-join dependencies)
/// 3. Gates (add condition labels)
///
/// Returns a new steps vector. The original steps vector is not modified.
pub fn apply_control_flow(
    steps: &[Step],
    compose: Option<&ComposeRules>,
) -> Result<Vec<Step>, String> {
    // Apply loops first (expands steps)
    let mut result = apply_loops(steps)?;

    // Apply branches (wires dependencies)
    result = apply_branches(&result, compose)?;

    // Apply gates (adds labels)
    result = apply_gates(&result, compose)?;

    Ok(result)
}

// ---------------------------------------------------------------------------
// cloneLoopSpec
// ---------------------------------------------------------------------------

/// Create a deep copy of a LoopSpec.
fn clone_loop_spec(loop_spec: Option<&LoopSpec>) -> Option<LoopSpec> {
    loop_spec.map(|ls| LoopSpec {
        count: ls.count,
        until: ls.until.clone(),
        max: ls.max,
        range: ls.range.clone(),
        var: ls.var.clone(),
        body: ls.body.clone(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_body_step(id: &str, title: &str) -> Step {
        Step {
            id: id.to_string(),
            title: Some(title.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_apply_loops_fixed_count() {
        let steps = vec![Step {
            id: "process".to_string(),
            title: Some("Process items".to_string()),
            r#loop: Some(LoopSpec {
                count: Some(3),
                body: vec![
                    make_body_step("fetch", "Fetch item"),
                    Step {
                        id: "transform".to_string(),
                        title: Some("Transform item".to_string()),
                        needs: vec!["fetch".to_string()],
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        }];

        let result = apply_loops(&steps).unwrap();

        // Should have 6 steps (3 iterations * 2 steps each)
        assert_eq!(result.len(), 6);

        // Check step IDs
        let expected_ids = vec![
            "process.iter1.fetch",
            "process.iter1.transform",
            "process.iter2.fetch",
            "process.iter2.transform",
            "process.iter3.fetch",
            "process.iter3.transform",
        ];

        for (i, expected) in expected_ids.iter().enumerate() {
            assert_eq!(
                result[i].id, *expected,
                "Step {}: expected ID {}, got {}",
                i, expected, result[i].id
            );
        }

        // Check that inner dependencies are preserved (within same iteration)
        let transform1 = &result[1];
        assert_eq!(
            transform1.needs,
            vec!["process.iter1.fetch"],
            "transform1 should need process.iter1.fetch, got {:?}",
            transform1.needs
        );

        // Check that iterations are chained (iter2 depends on iter1)
        let fetch2 = &result[2];
        assert!(
            fetch2
                .needs
                .contains(&"process.iter1.transform".to_string()),
            "iter2.fetch should need iter1.transform, got {:?}",
            fetch2.needs
        );
    }

    #[test]
    fn test_apply_loops_conditional() {
        let steps = vec![Step {
            id: "retry".to_string(),
            title: Some("Retry operation".to_string()),
            r#loop: Some(LoopSpec {
                until: Some("step.status == 'complete'".to_string()),
                max: Some(5),
                body: vec![make_body_step("attempt", "Attempt operation")],
                ..Default::default()
            }),
            ..Default::default()
        }];

        let result = apply_loops(&steps).unwrap();

        // Conditional loops expand once (runtime re-executes)
        assert_eq!(result.len(), 1);

        // Should have loop metadata label
        let step = &result[0];
        let has_loop_label = step.labels.iter().any(|l| l.starts_with("loop:"));
        assert!(has_loop_label, "Missing loop metadata label");

        // Check the format
        if let Some(label) = step.labels.iter().find(|l| l.starts_with("loop:")) {
            assert!(
                label.contains("\"until\""),
                "Loop label missing until field: {}",
                label
            );
            assert!(
                label.contains("\"max\""),
                "Loop label missing max field: {}",
                label
            );
        }
    }

    #[test]
    fn test_validate_loop_spec() {
        // Empty body
        let err = validate_loop_spec(
            &LoopSpec {
                count: Some(3),
                body: vec![],
                ..Default::default()
            },
            "test",
        )
        .unwrap_err();
        assert!(err.contains("body is required"));

        // Both count and until
        let err = validate_loop_spec(
            &LoopSpec {
                count: Some(3),
                until: Some("cond".to_string()),
                max: Some(5),
                body: vec![make_body_step("a", "A")],
                ..Default::default()
            },
            "test",
        )
        .unwrap_err();
        assert!(err.contains("only one of count, until, or range"));
    }

    #[test]
    fn test_apply_branches() {
        let steps = vec![
            make_body_step("setup", "Setup"),
            make_body_step("test", "Run tests"),
            make_body_step("lint", "Run linter"),
            make_body_step("build", "Build"),
            make_body_step("deploy", "Deploy"),
        ];

        let compose = ComposeRules {
            branch: Some(vec![BranchRule {
                from: "setup".to_string(),
                steps: vec!["test".to_string(), "lint".to_string(), "build".to_string()],
                join: "deploy".to_string(),
            }]),
            ..Default::default()
        };

        let result = apply_branches(&steps, Some(&compose)).unwrap();

        // Build step map for checking
        let mut step_map: HashMap<String, Step> = HashMap::new();
        for s in &result {
            step_map.insert(s.id.clone(), s.clone());
        }

        // Verify branch steps depend on 'from'
        for branch_step in &["test", "lint", "build"] {
            let s = step_map.get(*branch_step).unwrap();
            assert!(
                s.needs.contains(&"setup".to_string()),
                "Step {} should need 'setup', got {:?}",
                branch_step,
                s.needs
            );
        }

        // Verify 'join' depends on all branch steps
        let deploy = step_map.get("deploy").unwrap();
        for branch_step in &["test", "lint", "build"] {
            assert!(
                deploy.needs.contains(&branch_step.to_string()),
                "deploy should need {}, got {:?}",
                branch_step,
                deploy.needs
            );
        }
    }

    #[test]
    fn test_apply_gates() {
        let steps = vec![
            make_body_step("step1", "Step 1"),
            make_body_step("step2", "Step 2"),
        ];

        let compose = ComposeRules {
            gate: Some(vec![crate::formula::types::GateRule {
                before: "step2".to_string(),
                condition: "step.status == 'complete'".to_string(),
            }]),
            ..Default::default()
        };

        let result = apply_gates(&steps, Some(&compose)).unwrap();

        // step1 should have no gate label
        let step1 = result.iter().find(|s| s.id == "step1").unwrap();
        assert!(!step1.labels.iter().any(|l| l.starts_with("gate:")));

        // step2 should have a gate label
        let step2 = result.iter().find(|s| s.id == "step2").unwrap();
        assert!(
            step2.labels.iter().any(|l| l.starts_with("gate:")),
            "step2 should have gate label, got labels: {:?}",
            step2.labels
        );
    }

    #[test]
    fn test_apply_control_flow_order() {
        let steps = vec![
            make_body_step("setup", "Setup"),
            Step {
                id: "process".to_string(),
                title: Some("Process items".to_string()),
                r#loop: Some(LoopSpec {
                    count: Some(2),
                    body: vec![make_body_step("item", "Process item")],
                    ..Default::default()
                }),
                ..Default::default()
            },
            make_body_step("finalize", "Finalize"),
        ];

        let compose = ComposeRules {
            branch: Some(vec![BranchRule {
                from: "setup".to_string(),
                steps: vec!["process.iter1.item".to_string()],
                join: "finalize".to_string(),
            }]),
            ..Default::default()
        };

        let result = apply_control_flow(&steps, Some(&compose));

        // The loop expands first, then branches are applied.
        // The branch should reference the expanded loop step IDs.
        // If the branch references don't match expanded IDs, it should still work.
        assert!(
            result.is_ok(),
            "control flow should be applied successfully"
        );
    }

    #[test]
    fn test_expand_range_loop() {
        let step = Step {
            id: "build".to_string(),
            title: Some("Build".to_string()),
            r#loop: Some(LoopSpec {
                range: Some("1..3".to_string()),
                var: Some("index".to_string()),
                body: vec![Step {
                    id: "compile".to_string(),
                    title: Some("Compile {index}".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = apply_loops(&[step]).unwrap();

        // Should have 3 steps (iterations 1, 2, 3)
        assert_eq!(result.len(), 3);

        // Check step IDs
        assert_eq!(result[0].id, "build.iter1.compile");
        assert_eq!(result[1].id, "build.iter2.compile");
        assert_eq!(result[2].id, "build.iter3.compile");

        // Check iteration chaining
        assert!(
            result[1].needs.contains(&"build.iter1.compile".to_string()),
            "build.iter2 should depend on build.iter1.compile"
        );
        assert!(
            result[2].needs.contains(&"build.iter2.compile".to_string()),
            "build.iter3 should depend on build.iter2.compile"
        );
    }
}
