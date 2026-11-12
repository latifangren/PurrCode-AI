// SPDX-License-Identifier: GPL-3.0-or-later

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "purrcode-ai",
    author = "latifangren",
    version = "0.1.0",
    about = "🐾 PurrCode AI — Lightweight, modular AI agent with zero hardcode"
)]
pub struct Cli {
    /// One-shot prompt execution (non-interactive mode)
    #[arg(short = 'p', long)]
    pub prompt: Option<String>,

    /// Emit NDJSON streaming events to stdout
    #[arg(long)]
    pub json: bool,

    /// Run with Meowstro orchestrator + The Clowder subagents (default; use
    /// --direct for a single general agent with all tools instead)
    #[arg(long)]
    pub orchestrator: bool,

    /// Run as a single general agent with all tools (no Meowstro orchestrator).
    /// Default is orchestrator mode (Meowstro read-only + delegation); --direct
    /// is the opt-out for quick single-agent runs. --orchestrator wins if both
    /// flags are given.
    #[arg(long)]
    pub direct: bool,

    /// Automatically approve all tool actions without prompting (YOLO mode)
    #[arg(long)]
    pub yolo: bool,

    /// Override mode preset (carrier, tabby, oyen)
    #[arg(long)]
    pub preset: Option<String>,

    /// Override subagent model preset (free, local, premium, custom-endpoint)
    #[arg(long)]
    pub model_preset: Option<String>,

    /// Override model to use (format: "provider:model" or "provider/model")
    #[arg(short = 'm', long)]
    pub model: Option<String>,

    /// Explicit workspace root directory (defaults to current working directory)
    #[arg(short = 'w', long)]
    pub workspace: Option<PathBuf>,

    /// Path to custom configuration file
    #[arg(short = 'c', long)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Whether the Meowstro orchestrator runs (PRD §4.5.6). Orchestrator is the
/// default; `--direct` opts out to a single general agent; explicit
/// `--orchestrator` wins if both flags are given.
pub fn use_orchestrator(orchestrator: bool, direct: bool) -> bool {
    orchestrator || !direct
}

/// The audit label for the agent that actually ran. The generic single
/// agent is not Meowstro and must not be recorded as one.
pub fn agent_role_for(use_orchestrator: bool) -> &'static str {
    if use_orchestrator {
        "meowstro"
    } else {
        "agent"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_orchestrator_defaults_to_orchestrator() {
        assert!(use_orchestrator(false, false));
        assert!(use_orchestrator(true, false));
        assert!(use_orchestrator(true, true));
        assert!(!use_orchestrator(false, true));
    }

    #[test]
    fn test_agent_role_for_is_honest() {
        assert_eq!(agent_role_for(true), "meowstro");
        assert_eq!(agent_role_for(false), "agent");
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run setup wizard to configure providers and models
    Onboard,
    /// Diagnose PurrCode runtime, providers, and environment
    Doctor,
    /// List or inspect mode presets (carrier, tabby, oyen)
    Preset {
        #[arg(default_value = "list")]
        action: String,
        #[arg()]
        name: Option<String>,
    },
    /// List or inspect model presets (free, local, premium, custom-endpoint)
    ModelPreset {
        #[arg(default_value = "list")]
        action: String,
        #[arg()]
        name: Option<String>,
    },
    /// Manage sessions (list, export, fork)
    Session {
        #[arg(default_value = "list")]
        action: String,
        #[arg()]
        id: Option<String>,
        #[arg(long, default_value = "jsonl")]
        format: String,
    },
    /// List or inspect installed plugins
    Plugin {
        #[arg(default_value = "list")]
        action: String,
    },
    /// Generate or check systemd/OpenRC daemon service units
    Service {
        #[arg(default_value = "status")]
        action: String,
    },
    /// Headless execution reading prompt from stdin or arguments for CI/scripts
    Exec {
        #[arg()]
        prompt: Option<String>,
    },
    /// Run background gateway daemon for channel adapters (systemd/OpenRC service target)
    Gateway,
    /// Manage scheduled cron jobs (v0.3 F19): add/list/remove/enable/disable + delivery queue
    Cron {
        #[arg(default_value = "list")]
        action: String,
        #[arg()]
        name: Option<String>,
        #[arg(long)]
        schedule: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        allow: Option<String>,
        #[arg(long)]
        preset: Option<String>,
        #[arg(long)]
        yes: bool,
    },
}
