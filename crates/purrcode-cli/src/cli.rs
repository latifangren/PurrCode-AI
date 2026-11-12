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

    /// Run with Meowstro orchestrator + The Clowder subagents
    #[arg(long)]
    pub orchestrator: bool,

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
    /// Manage scheduled autonomous cron jobs (list, add, remove)
    Cron {
        #[arg(default_value = "list")]
        action: String,
        #[arg()]
        id_or_schedule: Option<String>,
        #[arg()]
        prompt: Option<String>,
        /// Comma-separated list of allowed tools (fail-closed default is read-only)
        #[arg(long)]
        tools: Option<String>,
    },
    /// Inspect and evaluate skills with the self-improving Curator engine (audit, candidates, approve, reject)
    Curator {
        #[arg(default_value = "audit")]
        action: String,
        #[arg()]
        target: Option<String>,
    },
    /// Run PurrCode local WebUI channel and REST/SSE server (Milestone v0.4)
    Web {
        /// HTTP host to bind
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// HTTP port to bind
        #[arg(long, default_value = "8080")]
        port: u16,
        /// Optional authentication token for remote access
        #[arg(long)]
        token: Option<String>,
    },
    /// Run background gateway daemon for channel adapters (systemd/OpenRC service target)
    Gateway,
}
