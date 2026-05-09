//! Capabilities command implementation.

use crate::cli::{
    CapabilitiesArgs, Cli, OutputFormat, resolve_output_format_basic_with_outer_mode,
};
use crate::error::{BeadsError, Result};
use crate::output::{OutputContext, OutputMode};
use clap::{Arg, Command as ClapCommand, CommandFactory};
use serde::Serialize;

const CONTRACT_VERSION: &str = "br.capabilities.v1";

#[derive(Debug, Serialize)]
struct CapabilitiesOutput {
    tool: &'static str,
    version: &'static str,
    contract_version: &'static str,
    features: &'static [FeatureCapability],
    commands: Vec<CommandCapability>,
    global_flags: &'static [FlagCapability],
    output_formats: &'static [&'static str],
    exit_codes: &'static [ExitCodeCapability],
    env_vars: &'static [EnvVarCapability],
    safety: &'static [SafetyCapability],
    recommended_entrypoints: &'static [&'static str],
    #[serde(skip_serializing_if = "Option::is_none")]
    command_detail: Option<CommandDetail>,
}

#[derive(Debug, Serialize)]
struct FeatureCapability {
    name: &'static str,
    description: &'static str,
}

#[derive(Debug, Serialize)]
struct CommandCapability {
    name: String,
    summary: String,
    operation: &'static str,
    workspace: &'static str,
    machine_output: &'static [&'static str],
    examples: &'static [&'static str],
}

#[derive(Debug, Serialize)]
struct FlagCapability {
    flag: &'static str,
    description: &'static str,
}

#[derive(Debug, Serialize)]
struct ExitCodeCapability {
    code: i32,
    category: &'static str,
    description: &'static str,
}

#[derive(Debug, Serialize)]
struct EnvVarCapability {
    name: &'static str,
    description: &'static str,
}

#[derive(Debug, Serialize)]
struct SafetyCapability {
    name: &'static str,
    guarantee: &'static str,
}

#[derive(Debug, Serialize)]
struct CommandDetail {
    path: String,
    name: String,
    summary: String,
    long_about: Option<String>,
    aliases: Vec<String>,
    operation: &'static str,
    workspace: &'static str,
    machine_output: &'static [&'static str],
    examples: &'static [&'static str],
    arguments: Vec<ArgumentCapability>,
    subcommands: Vec<SubcommandCapability>,
}

#[derive(Debug, Serialize)]
struct ArgumentCapability {
    id: String,
    kind: &'static str,
    long: Option<String>,
    short: Option<String>,
    aliases: Vec<String>,
    help: Option<String>,
    required: bool,
    action: String,
    value_names: Vec<String>,
    default_values: Vec<String>,
    possible_values: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SubcommandCapability {
    name: String,
    summary: String,
    aliases: Vec<String>,
}

#[derive(Clone, Copy)]
struct CommandContract {
    operation: &'static str,
    workspace: &'static str,
    machine_output: &'static [&'static str],
    examples: &'static [&'static str],
}

const FEATURES: &[FeatureCapability] = &[
    FeatureCapability {
        name: "local_first_issue_tracking",
        description: "Stores issue state locally in SQLite with git-friendly JSONL export.",
    },
    FeatureCapability {
        name: "agent_machine_output",
        description: "Read-side commands expose JSON, TOON, or documented robot surfaces.",
    },
    FeatureCapability {
        name: "schema_export",
        description: "br schema emits JSON schemas and command envelope shapes.",
    },
    FeatureCapability {
        name: "coordination_diagnostics",
        description: "br coordination status diagnoses hidden or stale in-progress claims.",
    },
    FeatureCapability {
        name: "mcp_stdio_optional",
        description: "Binaries built with the mcp feature can serve a stdio MCP API.",
    },
];

const GLOBAL_FLAGS: &[FlagCapability] = &[
    FlagCapability {
        flag: "--json",
        description: "Request machine-readable JSON output when a command supports it.",
    },
    FlagCapability {
        flag: "--no-color",
        description: "Disable ANSI styling for human-readable output.",
    },
    FlagCapability {
        flag: "-q, --quiet",
        description: "Suppress non-error output.",
    },
    FlagCapability {
        flag: "--db <PATH>",
        description: "Override the discovered .beads SQLite database path.",
    },
    FlagCapability {
        flag: "--actor <NAME>",
        description: "Set the actor recorded in mutation audit events.",
    },
];

const OUTPUT_FORMATS: &[&str] = &["text", "json", "toon"];

const EXIT_CODES: &[ExitCodeCapability] = &[
    ExitCodeCapability {
        code: 0,
        category: "success",
        description: "Command completed successfully.",
    },
    ExitCodeCapability {
        code: 1,
        category: "internal",
        description: "Unexpected internal error.",
    },
    ExitCodeCapability {
        code: 2,
        category: "database",
        description: "Database initialization, schema, or storage error.",
    },
    ExitCodeCapability {
        code: 3,
        category: "issue",
        description: "Issue lookup, ambiguity, or issue-state error.",
    },
    ExitCodeCapability {
        code: 4,
        category: "validation",
        description: "Invalid command input, flag value, or required field.",
    },
    ExitCodeCapability {
        code: 5,
        category: "dependency",
        description: "Dependency graph error such as a cycle or self-dependency.",
    },
    ExitCodeCapability {
        code: 6,
        category: "sync_jsonl",
        description: "JSONL import/export, merge, or sync safety error.",
    },
    ExitCodeCapability {
        code: 7,
        category: "config",
        description: "Configuration or workspace discovery error.",
    },
    ExitCodeCapability {
        code: 8,
        category: "io",
        description: "Filesystem or terminal I/O error.",
    },
];

const ENV_VARS: &[EnvVarCapability] = &[
    EnvVarCapability {
        name: "BD_DB / BD_DATABASE",
        description: "Override the SQLite database path.",
    },
    EnvVarCapability {
        name: "BEADS_JSONL",
        description: "Override the JSONL path when explicitly allowed.",
    },
    EnvVarCapability {
        name: "BR_OUTPUT_FORMAT",
        description: "Default output format: text, json, or toon.",
    },
    EnvVarCapability {
        name: "TOON_DEFAULT_FORMAT",
        description: "Fallback default that can select TOON output.",
    },
    EnvVarCapability {
        name: "NO_COLOR",
        description: "Disable colored human-readable output.",
    },
    EnvVarCapability {
        name: "RUST_LOG",
        description: "Set logging verbosity; use RUST_LOG=error for routine agent runs.",
    },
];

const SAFETY: &[SafetyCapability] = &[
    SafetyCapability {
        name: "no_automatic_git_operations",
        guarantee: "br never commits, pushes, pulls, or installs hooks automatically.",
    },
    SafetyCapability {
        name: "sync_path_allowlist",
        guarantee: "sync writes stay inside .beads unless an external JSONL path is explicitly allowed.",
    },
    SafetyCapability {
        name: "write_lock_for_storage_mutations",
        guarantee: "mutating commands acquire the workspace write lock before storage writes.",
    },
    SafetyCapability {
        name: "structured_errors",
        guarantee: "machine-output contexts render structured error envelopes on stderr.",
    },
];

const RECOMMENDED_ENTRYPOINTS: &[&str] = &[
    "br capabilities --format json",
    "br robot-docs guide",
    "br ready --json",
    "br coordination status --json",
    "br schema commands --format json",
];

/// Execute the capabilities command.
///
/// # Errors
///
/// Returns an error if output serialization fails.
pub fn execute(args: &CapabilitiesArgs, outer_ctx: &OutputContext) -> Result<()> {
    let output_format = resolve_output_format_basic_with_outer_mode(
        args.format,
        outer_ctx.inherited_output_mode(),
        false,
    );
    let quiet = matches!(outer_ctx.mode(), OutputMode::Quiet);
    let ctx = OutputContext::from_output_format(output_format, quiet, true);
    if ctx.is_quiet() {
        return Ok(());
    }

    let command_detail = args
        .command
        .as_deref()
        .map(command_detail_for_path)
        .transpose()?;

    let payload = CapabilitiesOutput {
        tool: "br",
        version: env!("CARGO_PKG_VERSION"),
        contract_version: CONTRACT_VERSION,
        features: FEATURES,
        commands: command_capabilities(),
        global_flags: GLOBAL_FLAGS,
        output_formats: OUTPUT_FORMATS,
        exit_codes: EXIT_CODES,
        env_vars: ENV_VARS,
        safety: SAFETY,
        recommended_entrypoints: RECOMMENDED_ENTRYPOINTS,
        command_detail,
    };

    match output_format {
        OutputFormat::Json => ctx.json_pretty(&payload),
        OutputFormat::Toon => ctx.toon_with_stats(&payload, args.stats),
        OutputFormat::Text | OutputFormat::Csv => render_text(&payload, args.command.as_deref()),
    }

    Ok(())
}

fn command_capabilities() -> Vec<CommandCapability> {
    Cli::command()
        .get_subcommands()
        .filter(|command| command.get_name() != "help")
        .map(|command| {
            let contract = command_contract(command.get_name());
            CommandCapability {
                name: command.get_name().to_string(),
                summary: command
                    .get_about()
                    .map(std::string::ToString::to_string)
                    .unwrap_or_default(),
                operation: contract.operation,
                workspace: contract.workspace,
                machine_output: contract.machine_output,
                examples: contract.examples,
            }
        })
        .collect()
}

fn command_detail_for_path(path: &str) -> Result<CommandDetail> {
    let segments = command_path_segments(path)?;
    let root = Cli::command();
    let (canonical_path, command) = find_command_path(&root, &segments).ok_or_else(|| {
        BeadsError::validation(
            "command",
            format!(
                "unknown command path '{path}'. Try one of: {}",
                root.get_subcommands()
                    .map(ClapCommand::get_name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
    })?;

    Ok(command_detail(command, &canonical_path))
}

fn command_path_segments(path: &str) -> Result<Vec<&str>> {
    let mut segments = path.split_whitespace().collect::<Vec<_>>();
    if segments.first().is_some_and(|segment| *segment == "br") {
        segments.remove(0);
    }
    if segments.is_empty() {
        return Err(BeadsError::validation(
            "command",
            "command path cannot be empty",
        ));
    }
    Ok(segments)
}

fn find_command_path<'a>(
    root: &'a ClapCommand,
    segments: &[&str],
) -> Option<(Vec<String>, &'a ClapCommand)> {
    let mut current = root;
    let mut canonical_path = Vec::with_capacity(segments.len());

    for segment in segments {
        let next = current
            .get_subcommands()
            .find(|command| command_matches_path_segment(command, segment))?;
        canonical_path.push(next.get_name().to_string());
        current = next;
    }

    Some((canonical_path, current))
}

fn command_matches_path_segment(command: &ClapCommand, segment: &str) -> bool {
    command.get_name() == segment || command.get_all_aliases().any(|alias| alias == segment)
}

fn command_detail(command: &ClapCommand, canonical_path: &[String]) -> CommandDetail {
    let contract = command_contract(
        canonical_path
            .first()
            .map_or_else(|| command.get_name(), String::as_str),
    );
    CommandDetail {
        path: canonical_path.join(" "),
        name: command.get_name().to_string(),
        summary: command
            .get_about()
            .map(std::string::ToString::to_string)
            .unwrap_or_default(),
        long_about: command
            .get_long_about()
            .map(std::string::ToString::to_string),
        aliases: command.get_visible_aliases().map(str::to_string).collect(),
        operation: contract.operation,
        workspace: contract.workspace,
        machine_output: contract.machine_output,
        examples: contract.examples,
        arguments: command
            .get_arguments()
            .filter(|argument| !argument.is_hide_set())
            .map(argument_capability)
            .collect(),
        subcommands: command
            .get_subcommands()
            .filter(|subcommand| subcommand.get_name() != "help")
            .map(subcommand_capability)
            .collect(),
    }
}

fn argument_capability(argument: &Arg) -> ArgumentCapability {
    let long = argument.get_long().map(|long| format!("--{long}"));
    let short = argument.get_short().map(|short| format!("-{short}"));
    let kind = if long.is_some() || short.is_some() {
        "option"
    } else {
        "positional"
    };

    ArgumentCapability {
        id: argument.get_id().to_string(),
        kind,
        long,
        short,
        aliases: argument
            .get_visible_aliases()
            .unwrap_or_default()
            .into_iter()
            .map(str::to_string)
            .collect(),
        help: argument.get_help().map(std::string::ToString::to_string),
        required: argument.is_required_set(),
        action: format!("{:?}", argument.get_action()),
        value_names: argument
            .get_value_names()
            .unwrap_or_default()
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
        default_values: argument
            .get_default_values()
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect(),
        possible_values: argument
            .get_possible_values()
            .into_iter()
            .map(|value| value.get_name().to_string())
            .collect(),
    }
}

fn subcommand_capability(command: &ClapCommand) -> SubcommandCapability {
    SubcommandCapability {
        name: command.get_name().to_string(),
        summary: command
            .get_about()
            .map(std::string::ToString::to_string)
            .unwrap_or_default(),
        aliases: command.get_visible_aliases().map(str::to_string).collect(),
    }
}

#[allow(clippy::too_many_lines)]
fn command_contract(name: &str) -> CommandContract {
    match name {
        "capabilities" => CommandContract {
            operation: "read",
            workspace: "none",
            machine_output: &["json", "toon", "text"],
            examples: &["br capabilities --format json"],
        },
        "robot-docs" => CommandContract {
            operation: "read",
            workspace: "none",
            machine_output: &["json", "toon", "text"],
            examples: &["br robot-docs guide"],
        },
        "schema" => CommandContract {
            operation: "read",
            workspace: "none",
            machine_output: &["json", "toon", "text"],
            examples: &["br schema commands --format json"],
        },
        "version" => CommandContract {
            operation: "read",
            workspace: "none",
            machine_output: &["json", "toon", "text"],
            examples: &["br version --json"],
        },
        "completions" => CommandContract {
            operation: "read",
            workspace: "none",
            machine_output: &["text"],
            examples: &["br completions zsh"],
        },
        "init" => CommandContract {
            operation: "write",
            workspace: "none",
            machine_output: &["text"],
            examples: &["br init --prefix br"],
        },
        "create" => CommandContract {
            operation: "write",
            workspace: "required",
            machine_output: &["json", "text"],
            examples: &["br create \"Fix login\" --type bug --priority 1 --json"],
        },
        "q" => CommandContract {
            operation: "write",
            workspace: "required",
            machine_output: &["text"],
            examples: &["br q \"Quick note\""],
        },
        "update" => CommandContract {
            operation: "write",
            workspace: "required",
            machine_output: &["json", "text"],
            examples: &["br update br-abc --claim --json"],
        },
        "close" => CommandContract {
            operation: "write",
            workspace: "required",
            machine_output: &["json", "text"],
            examples: &["br close br-abc --reason \"Done\" --json"],
        },
        "reopen" => CommandContract {
            operation: "write",
            workspace: "required",
            machine_output: &["json", "text"],
            examples: &["br reopen br-abc --json"],
        },
        "delete" => CommandContract {
            operation: "write",
            workspace: "required",
            machine_output: &["json", "text"],
            examples: &["br delete br-abc --reason duplicate --json"],
        },
        "defer" => CommandContract {
            operation: "write",
            workspace: "required",
            machine_output: &["json", "text"],
            examples: &["br defer br-abc --until tomorrow --json"],
        },
        "undefer" => CommandContract {
            operation: "write",
            workspace: "required",
            machine_output: &["json", "text"],
            examples: &["br undefer br-abc --json"],
        },
        "list" => CommandContract {
            operation: "read",
            workspace: "required",
            machine_output: &["json", "toon", "csv", "text"],
            examples: &["br list --status open --format json"],
        },
        "ready" => CommandContract {
            operation: "read",
            workspace: "required",
            machine_output: &["json", "toon", "text"],
            examples: &["br ready --json"],
        },
        "scheduler" => CommandContract {
            operation: "read",
            workspace: "required",
            machine_output: &["json", "toon", "text"],
            examples: &["br scheduler --json"],
        },
        "coordination" => CommandContract {
            operation: "read",
            workspace: "required",
            machine_output: &["json", "toon", "text"],
            examples: &["br coordination status --json"],
        },
        "blocked" => CommandContract {
            operation: "read",
            workspace: "required",
            machine_output: &["json", "toon", "text"],
            examples: &["br blocked --json"],
        },
        "show" => CommandContract {
            operation: "read",
            workspace: "required",
            machine_output: &["json", "toon", "text"],
            examples: &["br show br-abc --json"],
        },
        "search" => CommandContract {
            operation: "read",
            workspace: "required",
            machine_output: &["json", "toon", "csv", "text"],
            examples: &["br search \"auth\" --format json"],
        },
        "count" => CommandContract {
            operation: "read",
            workspace: "required",
            machine_output: &["json", "text"],
            examples: &["br count --by status --json"],
        },
        "stale" => CommandContract {
            operation: "read",
            workspace: "required",
            machine_output: &["json", "toon", "text"],
            examples: &["br stale --days 30 --json"],
        },
        "stats" | "status" => CommandContract {
            operation: "read",
            workspace: "required",
            machine_output: &["json", "toon", "text"],
            examples: &["br stats --format json"],
        },
        "where" | "info" => CommandContract {
            operation: "read",
            workspace: "optional",
            machine_output: &["json", "toon", "text"],
            examples: &["br where --json"],
        },
        "sync" => CommandContract {
            operation: "mixed",
            workspace: "required",
            machine_output: &["json", "text"],
            examples: &["br sync --status --json", "br sync --flush-only"],
        },
        "doctor" => CommandContract {
            operation: "mixed",
            workspace: "optional",
            machine_output: &["json", "text"],
            examples: &["br doctor --json"],
        },
        "config" | "history" | "query" | "dep" | "label" | "comments" | "epic" => CommandContract {
            operation: "mixed",
            workspace: "required",
            machine_output: &["json", "toon", "text"],
            examples: &[],
        },
        "graph" | "orphans" | "changelog" | "lint" | "audit" => CommandContract {
            operation: "mixed",
            workspace: "required",
            machine_output: &["json", "text"],
            examples: &[],
        },
        "agents" => CommandContract {
            operation: "mixed",
            workspace: "optional",
            machine_output: &["json", "text"],
            examples: &["br agents --check --json"],
        },
        "upgrade" => CommandContract {
            operation: "write",
            workspace: "none",
            machine_output: &["text"],
            examples: &["br upgrade --check"],
        },
        _ => CommandContract {
            operation: "unknown",
            workspace: "unknown",
            machine_output: &["text"],
            examples: &[],
        },
    }
}

fn render_text(output: &CapabilitiesOutput, requested_command: Option<&str>) {
    println!(
        "{} {} ({})",
        output.tool, output.version, output.contract_version
    );
    println!();
    println!("Recommended agent entrypoints:");
    for command in output.recommended_entrypoints {
        println!("  {command}");
    }
    println!();
    println!("Commands:");
    for command in &output.commands {
        println!(
            "  {:<16} {:<5} {:<8} {}",
            command.name, command.operation, command.workspace, command.summary
        );
    }
    if let Some(detail) = output.command_detail.as_ref() {
        println!();
        println!(
            "Command detail: {}",
            requested_command.unwrap_or(detail.path.as_str())
        );
        println!(
            "  canonical: {}  operation: {}  workspace: {}",
            detail.path, detail.operation, detail.workspace
        );
        if !detail.arguments.is_empty() {
            println!("  arguments:");
            for argument in &detail.arguments {
                let spellings = [argument.short.as_deref(), argument.long.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(", ");
                let name = if spellings.is_empty() {
                    argument.id.as_str()
                } else {
                    spellings.as_str()
                };
                println!(
                    "    {:<22} {:<10} required={} {}",
                    name,
                    argument.kind,
                    argument.required,
                    argument.help.as_deref().unwrap_or_default()
                );
            }
        }
        if !detail.subcommands.is_empty() {
            println!("  subcommands:");
            for subcommand in &detail.subcommands {
                println!("    {:<22} {}", subcommand.name, subcommand.summary);
            }
        }
    }
    println!();
    println!("Safety:");
    for item in output.safety {
        println!("  {}: {}", item.name, item.guarantee);
    }
}
