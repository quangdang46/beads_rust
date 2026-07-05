//! Mol / Swarm command tree — molecule management for agent workflows.
//!
//! Subcommands:
//!   bond <source> <target>       — Bond two protos or molecules together
//!   pour <id> [--var key=val]    — Instantiate a proto as persistent mol
//!   swarm validate <epic-id>     — Validate epic structure for swarming
//!   swarm create <epic-id>       — Create a swarm molecule from an epic

use crate::config;
use crate::error::{BeadsError, Result};
use crate::model::{DependencyType, Issue, IssueType, MolType, Status};
use crate::output::OutputContext;
use crate::storage::SqliteStorage;
use crate::util::id::IdResolver;
use chrono::Utc;
use serde::Serialize;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// CLI argument types (re-exported from cli::MolCommands)
// ---------------------------------------------------------------------------

/// Bond two protos or molecules together.
#[derive(clap::Args, Debug, Clone)]
pub struct MolBondArgs {
    /// Source issue ID (the thing being bonded FROM)
    pub source: String,
    /// Target issue ID (the thing being bonded TO)
    pub target: String,
    /// Bond type: sequential (default), parallel, or conditional
    #[arg(long, default_value = "sequential")]
    pub bond_type: String,
    /// Dry-run: preview without writing
    #[arg(long)]
    pub dry_run: bool,
}

/// Instantiate a proto as persistent mol.
#[derive(clap::Args, Debug, Clone)]
pub struct MolPourArgs {
    /// Proto ID to pour from
    pub id: String,
    /// Variable substitution (key=value)
    #[arg(long = "var", value_name = "KEY=VALUE")]
    pub var: Vec<String>,
    /// Assign the root issue to this agent/user
    #[arg(long)]
    pub assignee: Option<String>,
    /// Dry-run: preview without writing
    #[arg(long)]
    pub dry_run: bool,
}

/// Validate epic structure for swarming.
#[derive(clap::Args, Debug, Clone)]
pub struct MolSwarmValidateArgs {
    /// Epic ID to validate
    pub epic_id: String,
}

/// Create a swarm molecule from an epic.
#[derive(clap::Args, Debug, Clone)]
pub struct MolSwarmCreateArgs {
    /// Epic ID to create a swarm for
    pub epic_id: String,
    /// Coordinator assignment for the swarm (optional)
    #[arg(long)]
    pub coordinator: Option<String>,
    /// Create swarm even if one already exists
    #[arg(long)]
    pub force: bool,
}

// ---------------------------------------------------------------------------
// Mol commands enum
// ---------------------------------------------------------------------------

#[derive(clap::Subcommand, Debug)]
pub enum MolCommands {
    /// Bond two protos or molecules together
    Bond(MolBondArgs),
    /// Instantiate a proto as persistent mol
    Pour(MolPourArgs),
    /// Swarm management subcommands
    Swarm {
        #[command(subcommand)]
        command: MolSwarmCommands,
    },
}

#[derive(clap::Subcommand, Debug)]
pub enum MolSwarmCommands {
    /// Validate epic structure for swarming
    Validate(MolSwarmValidateArgs),
    /// Create a swarm molecule from an epic
    Create(MolSwarmCreateArgs),
}

// ---------------------------------------------------------------------------
// Execute entry point
// ---------------------------------------------------------------------------

pub fn execute(
    command: &MolCommands,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let mut storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    let config_layer = storage_ctx.load_config(cli)?;
    let actor = config::resolve_actor(&config_layer);
    let storage = &mut storage_ctx.storage;

    match command {
        MolCommands::Bond(args) => execute_bond(args, storage, &actor, ctx),
        MolCommands::Pour(args) => execute_pour(args, storage, &actor, ctx),
        MolCommands::Swarm { command } => match command {
            MolSwarmCommands::Validate(args) => execute_swarm_validate(args, storage, ctx),
            MolSwarmCommands::Create(args) => execute_swarm_create(args, storage, &actor, ctx),
        },
    }
}

// ---------------------------------------------------------------------------
// Helper: resolve an issue ID from input
// ---------------------------------------------------------------------------

fn resolve_id(storage: &SqliteStorage, input: &str) -> Result<String> {
    let prefix = storage
        .get_config("issue_prefix")
        .ok()
        .flatten()
        .unwrap_or_else(|| "bd".to_string());
    let resolver = IdResolver::new(crate::util::id::ResolverConfig::with_prefix(prefix));
    resolver
        .resolve_fallible(
            input,
            |id| storage.id_exists(id),
            |hash| storage.find_ids_by_hash(hash),
        )
        .map(|r| r.id)
}

// ---------------------------------------------------------------------------
// bond command
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct BondOutput {
    source: String,
    target: String,
    bond_type: String,
    result: String,
    message: String,
}

fn execute_bond(
    args: &MolBondArgs,
    storage: &mut SqliteStorage,
    actor: &str,
    ctx: &OutputContext,
) -> Result<()> {
    let source_id = resolve_id(storage, &args.source)?;
    let target_id = resolve_id(storage, &args.target)?;

    // Validate bond type
    let bond_type = args.bond_type.to_lowercase();
    if !["sequential", "parallel", "conditional"].contains(&bond_type.as_str()) {
        return Err(BeadsError::Validation {
            field: "bond_type".to_string(),
            reason: format!(
                "invalid bond type '{bond_type}', must be: sequential, parallel, or conditional"
            ),
        });
    }

    // Resolve to dependency type
    let dep_type_str = match bond_type.as_str() {
        "sequential" => "blocks",
        "conditional" => "conditional-blocks",
        _ => "parent-child", // parallel
    };

    if args.dry_run {
        if ctx.is_json() || ctx.is_toon() {
            ctx.json_pretty(&BondOutput {
                source: source_id.clone(),
                target: target_id.clone(),
                bond_type: bond_type.clone(),
                result: "dry_run".to_string(),
                message: format!(
                    "Would bond {} -> {} with type '{}'",
                    source_id, target_id, bond_type
                ),
            });
        } else {
            ctx.info(&format!(
                "Dry run: would bond {} -> {} (type: {})",
                source_id, target_id, bond_type
            ));
        }
        return Ok(());
    }

    // Create the dependency: target depends on source with the resolved dep type
    storage.add_dependency(&target_id, &source_id, dep_type_str, actor)?;

    // Add metadata to the dependency to record bonding info
    let metadata = serde_json::json!({
        "bond_type": bond_type,
        "bond_point": "",
    });
    storage.add_dependency_with_metadata(
        &target_id,
        &source_id,
        dep_type_str,
        actor,
        Some(&metadata.to_string()),
    )?;

    if ctx.is_json() || ctx.is_toon() {
        ctx.json_pretty(&BondOutput {
            source: source_id.clone(),
            target: target_id.clone(),
            bond_type: bond_type.clone(),
            result: "ok".to_string(),
            message: format!(
                "Bonded {} -> {} with type '{}'",
                source_id, target_id, bond_type
            ),
        });
    } else {
        ctx.success(&format!(
            "Bonded {} -> {} (type: {})",
            source_id, target_id, bond_type
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// pour command
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct PourOutput {
    proto_id: String,
    created_count: usize,
    root_id: Option<String>,
    phase: String,
}

fn execute_pour(
    args: &MolPourArgs,
    storage: &mut SqliteStorage,
    actor: &str,
    ctx: &OutputContext,
) -> Result<()> {
    let proto_id = resolve_id(storage, &args.id)?;

    // Verify the source issue exists and is a template/proto
    let proto = storage
        .get_issue(&proto_id)?
        .ok_or_else(|| BeadsError::IssueNotFound {
            id: proto_id.clone(),
        })?;

    // Parse variables (reserved for future use in template substitution)
    let _vars = parse_vars(&args.var);

    if args.dry_run {
        let count = storage.get_dependents(&proto_id)?.len();
        let children_count = count;
        if ctx.is_json() || ctx.is_toon() {
            ctx.json_pretty(&PourOutput {
                proto_id: proto_id.clone(),
                created_count: children_count + 1,
                root_id: None,
                phase: "liquid".to_string(),
            });
        } else {
            ctx.info(&format!(
                "Dry run: would pour {} issues from proto {} (phase: liquid)",
                children_count + 1,
                proto_id
            ));
        }
        return Ok(());
    }

    // Create a persistent molecule from the proto
    // The poured mol gets the issue_type Molecule, status Open
    let mut new_issue = proto.clone();
    new_issue.id = format!("{}-mol-{}", proto_id, chrono::Utc::now().timestamp());
    // Ensure unique ID by checking existence
    let final_id = ensure_unique_id(storage, &new_issue.id)?;
    new_issue.id = final_id.clone();
    new_issue.issue_type = IssueType::Molecule;
    new_issue.mol_type = MolType::Work;
    new_issue.status = Status::Open;
    new_issue.created_at = Utc::now();
    new_issue.updated_at = Utc::now();
    new_issue.created_by = Some(actor.to_string());
    new_issue.is_template = false;
    new_issue.source_formula = Some(proto_id.clone());
    if let Some(assignee) = &args.assignee {
        new_issue.assignee = Some(assignee.clone());
    }

    storage.create_issue(&new_issue, actor)?;

    // Wire a dependency: new molecule is "related to" the proto
    storage.add_dependency(&final_id, &proto_id, "related", actor)?;

    if ctx.is_json() || ctx.is_toon() {
        ctx.json_pretty(&PourOutput {
            proto_id,
            created_count: 1,
            root_id: Some(final_id.clone()),
            phase: "liquid".to_string(),
        });
    } else {
        ctx.success(&format!(
            "Poured mol: created issue {} from proto {}",
            final_id, args.id
        ));
    }

    Ok(())
}

/// Ensure an ID is unique by appending a suffix if needed.
fn ensure_unique_id(storage: &SqliteStorage, base_id: &str) -> Result<String> {
    if !storage.id_exists(base_id)? {
        return Ok(base_id.to_string());
    }
    for i in 1..100 {
        let candidate = format!("{base_id}-{i}");
        if !storage.id_exists(&candidate)? {
            return Ok(candidate);
        }
    }
    Err(BeadsError::Internal {
        message: format!("could not generate unique ID from base '{base_id}' after 100 attempts"),
    })
}

/// Parse --var "key=value" arguments into a HashMap.
fn parse_vars(var_args: &[String]) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    for v in var_args {
        if let Some((key, value)) = v.split_once('=') {
            vars.insert(key.to_string(), value.to_string());
        }
    }
    vars
}

// ---------------------------------------------------------------------------
// swarm validate command
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone)]
struct ReadyFront {
    wave: usize,
    issues: Vec<String>,
    titles: Vec<String>,
}

#[derive(Serialize, Debug, Clone)]
struct IssueNode {
    id: String,
    title: String,
    status: String,
    depends_on: Vec<String>,
    depended_on_by: Vec<String>,
    wave: i32,
}

#[derive(Serialize)]
struct SwarmAnalysis {
    epic_id: String,
    epic_title: String,
    total_issues: usize,
    closed_issues: usize,
    ready_fronts: Vec<ReadyFront>,
    max_parallelism: usize,
    estimated_sessions: usize,
    warnings: Vec<String>,
    errors: Vec<String>,
    swarmable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    issues: Option<HashMap<String, IssueNode>>,
}

fn execute_swarm_validate(
    args: &MolSwarmValidateArgs,
    storage: &mut SqliteStorage,
    ctx: &OutputContext,
) -> Result<()> {
    let epic_id = resolve_id(storage, &args.epic_id)?;
    let epic = storage
        .get_issue(&epic_id)?
        .ok_or_else(|| BeadsError::IssueNotFound {
            id: epic_id.clone(),
        })?;

    if epic.issue_type != IssueType::Epic && epic.issue_type != IssueType::Molecule {
        return Err(BeadsError::Validation {
            field: "epic_id".to_string(),
            reason: format!(
                "'{}' is not an epic or molecule (type: {})",
                epic_id, epic.issue_type
            ),
        });
    }

    let analysis = analyze_epic_for_swarm(storage, &epic, true)?;

    if ctx.is_json() || ctx.is_toon() {
        ctx.json_pretty(&analysis);
    } else {
        render_swarm_analysis(&analysis, ctx);
    }

    Ok(())
}

fn analyze_epic_for_swarm(
    storage: &SqliteStorage,
    epic: &Issue,
    verbose: bool,
) -> Result<SwarmAnalysis> {
    let mut analysis = SwarmAnalysis {
        epic_id: epic.id.clone(),
        epic_title: epic.title.clone(),
        total_issues: 0,
        closed_issues: 0,
        ready_fronts: Vec::new(),
        max_parallelism: 0,
        estimated_sessions: 0,
        warnings: Vec::new(),
        errors: Vec::new(),
        swarmable: true,
        issues: if verbose { Some(HashMap::new()) } else { None },
    };

    // Get child issues of the epic (parent-child relationships)
    let child_ids = storage.get_dependents(&epic.id)?;
    let children: Vec<Issue> = child_ids
        .iter()
        .filter_map(|id| storage.get_issue(id).ok().flatten())
        .collect();

    if children.is_empty() {
        analysis.warnings.push("Epic has no children".to_string());
        return Ok(analysis);
    }

    analysis.total_issues = children.len();
    for child in &children {
        if child.status.is_terminal() {
            analysis.closed_issues += 1;
        }
    }

    // Get dependency records for all children
    let child_ids_vec: Vec<String> = children.iter().map(|c| c.id.clone()).collect();
    let deps_map = storage.get_dependencies_full_for_issues(&child_ids_vec)?;

    // Build the issue graph
    let mut issue_nodes: HashMap<String, IssueNode> = HashMap::new();
    let child_set: std::collections::HashSet<String> =
        children.iter().map(|c| c.id.clone()).collect();

    for child in &children {
        let mut depends_on = Vec::new();
        if let Some(deps) = deps_map.get(&child.id) {
            for dep in deps {
                // Skip the parent-child relationship to the epic itself
                if dep.depends_on_id == epic.id && dep.dep_type == DependencyType::ParentChild {
                    continue;
                }
                // Only consider blocking dependencies
                if !dep.dep_type.affects_ready_work() {
                    continue;
                }
                // Only track dependencies within the epic's children
                if child_set.contains(&dep.depends_on_id) {
                    depends_on.push(dep.depends_on_id.clone());
                }
            }
        }

        issue_nodes.insert(
            child.id.clone(),
            IssueNode {
                id: child.id.clone(),
                title: child.title.clone(),
                status: child.status.to_string(),
                depends_on: Vec::new(), // populated below
                depended_on_by: Vec::new(),
                wave: -1,
            },
        );
    }

    // Build depends_on and depended_on_by
    for child in &children {
        if let Some(deps) = deps_map.get(&child.id) {
            let mut child_depends_on = Vec::new();
            let mut child_depended_on_by_updates: Vec<(String, String)> = Vec::new();
            for dep in deps {
                if dep.depends_on_id == epic.id && dep.dep_type == DependencyType::ParentChild {
                    continue;
                }
                if !dep.dep_type.affects_ready_work() {
                    continue;
                }
                if child_set.contains(&dep.depends_on_id) {
                    child_depends_on.push(dep.depends_on_id.clone());
                    child_depended_on_by_updates
                        .push((dep.depends_on_id.clone(), child.id.clone()));
                }
                // External dependency warnings
                if !child_set.contains(&dep.depends_on_id) && dep.depends_on_id != epic.id {
                    if dep.depends_on_id.starts_with("external:") {
                        analysis.warnings.push(format!(
                            "{} has external dependency: {}",
                            child.id, dep.depends_on_id
                        ));
                    } else {
                        analysis.warnings.push(format!(
                            "{} depends on {} (outside epic)",
                            child.id, dep.depends_on_id
                        ));
                    }
                }
            }
            if let Some(node) = issue_nodes.get_mut(&child.id) {
                node.depends_on = child_depends_on;
            }
            for (target_id, dep_id) in child_depended_on_by_updates {
                if let Some(target_node) = issue_nodes.get_mut(&target_id) {
                    target_node.depended_on_by.push(dep_id);
                }
            }
        }
    }

    // Detect structural issues
    detect_structural_issues(&mut analysis, &issue_nodes);

    // Compute ready fronts
    compute_ready_fronts(&mut analysis, &issue_nodes);

    analysis.swarmable = analysis.errors.is_empty();

    if verbose {
        analysis.issues = Some(issue_nodes);
    }

    Ok(analysis)
}

fn detect_structural_issues(analysis: &mut SwarmAnalysis, nodes: &HashMap<String, IssueNode>) {
    // Find roots and leaves
    let mut roots = Vec::new();
    let mut leaves = Vec::new();
    for (id, node) in nodes {
        if node.depends_on.is_empty() {
            roots.push(id.clone());
        }
        if node.depended_on_by.is_empty() {
            leaves.push(id.clone());
        }
    }

    // Check for disconnected subgraphs via DFS from roots
    let mut visited = std::collections::HashSet::new();
    fn dfs(
        id: &str,
        nodes: &HashMap<String, IssueNode>,
        visited: &mut std::collections::HashSet<String>,
    ) {
        if !visited.insert(id.to_string()) {
            return;
        }
        if let Some(node) = nodes.get(id) {
            for dep_id in &node.depended_on_by {
                dfs(dep_id, nodes, visited);
            }
        }
    }
    for root in &roots {
        dfs(root, nodes, &mut visited);
    }

    let disconnected: Vec<&String> = nodes.keys().filter(|id| !visited.contains(*id)).collect();
    if !disconnected.is_empty() {
        analysis.warnings.push(format!(
            "Disconnected issues (not reachable from roots): {}",
            disconnected
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    // Detect cycles using DFS
    let mut in_progress: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut completed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut cycle_path = Vec::new();
    let mut has_cycle = false;

    fn detect_cycle(
        id: &str,
        nodes: &HashMap<String, IssueNode>,
        in_progress: &mut std::collections::HashSet<String>,
        completed: &mut std::collections::HashSet<String>,
        cycle_path: &mut Vec<String>,
        has_cycle: &mut bool,
    ) -> bool {
        if completed.contains(id) {
            return false;
        }
        if !in_progress.insert(id.to_string()) {
            *has_cycle = true;
            return true;
        }
        cycle_path.push(id.to_string());

        if let Some(node) = nodes.get(id) {
            for dep_id in &node.depends_on {
                if detect_cycle(dep_id, nodes, in_progress, completed, cycle_path, has_cycle) {
                    return true;
                }
            }
        }

        cycle_path.pop();
        in_progress.remove(id);
        completed.insert(id.to_string());
        false
    }

    for id in nodes.keys() {
        if !completed.contains(id) {
            if detect_cycle(
                id,
                nodes,
                &mut in_progress,
                &mut completed,
                &mut cycle_path,
                &mut has_cycle,
            ) {
                break;
            }
        }
    }

    if has_cycle {
        analysis.errors.push(format!(
            "Dependency cycle detected involving: {:?}",
            cycle_path
        ));
    }
}

fn compute_ready_fronts(analysis: &mut SwarmAnalysis, nodes: &HashMap<String, IssueNode>) {
    if !analysis.errors.is_empty() {
        return;
    }

    // Kahn's algorithm with level tracking
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for (id, node) in nodes {
        in_degree.insert(id.as_str(), node.depends_on.len());
    }

    let mut current_wave: Vec<&str> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(id, _)| *id)
        .collect();

    let mut wave_num = 0;
    while !current_wave.is_empty() {
        current_wave.sort_unstable();

        let mut titles = Vec::new();
        for id in &current_wave {
            if let Some(node) = nodes.get(*id) {
                titles.push(node.title.clone());
            }
        }

        analysis.ready_fronts.push(ReadyFront {
            wave: wave_num,
            issues: current_wave.iter().map(|s| s.to_string()).collect(),
            titles,
        });

        if current_wave.len() > analysis.max_parallelism {
            analysis.max_parallelism = current_wave.len();
        }

        let mut next_wave = Vec::new();
        for id in &current_wave {
            if let Some(node) = nodes.get(*id) {
                for dep_id in &node.depended_on_by {
                    if let Some(deg) = in_degree.get_mut(dep_id.as_str()) {
                        *deg -= 1;
                        if *deg == 0 {
                            next_wave.push(dep_id.as_str());
                        }
                    }
                }
            }
        }

        current_wave = next_wave;
        wave_num += 1;
    }

    analysis.estimated_sessions = analysis.total_issues;
}

fn render_swarm_analysis(analysis: &SwarmAnalysis, ctx: &OutputContext) {
    ctx.print_line(&format!("Swarm Analysis: {}", analysis.epic_title));
    ctx.print_line(&format!("  Epic ID: {}", analysis.epic_id));
    ctx.print_line(&format!(
        "  Total issues: {} ({} closed)",
        analysis.total_issues, analysis.closed_issues
    ));

    if analysis.total_issues == 0 {
        ctx.print_line("Epic has no children to swarm");
        return;
    }

    // Ready fronts
    if !analysis.ready_fronts.is_empty() {
        ctx.print_line("Ready Fronts (waves of parallel work):");
        for front in &analysis.ready_fronts {
            ctx.print_line(&format!(
                "  Wave {}: {} issues",
                front.wave + 1,
                front.issues.len()
            ));
            for (i, id) in front.issues.iter().enumerate() {
                let title = front.titles.get(i).map(|s| s.as_str()).unwrap_or("");
                ctx.print_line(&format!("    - {}: {}", id, title));
            }
        }
    }

    // Summary
    ctx.print_line(&format!(
        "  Estimated worker-sessions: {}",
        analysis.estimated_sessions
    ));
    ctx.print_line(&format!("  Max parallelism: {}", analysis.max_parallelism));
    ctx.print_line(&format!("  Total waves: {}", analysis.ready_fronts.len()));

    // Warnings
    for warning in &analysis.warnings {
        ctx.print_line(&format!("  Warning: {}", warning));
    }

    // Errors
    for err in &analysis.errors {
        ctx.print_line(&format!("  Error: {}", err));
    }

    if analysis.swarmable {
        ctx.success("Swarmable: YES");
    } else {
        ctx.print_line("Swarmable: NO (fix errors first)");
    }
}

// ---------------------------------------------------------------------------
// swarm create command
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SwarmCreateOutput {
    swarm_id: String,
    epic_id: String,
    coordinator: String,
    total_issues: usize,
    max_parallelism: usize,
    waves: usize,
}

fn execute_swarm_create(
    args: &MolSwarmCreateArgs,
    storage: &mut SqliteStorage,
    actor: &str,
    ctx: &OutputContext,
) -> Result<()> {
    let epic_id = resolve_id(storage, &args.epic_id)?;
    let epic = storage
        .get_issue(&epic_id)?
        .ok_or_else(|| BeadsError::IssueNotFound {
            id: epic_id.clone(),
        })?;

    if epic.issue_type != IssueType::Epic && epic.issue_type != IssueType::Molecule {
        return Err(BeadsError::Validation {
            field: "epic_id".to_string(),
            reason: format!(
                "'{}' is not an epic or molecule (type: {})",
                epic_id, epic.issue_type
            ),
        });
    }

    // Check if a swarm already exists for this epic
    let existing_swarm = find_existing_swarm(storage, &epic_id)?;
    if existing_swarm.is_some() && !args.force {
        let existing = existing_swarm.unwrap();
        return Err(BeadsError::Validation {
            field: "epic_id".to_string(),
            reason: format!(
                "swarm already exists for epic '{}': {}. Use --force to create another.",
                epic_id, existing.id
            ),
        });
    }

    // Validate epic structure
    let analysis = analyze_epic_for_swarm(storage, &epic, false)?;
    if !analysis.swarmable {
        let err_msg = analysis.errors.join("; ");
        return Err(BeadsError::Validation {
            field: "epic_id".to_string(),
            reason: format!("epic '{}' is not swarmable: {}", epic_id, err_msg),
        });
    }

    // Create the swarm molecule
    let swarm_title = format!("Swarm: {}", epic.title);
    let coordinator = args.coordinator.clone().unwrap_or_default();
    let swarm_mol = Issue {
        id: String::new(), // Will be auto-generated
        title: swarm_title.clone(),
        description: Some(format!(
            "Swarm molecule orchestrating epic {}.\n\nEpic: {}\nCoordinator: {}",
            epic_id, epic_id, coordinator
        )),
        status: Status::Open,
        priority: epic.priority,
        issue_type: IssueType::Molecule,
        mol_type: MolType::Swarm,
        assignee: if coordinator.is_empty() {
            None
        } else {
            Some(coordinator.clone())
        },
        created_by: Some(actor.to_string()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        ..Issue::default()
    };

    storage.create_issue(&swarm_mol, actor)?;

    // Link via relates-to dependency
    let swarm_id = swarm_mol.id.clone();
    storage.add_dependency(&swarm_id, &epic_id, "relates-to", actor)?;

    let output = SwarmCreateOutput {
        swarm_id: swarm_id.clone(),
        epic_id: epic_id.clone(),
        coordinator,
        total_issues: analysis.total_issues,
        max_parallelism: analysis.max_parallelism,
        waves: analysis.ready_fronts.len(),
    };

    if ctx.is_json() || ctx.is_toon() {
        ctx.json_pretty(&output);
    } else {
        ctx.success(&format!(
            "Created swarm molecule: {} for epic {}",
            swarm_id, epic_id
        ));
        ctx.print_line(&format!("  Total issues: {}", analysis.total_issues));
        ctx.print_line(&format!("  Max parallelism: {}", analysis.max_parallelism));
        ctx.print_line(&format!("  Waves: {}", analysis.ready_fronts.len()));
    }

    Ok(())
}

/// Find an existing swarm molecule linked to an epic via relates-to.
fn find_existing_swarm(storage: &SqliteStorage, epic_id: &str) -> Result<Option<Issue>> {
    let dependents: Vec<String> = storage.get_dependents(epic_id)?;
    for dep_id in &dependents {
        if let Ok(Some(issue)) = storage.get_issue(dep_id) {
            if issue.issue_type == IssueType::Molecule && issue.mol_type == MolType::Swarm {
                // Verify the link is via relates-to
                if let Ok(deps) = storage.get_dependencies_full(dep_id) {
                    for d in deps {
                        if d.depends_on_id == epic_id && d.dep_type == DependencyType::RelatesTo {
                            return Ok(Some(issue));
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}
