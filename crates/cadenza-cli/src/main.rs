mod pr_gate;

use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "cadenza")]
#[command(about = "Cadenza orchestration runtime CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate local development prerequisites and a WORKFLOW.md file.
    Doctor {
        #[arg(long, default_value = "WORKFLOW.md")]
        workflow: PathBuf,
    },
    /// Print the sanitized workspace key for an issue identifier.
    WorkspaceKey { identifier: String },
    /// Print the full workspace path for an issue identifier.
    WorkspacePath {
        #[arg(long)]
        root: Utf8PathBuf,
        identifier: String,
    },
    /// Run the author-time PR gate (reads the GitHub event + git diff, exits 1 on violation).
    PrGate {
        /// Base ref to diff against (default: origin/main).
        #[arg(long, default_value = "origin/main")]
        base: String,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Doctor { workflow } => {
            let source = fs::read_to_string(&workflow)
                .with_context(|| format!("failed to read {}", workflow.display()))?;
            let parsed = cadenza_workflow::parse_workflow(&source)?;
            println!("workflow: ok");
            println!("prompt bytes: {}", parsed.prompt_template.len());
            println!("config: {}", serde_yaml::to_string(&parsed.config)?.trim());
        }
        Command::WorkspaceKey { identifier } => {
            println!("{}", cadenza_core::workspace_key(&identifier));
        }
        Command::WorkspacePath { root, identifier } => {
            let path = cadenza_workspace::workspace_path(root, &identifier)?;
            println!("{path}");
        }
        Command::PrGate { base } => {
            pr_gate::run(&base)?;
        }
    }

    Ok(())
}
