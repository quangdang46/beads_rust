//! Formula parser — loads, validates, and resolves `.formula.json` files.
//!
//! Ported from Go beads /internal/formula/parser.go.
//! Supports JSON and TOML formula files with inheritance via `extends`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::formula::types::*;

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Handles loading and resolving formulas with caching and cycle detection.
pub struct Parser {
    /// Directories to search for formulas (in order).
    search_paths: Vec<PathBuf>,
    /// Cache of loaded formulas by name and absolute path.
    cache: HashMap<String, Formula>,
    /// Formulas currently being resolved (for cycle detection).
    resolving_set: HashSet<String>,
    /// Order of formula resolution (for error messages).
    resolving_chain: Vec<String>,
}

impl Parser {
    /// Create a new parser with the given search paths.
    /// If empty, uses default search paths.
    pub fn new(search_paths: Vec<PathBuf>) -> Self {
        let paths = if search_paths.is_empty() {
            Self::default_search_paths()
        } else {
            search_paths
        };
        Self {
            search_paths: paths,
            cache: HashMap::new(),
            resolving_set: HashSet::new(),
            resolving_chain: Vec::new(),
        }
    }

    /// Get the default formula search paths.
    fn default_search_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        // Project-level: .beads/formulas/
        if let Ok(cwd) = std::env::current_dir() {
            paths.push(cwd.join(".beads").join("formulas"));
        }
        // User-level: ~/.beads/formulas/
        if let Ok(home) = std::env::var("HOME") {
            paths.push(PathBuf::from(home).join(".beads").join("formulas"));
        }
        // GT_ROOT: orchestrator formulas
        if let Ok(gt_root) = std::env::var("GT_ROOT") {
            paths.push(PathBuf::from(gt_root).join(".beads").join("formulas"));
        }
        paths
    }

    /// Parse a formula from a file path.
    /// Detects format from extension: .formula.json or .formula.toml.
    pub fn parse_file(&mut self, path: &Path) -> Result<Formula, String> {
        let abs_path = path
            .canonicalize()
            .map_err(|e| format!("resolve path: {}", e))?;
        let path_str = abs_path.to_string_lossy().to_string();

        // Check cache
        if let Some(cached) = self.cache.get(&path_str) {
            return Ok(cached.clone());
        }
        if let Some(cached) = self.cache.get(path_str.as_str()) {
            return Ok(cached.clone());
        }

        // Read file
        let data = fs::read(&abs_path)
            .map_err(|e| format!("read {}: {}", path.display(), e))?;

        // Detect format from extension
        let mut formula = if path_str.ends_with(".formula.toml") {
            Self::parse_toml(&data)?
        } else {
            Self::parse_json(&data)?
        };

        formula.source = Some(path_str.clone());

        // Set source tracing info on all steps
        set_source_info(&mut formula);

        // Cache by absolute path and formula name
        self.cache.insert(path_str, formula.clone());
        self.cache.insert(formula.formula.clone(), formula.clone());

        Ok(formula)
    }

    /// Parse a formula from JSON bytes.
    pub fn parse_json(data: &[u8]) -> Result<Formula, String> {
        let mut formula: Formula =
            serde_json::from_slice(data).map_err(|e| format!("json parse error: {}", e))?;
        // Set defaults
        if formula.version == 0 {
            formula.version = 1;
        }
        Ok(formula)
    }

    /// Parse a formula from TOML bytes.
    pub fn parse_toml(data: &[u8]) -> Result<Formula, String> {
        let data_str =
            std::str::from_utf8(data).map_err(|e| format!("toml encoding error: {}", e))?;
        let mut formula: Formula =
            toml::from_str(data_str).map_err(|e| format!("toml parse error: {}", e))?;
        // Set defaults
        if formula.version == 0 {
            formula.version = 1;
        }
        Ok(formula)
    }

    /// Resolve a formula, processing extends (inheritance chain).
    /// Returns a new formula with all inheritance applied.
    pub fn resolve(&mut self, formula: &Formula) -> Result<Formula, String> {
        // Cycle detection
        if self.resolving_set.contains(&formula.formula) {
            let chain = self
                .resolving_chain
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>();
            return Err(format!(
                "circular extends detected: {}",
                chain.join(" -> ")
            ));
        }
        self.resolving_set.insert(formula.formula.clone());
        self.resolving_chain.push(formula.formula.clone());

        // If no extends, just validate and return
        if formula.extends.is_empty() {
            self.resolving_set.remove(&formula.formula);
            self.resolving_chain.pop();
            formula.validate()?;
            return Ok(formula.clone());
        }

        // Build merged formula from parents
        let mut merged = Formula {
            formula: formula.formula.clone(),
            description: formula.description.clone(),
            version: formula.version,
            r#type: formula.r#type.clone(),
            source: formula.source.clone(),
            vars: formula.vars.clone(),
            steps: None,
            compose: None,
            ..Default::default()
        };

        // Track all resolved vars and steps for merging
        let mut all_vars: Vec<VarDef> = Vec::new();
        let mut all_steps: Vec<Step> = Vec::new();
        let mut merged_compose: Option<ComposeRules> = None;

        // Process each parent
        for parent_name in &formula.extends {
            let parent = self.load_formula(parent_name)?;
            let parent = self.resolve(&parent)?;

            // Merge vars: parent vars are inherited, child overrides
            if let Some(p_vars) = &parent.vars {
                for pv in p_vars {
                    if !all_vars.iter().any(|v| v.name == pv.name) {
                        all_vars.push(pv.clone());
                    }
                }
            }

            // Merge steps: parent steps are prepended
            if let Some(p_steps) = &parent.steps {
                let existing_ids: HashSet<String> =
                    all_steps.iter().map(|s| s.id.clone()).collect();
                for ps in p_steps {
                    if !existing_ids.contains(&ps.id) {
                        all_steps.push(ps.clone());
                    }
                }
            }

            // Merge compose rules
            merged_compose = merge_compose_rules(merged_compose.as_ref(), parent.compose.as_ref());
        }

        // Apply child overrides: child vars override parent vars
        if let Some(c_vars) = &formula.vars {
            for cv in c_vars {
                if let Some(existing) = all_vars.iter_mut().find(|v| v.name == cv.name) {
                    *existing = cv.clone();
                } else {
                    all_vars.push(cv.clone());
                }
            }
        }

        // Merge child steps: override by ID, append new
        merge_steps_into(&mut all_steps, formula.steps.as_deref().unwrap_or(&[]));

        // Apply child compose overrides
        merged_compose =
            merge_compose_rules(merged_compose.as_ref(), formula.compose.as_ref());

        // Use child description if set
        if formula.description.is_some() {
            merged.description = formula.description.clone();
        }

        merged.vars = if all_vars.is_empty() {
            None
        } else {
            Some(all_vars)
        };
        merged.steps = if all_steps.is_empty() {
            None
        } else {
            Some(all_steps)
        };
        merged.compose = merged_compose;

        // Validate the merged result
        merged.validate()?;

        self.resolving_set.remove(&formula.formula);
        self.resolving_chain.pop();

        Ok(merged)
    }

    /// Load a formula by name from search paths.
    /// Tries .formula.toml first, then .formula.json.
    fn load_formula(&mut self, name: &str) -> Result<Formula, String> {
        // Check cache first
        if let Some(cached) = self.cache.get(name) {
            return Ok(cached.clone());
        }

        // Search paths
        let extensions = [".formula.toml", ".formula.json"];
        for dir in &self.search_paths {
            for ext in &extensions {
                let path = dir.join(format!("{}{}", name, ext));
                if path.exists() {
                    return self.parse_file(&path);
                }
            }
        }

        Err(format!("formula {:?} not found in search paths", name))
    }

    /// Load a formula by name (public API).
    pub fn load_by_name(&mut self, name: &str) -> Result<Formula, String> {
        self.load_formula(name)
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

impl Formula {
    /// Validate the formula for structural errors.
    pub fn validate(&self) -> Result<(), String> {
        let mut errors: Vec<String> = Vec::new();

        if self.formula.is_empty() {
            errors.push("formula: name is required".to_string());
        }

        if self.version < 1 {
            errors.push("version: must be >= 1".to_string());
        }

        if !self.r#type.is_valid() {
            errors.push(format!(
                "type: invalid value {:?} (must be workflow, expansion, aspect, or convoy)",
                self.r#type
            ));
        }

        // Validate variables
        if let Some(vars) = &self.vars {
            for v in vars {
                if v.name.is_empty() {
                    errors.push("vars: variable name cannot be empty".to_string());
                    continue;
                }
                if v.required && v.default.is_some() {
                    errors.push(format!(
                        "vars.{}: cannot have both required:true and default",
                        v.name
                    ));
                }
            }
        }

        // Validate steps — track IDs and locations
        let mut step_id_locations: HashMap<String, String> = HashMap::new();
        if let Some(steps) = &self.steps {
            for (i, step) in steps.iter().enumerate() {
                let prefix = format!("steps[{}]", i);
                if let Err(e) = validate_step(step, &prefix, &mut step_id_locations) {
                    errors.push(e);
                }
            }

            // Validate step dependencies reference valid IDs
            for (i, step) in steps.iter().enumerate() {
                for dep in &step.depends_on {
                    if !step_id_locations.contains_key(dep) {
                        errors.push(format!(
                            "steps[{}] ({}): depends_on references unknown step {:?}",
                            i, step.id, dep
                        ));
                    }
                }
                for need in &step.needs {
                    if !step_id_locations.contains_key(need) {
                        errors.push(format!(
                            "steps[{}] ({}): needs references unknown step {:?}",
                            i, step.id, need
                        ));
                    }
                }
                // Validate children's deps recursively
                if let Some(children) = &step.children {
                    validate_child_deps(children, &step_id_locations, &mut errors, &format!("steps[{}]", i));
                }
            }
        }

        // Validate compose rules
        if let Some(compose) = &self.compose {
            if let Some(bond_points) = &compose.bond_points {
                for (i, bp) in bond_points.iter().enumerate() {
                    if bp.id.is_empty() {
                        errors.push(format!("compose.bond_points[{}]: id is required", i));
                    }
                    if bp.after_step.is_some() && bp.before_step.is_some() {
                        errors.push(format!(
                            "compose.bond_points[{}] ({}): cannot have both after_step and before_step",
                            i, bp.id
                        ));
                    }
                    if let Some(after) = &bp.after_step {
                        if !step_id_locations.contains_key(after) {
                            errors.push(format!(
                                "compose.bond_points[{}] ({}): after_step references unknown step {:?}",
                                i, bp.id, after
                            ));
                        }
                    }
                    if let Some(before) = &bp.before_step {
                        if !step_id_locations.contains_key(before) {
                            errors.push(format!(
                                "compose.bond_points[{}] ({}): before_step references unknown step {:?}",
                                i, bp.id, before
                            ));
                        }
                    }
                }
            }

            if let Some(hooks) = &compose.hooks {
                for (i, hook) in hooks.iter().enumerate() {
                    if hook.trigger.is_empty() {
                        errors.push(format!("compose.hooks[{}]: trigger is required", i));
                    }
                    if hook.attach.is_empty() {
                        errors.push(format!("compose.hooks[{}]: attach is required", i));
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "formula validation failed:\n  - {}",
                errors.join("\n  - ")
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Step validation helpers
// ---------------------------------------------------------------------------

fn validate_step(
    step: &Step,
    prefix: &str,
    id_locations: &mut HashMap<String, String>,
) -> Result<(), String> {
    if step.id.is_empty() {
        return Err(format!("{}: id is required", prefix));
    }
    if let Some(first_loc) = id_locations.get(&step.id) {
        return Err(format!(
            "{}: duplicate id {:?} (first defined at {})",
            prefix, step.id, first_loc
        ));
    }
    id_locations.insert(step.id.clone(), prefix.to_string());

    if step.title.is_none() && step.expand.is_none() {
        return Err(format!(
            "{} ({}): title is required (unless using expand)",
            prefix, step.id
        ));
    }

    // Validate priority range
    if let Some(p) = step.priority {
        if !(0..=4).contains(&p) {
            return Err(format!(
                "{} ({}): priority must be 0-4",
                prefix, step.id
            ));
        }
    }

    // Validate children
    if let Some(children) = &step.children {
        for (i, child) in children.iter().enumerate() {
            let child_prefix = format!("{}.children[{}]", prefix, i);
            validate_step(child, &child_prefix, id_locations)?;
        }
    }

    Ok(())
}

fn validate_child_deps(
    children: &[Step],
    id_locations: &HashMap<String, String>,
    errors: &mut Vec<String>,
    prefix: &str,
) {
    for (i, child) in children.iter().enumerate() {
        let child_prefix = format!("{}.children[{}]", prefix, i);
        for dep in &child.depends_on {
            if !id_locations.contains_key(dep) {
                errors.push(format!(
                    "{} ({}): depends_on references unknown step {:?}",
                    child_prefix, child.id, dep
                ));
            }
        }
        for need in &child.needs {
            if !id_locations.contains_key(need) {
                errors.push(format!(
                    "{} ({}): needs references unknown step {:?}",
                    child_prefix, child.id, need
                ));
            }
        }
        if let Some(grandchildren) = &child.children {
            validate_child_deps(grandchildren, id_locations, errors, &child_prefix);
        }
    }
}

// ---------------------------------------------------------------------------
// Merge helpers
// ---------------------------------------------------------------------------

/// Merge compose rules (parent + child).
fn merge_compose_rules(
    parent: Option<&ComposeRules>,
    child: Option<&ComposeRules>,
) -> Option<ComposeRules> {
    match (parent, child) {
        (None, None) => None,
        (Some(p), None) => Some(p.clone()),
        (None, Some(c)) => Some(c.clone()),
        (Some(p), Some(c)) => {
            let mut merged = p.clone();
            // Merge bond points: child overrides parent by id
            if let Some(c_bps) = &c.bond_points {
                let mut bps = merged.bond_points.unwrap_or_default();
                for cbp in c_bps {
                    if let Some(pos) = bps.iter().position(|b| b.id == cbp.id) {
                        bps[pos] = cbp.clone();
                    } else {
                        bps.push(cbp.clone());
                    }
                }
                merged.bond_points = Some(bps);
            }
            // Expand, Map, Branch, Gate: child overrides
            if c.expand.is_some() {
                merged.expand = c.expand.clone();
            }
            if c.r#map.is_some() {
                merged.r#map = c.r#map.clone();
            }
            if c.branch.is_some() {
                merged.branch = c.branch.clone();
            }
            if c.gate.is_some() {
                merged.gate = c.gate.clone();
            }
            if c.aspects.is_some() {
                merged.aspects = c.aspects.clone();
            }
            Some(merged)
        }
    }
}

/// Merge child steps into parent steps.
/// Child steps override parent steps by ID; new child steps are appended.
fn merge_steps_into(parent_steps: &mut Vec<Step>, child_steps: &[Step]) {
    for cs in child_steps {
        if let Some(pos) = parent_steps.iter().position(|ps| ps.id == cs.id) {
            parent_steps[pos] = cs.clone();
        } else {
            parent_steps.push(cs.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Source tracing
// ---------------------------------------------------------------------------

/// Set source tracking info on all steps recursively.
pub fn set_source_info(formula: &mut Formula) {
    let formula_name = formula.formula.clone();
    if let Some(steps) = &mut formula.steps {
        for (i, step) in steps.iter_mut().enumerate() {
            set_step_source(step, &formula_name, &format!("steps[{}]", i));
        }
    }
    if let Some(template) = &mut formula.template {
        for (i, step) in template.iter_mut().enumerate() {
            set_step_source(step, &formula_name, &format!("template[{}]", i));
        }
    }
}

fn set_step_source(step: &mut Step, formula_name: &str, location: &str) {
    step.source_formula = Some(formula_name.to_string());
    step.source_location = Some(location.to_string());
    if let Some(children) = &mut step.children {
        for (i, child) in children.iter_mut().enumerate() {
            set_step_source(child, formula_name, &format!("{}.children[{}]", location, i));
        }
    }
}

// ---------------------------------------------------------------------------
// Variable substitution
// ---------------------------------------------------------------------------

/// Substitute {{variable}} placeholders in a string with actual values.
pub fn substitute_vars(template: &str, vars: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        let placeholder = format!("{{{{{}}}}}", key);
        result = result.replace(&placeholder, value);
    }
    result
}

// ---------------------------------------------------------------------------
// Default trait implementations
// ---------------------------------------------------------------------------

impl Default for Formula {
    fn default() -> Self {
        Self {
            formula: String::new(),
            description: None,
            version: 1,
            r#type: FormulaType::Workflow,
            extends: Vec::new(),
            vars: None,
            steps: None,
            template: None,
            compose: None,
            advice: None,
            pointcuts: None,
            phase: None,
            pour: false,
            source: None,
        }
    }
}

impl Default for Step {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: None,
            description: None,
            notes: None,
            r#type: None,
            priority: None,
            labels: Vec::new(),
            metadata: None,
            depends_on: Vec::new(),
            needs: Vec::new(),
            waits_for: None,
            assignee: None,
            expand: None,
            expand_vars: None,
            condition: None,
            children: None,
            gate: None,
            r#loop: None,
            on_complete: None,
            source_formula: None,
            source_location: None,
        }
    }
}
