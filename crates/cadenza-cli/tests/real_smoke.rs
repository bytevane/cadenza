//! Real integration smoke (#23). All tests here are `#[ignore]`d so
//! `cargo test --workspace` never requires credentials; opt in via
//! `cargo test --test real_smoke -- --ignored` or
//! `./scripts/real-smoke.sh`.
//!
//! Each test reads the credentials it needs from env vars and
//! **skips** (returns early after logging a clear reason) when any
//! required env is unset. That keeps the same binary safe to run in
//! environments without credentials.
//!
//! Logging is gated through `cadenza_obs::Scrubber` so a registered
//! token cannot appear in any captured output even on failure.

use std::time::Duration;

use cadenza_codex::{AppServerLauncher, ClientInfo};
use cadenza_obs::Scrubber;
use cadenza_tracker_linear::{
    IssueTrackerClient, LinearClient, LinearClientConfig, transport::HttpLinearTransport,
};

const SKIP_ENV_CODEX: &str = "CADENZA_REAL_SMOKE_CODEX";
const SKIP_ENV_LINEAR_TOKEN: &str = "CADENZA_LINEAR_TOKEN";
const SKIP_ENV_LINEAR_PROJECT: &str = "CADENZA_LINEAR_PROJECT_SLUG_ID";

fn skip_if_unset(var: &str) -> Option<String> {
    match std::env::var(var) {
        Ok(v) if !v.is_empty() => Some(v),
        _ => {
            eprintln!(
                "[real-smoke] SKIP: env var `{var}` is unset. \
                 Set it to opt into the real integration profile."
            );
            None
        }
    }
}

#[tokio::test]
#[ignore = "opt-in real integration profile; see docs/operations/real-smoke.md"]
async fn real_codex_app_server_handshake() {
    let Some(_) = skip_if_unset(SKIP_ENV_CODEX) else {
        return;
    };
    let workspace = tempfile::tempdir().expect("tempdir");
    // The CLI command is operator-configurable. Default matches the
    // shipped `WORKFLOW.example.md`.
    let command = std::env::var("CADENZA_CODEX_COMMAND")
        .unwrap_or_else(|_| "codex app-server --listen stdio://".to_string());
    let scrubber = Scrubber::with_secrets(
        std::env::var("CADENZA_REAL_SMOKE_SECRETS")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    );

    let client = AppServerLauncher::new(command, workspace.path().to_path_buf())
        .with_startup_timeout(Duration::from_secs(30))
        .with_client_info(ClientInfo {
            name: "cadenza-real-smoke".into(),
            title: None,
            version: env!("CARGO_PKG_VERSION").into(),
        })
        .with_secrets(
            std::env::var("CADENZA_REAL_SMOKE_SECRETS")
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        )
        .launch()
        .await;
    match client {
        Ok(client) => {
            let info = client.initialize_response().clone();
            // Print through the scrubber so anything sensitive in
            // userAgent / codexHome cannot leak even via test output.
            let line = format!(
                "[real-smoke] codex handshake OK: ua={}, home={}, family={}, os={}",
                info.user_agent, info.codex_home, info.platform_family, info.platform_os,
            );
            eprintln!("{}", scrubber.scrub_text(&line));
            client.shutdown().await.expect("shutdown");
        }
        Err(err) => {
            let line = format!("[real-smoke] codex handshake failed: {err}");
            eprintln!("{}", scrubber.scrub_text(&line));
            panic!("real codex handshake failed (see scrubbed output above)");
        }
    }
}

#[tokio::test]
#[ignore = "opt-in real integration profile; see docs/operations/real-smoke.md"]
async fn real_linear_read_returns_issue_set() {
    let Some(token) = skip_if_unset(SKIP_ENV_LINEAR_TOKEN) else {
        return;
    };
    let Some(project) = skip_if_unset(SKIP_ENV_LINEAR_PROJECT) else {
        return;
    };
    let scrubber = Scrubber::with_secrets(vec![token.clone()]);

    let transport = HttpLinearTransport::new("https://api.linear.app/graphql", token.clone())
        .expect("http transport");
    let config = LinearClientConfig {
        project_slug_id: Some(project),
        page_size: 5,
        ..Default::default()
    };
    let client = LinearClient::new(config, transport);
    match client.fetch_candidate_issues().await {
        Ok(issues) => {
            let line = format!(
                "[real-smoke] linear read OK: {} issue(s); first identifier={}",
                issues.len(),
                issues
                    .first()
                    .map(|i| i.identifier.as_str())
                    .unwrap_or("<none>"),
            );
            eprintln!("{}", scrubber.scrub_text(&line));
        }
        Err(err) => {
            let line = format!("[real-smoke] linear read failed: {err}");
            eprintln!("{}", scrubber.scrub_text(&line));
            panic!("real linear read failed (see scrubbed output above)");
        }
    }
}
