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

    /// Run with Meowstro orchestrator + Tinker subagents
    #[arg(long)]
    pub orchestrator: bool,

    /// Override model to use (e.g. "magpie/antigravity/gemini-3.8-flash" or "ollama:qwen2.5-coder:7b")
    #[arg(short = 'm', long)]
    pub model: Option<String>,

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
    },
    /// List or inspect model presets (free, local, premium, custom)
    ModelPreset {
        #[arg(default_value = "list")]
        action: String,
    },
}
