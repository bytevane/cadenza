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
            run_pr_gate(&base)?;
        }
    }

    Ok(())
}

fn run_pr_gate(base: &str) -> anyhow::Result<()> {
    use std::process::Command as Proc;

    // PR body from the GitHub event payload. Fail-closed if unreadable.
    let event_path = std::env::var("GITHUB_EVENT_PATH")
        .context("GITHUB_EVENT_PATH not set (gate must run in GitHub Actions)")?;
    let event: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&event_path).with_context(|| format!("read {event_path}"))?,
    )?;
    let pr_body = event
        .pointer("/pull_request/body")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Changed files vs base. Fail-closed if the diff can't be produced.
    let out = Proc::new("git")
        .args(["diff", "--name-only", &format!("{base}...HEAD")])
        .output()
        .context("running git diff")?;
    if !out.status.success() {
        anyhow::bail!(
            "git diff against {base} failed (need fetch-depth: 0). Refusing to pass (fail-closed)."
        );
    }
    let names: Vec<String> = String::from_utf8(out.stdout)?
        .lines()
        .map(|s| s.to_string())
        .collect();

    // For versions.toml, compute which MVP-critical key values changed.
    let changed: Vec<pr_gate::ChangedFile> = names
        .iter()
        .map(|p| {
            let changed_version_keys = if p == "tools/versions.toml" {
                let base_toml = Proc::new("git")
                    .args(["show", &format!("{base}:tools/versions.toml")])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                    .unwrap_or_default();
                let head_toml = fs::read_to_string("tools/versions.toml").unwrap_or_default();
                pr_gate::changed_version_keys(&base_toml, &head_toml)
            } else {
                Vec::new()
            };
            pr_gate::ChangedFile {
                path: p.clone(),
                changed_version_keys,
            }
        })
        .collect();

    let result = pr_gate::evaluate(&changed, pr_body);
    if result.passed() {
        println!("pr-gate: ok ({} files checked)", changed.len());
        Ok(())
    } else {
        for v in &result.violations {
            eprintln!("pr-gate: {v}");
        }
        std::process::exit(1);
    }
}
