//! AI tool recipe management.
//!
//! Recipes define where beads workflow instructions should be written
//! for different AI coding tools (Claude Code, Cursor, Copilot, etc.).
//!
//! Each recipe specifies:
//! - The target file path (e.g., `.cursor/rules/beads.md`)
//! - The type of installation (write a file, add a hooks config entry, inject a section)
//! - The content template to write

use crate::error::Result;
use std::path::{Path, PathBuf};

/// How a recipe is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeType {
    /// Write a single file to the target path.
    File,
    /// Modify JSON settings to add hooks (Claude Code, Gemini CLI).
    Hooks,
    /// Inject a marked section into an existing file (AGENTS.md sections).
    Section,
    /// Write multiple files.
    MultiFile,
}

impl RecipeType {
    /// Parse a recipe type from a string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "file" => Some(Self::File),
            "hooks" => Some(Self::Hooks),
            "section" => Some(Self::Section),
            "multifile" => Some(Self::MultiFile),
            _ => None,
        }
    }

    /// Return the string representation.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Hooks => "hooks",
            Self::Section => "section",
            Self::MultiFile => "multifile",
        }
    }
}

/// A recipe for installing beads workflow instructions into an AI tool.
#[derive(Debug, Clone)]
pub struct Recipe {
    /// Display name (e.g., "Cursor IDE").
    pub name: &'static str,
    /// Short description.
    pub description: &'static str,
    /// How the recipe is installed.
    pub recipe_type: RecipeType,
    /// Primary file path (for File type).
    pub path: &'static str,
    /// Optional paths for MultiFile type.
    pub paths: &'static [&'static str],
    /// Optional content map for MultiFile type (indexed by path).
    pub contents: &'static [(&'static str, &'static str)],
}

/// Built-in recipe definitions.
const BUILTIN_RECIPES: &[Recipe] = &[
    Recipe {
        name: "Cursor IDE",
        description: "Cursor IDE rules file",
        recipe_type: RecipeType::File,
        path: ".cursor/rules/beads.mdc",
        paths: &[],
        contents: &[],
    },
    Recipe {
        name: "Windsurf",
        description: "Windsurf editor rules file",
        recipe_type: RecipeType::File,
        path: ".windsurf/rules/beads.md",
        paths: &[],
        contents: &[],
    },
    Recipe {
        name: "Sourcegraph Cody",
        description: "Cody AI rules file",
        recipe_type: RecipeType::File,
        path: ".cody/rules/beads.md",
        paths: &[],
        contents: &[],
    },
    Recipe {
        name: "Kilo Code",
        description: "Kilo Code rules file",
        recipe_type: RecipeType::File,
        path: ".kilocode/rules/beads.md",
        paths: &[],
        contents: &[],
    },
    Recipe {
        name: "Claude Code",
        description: "Claude Code hooks (SessionStart)",
        recipe_type: RecipeType::Hooks,
        path: "",
        paths: &[".claude/settings.local.json", "~/.claude/settings.json"],
        contents: &[],
    },
    Recipe {
        name: "Gemini CLI",
        description: "Gemini CLI hooks (SessionStart)",
        recipe_type: RecipeType::Hooks,
        path: "",
        paths: &[".gemini/settings.json", "~/.gemini/settings.json"],
        contents: &[],
    },
    Recipe {
        name: "GitHub Copilot CLI",
        description: "Copilot CLI plugin manifest + instructions",
        recipe_type: RecipeType::MultiFile,
        path: "",
        paths: &[".copilot-plugin/plugin.json", ".github/copilot-instructions.md"],
        contents: &[
            (".copilot-plugin/plugin.json", r#"{
  "name": "beads",
  "description": "Beads issue tracking integration",
  "version": "1.0.0"
}
"#),
            (".github/copilot-instructions.md", r#"# GitHub Copilot Instructions

This repository uses **Beads (br)** for issue tracking.

## Core Workflow

- Use `br ready` to find unblocked work
- Use `br create` to track new work
- Use `br update <id> --claim` before starting
- Use `br close <id>` when work is complete
- Run `br sync --flush-only` before session end
- Do not commit, push, or run git operations on .beads/ unless explicitly authorized
"#),
        ],
    },
    Recipe {
        name: "Factory",
        description: "Factory.ai (Droid) AGENTS.md section",
        recipe_type: RecipeType::Section,
        path: "AGENTS.md",
        paths: &[],
        contents: &[],
    },
    Recipe {
        name: "OpenCode",
        description: "OpenCode AGENTS.md section",
        recipe_type: RecipeType::Section,
        path: "AGENTS.md",
        paths: &[],
        contents: &[],
    },
    Recipe {
        name: "Codex CLI",
        description: "Codex CLI skill guidance",
        recipe_type: RecipeType::Section,
        path: "AGENTS.md",
        paths: &[],
        contents: &[],
    },
    Recipe {
        name: "Mux",
        description: "Mux AGENTS.md section",
        recipe_type: RecipeType::Section,
        path: "AGENTS.md",
        paths: &[],
        contents: &[],
    },
    Recipe {
        name: "Aider",
        description: "Aider config and instruction files",
        recipe_type: RecipeType::MultiFile,
        path: "",
        paths: &[".aider.conf.yml", ".aider/BEADS.md", ".aider/README.md"],
        contents: &[
            (".aider.conf.yml", "# Aider Configuration\n# https://aider.chat/docs/config.html\n\n# Beads workflow instructions in .aider/BEADS.md\nread: [.aider/BEADS.md]\n"),
            (".aider/BEADS.md", "# Beads Issue Tracking\n\nThis project uses Beads (br) for issue tracking.\n\n## Workflow\n\n1. `br ready` - Find ready work\n2. `br update <id> --claim` - Claim an issue\n3. Do the work\n4. `br close <id>` - Mark complete\n5. `br sync --flush-only` - Export to JSONL\n\n## Quick Reference\n\n```bash\nbr ready                              # Show issues ready to work\nbr list --status=open                 # List all open issues\nbr create --title=\"...\" --type=task   # Create new issue\nbr show <id>                          # Issue details\nbr close <id>                         # Mark complete\n```\n"),
            (".aider/README.md", "# Aider + Beads\n\nThis directory contains Aider configuration and instructions for working with Beads issues.\n\nSee `BEADS.md` for workflow instructions.\n"),
        ],
    },
    Recipe {
        name: "Junie",
        description: "Junie guidelines and MCP configuration",
        recipe_type: RecipeType::MultiFile,
        path: "",
        paths: &[".junie/guidelines.md", ".junie/mcp/mcp.json"],
        contents: &[
            (".junie/guidelines.md", "# Junie Guidelines\n\nThis project uses Beads (br) for issue tracking.\n\n- Run `br ready` at session start\n- Always run `br sync --flush-only` before ending session\n"),
            (".junie/mcp/mcp.json", r#"{
  "mcpServers": {
    "beads": {
      "command": "br",
      "args": ["serve"]
    }
  }
}
"#),
        ],
    },
];

/// Find a recipe by display name (case-insensitive).
///
/// Supports matching by the first word of the name (e.g., "cursor" matches
/// "Cursor IDE") and common aliases.
#[must_use]
pub fn find_recipe(name: &str) -> Option<&'static Recipe> {
    let lower = name.to_ascii_lowercase();
    BUILTIN_RECIPES.iter().find(|r| {
        let r_lower = r.name.to_ascii_lowercase();
        let first_word = r_lower.split(' ').next().unwrap_or("");
        r_lower == lower
            || r_lower.replace(' ', "") == lower
            || first_word == lower
            || matches!(
                (r_lower.as_str(), lower.as_str()),
                ("sourcegraph cody", "cody")
                    | ("sourcegraph cody", "sourcegraph")
                    | ("github copilot cli", "copilot")
                    | ("github copilot cli", "copilotcli")
            )
    })
}

/// List all recipe names and descriptions.
#[must_use]
pub fn list_recipes() -> Vec<(&'static str, &'static str, RecipeType)> {
    let mut result: Vec<(&'static str, &'static str, RecipeType)> = BUILTIN_RECIPES
        .iter()
        .map(|r| (r.name, r.description, r.recipe_type))
        .collect();
    result.sort_by_key(|(name, _, _)| *name);
    result
}

/// Install a recipe by writing its files to the specified project directory.
///
/// # Errors
///
/// Returns an error if file operations fail.
pub fn install_recipe(recipe: &Recipe, project_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();

    match recipe.recipe_type {
        RecipeType::File => {
            let target = project_dir.join(recipe.path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, get_recipe_content(recipe, recipe.path))?;
            written.push(target);
        }
        RecipeType::MultiFile => {
            for p in recipe.paths {
                let target = project_dir.join(p);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&target, get_recipe_content(recipe, p))?;
                written.push(target);
            }
        }
        RecipeType::Section | RecipeType::Hooks => {
            // Section and hooks types require AGENTS.md or settings modification
            // which is handled by the `agents` command
            let target = project_dir.join(recipe.path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, get_recipe_content(recipe, recipe.path))?;
            written.push(target);
        }
    }

    Ok(written)
}

/// Get the content to write for a recipe at a given path.
fn get_recipe_content(recipe: &Recipe, path: &str) -> &'static str {
    // Check for specific content map entries
    for (p, content) in recipe.contents {
        if *p == path {
            return content;
        }
    }
    // Default: use the AGENT_BLURB from the agents module
    // For section type, we write the full blurb
    crate::cli::commands::agents::AGENT_BLURB
}
