//! Acceptance tests for Issue #12 — Codex app-server stdio launcher + handshake.
//!
//! The "Codex" process is mocked via `bash -lc` running an inline script
//! that reads one JSON-RPC line from stdin and writes a fake initialize
//! response (or no response, for the timeout / early-exit cases) to
//! stdout. This keeps the suite hermetic — no `codex` binary required.

use std::time::{Duration, Instant};

use cadenza_codex::{AppServerLauncher, ClientInfo, LaunchError};

const MOCK_OK: &str = r#"
read -r REQ
printf '{"jsonrpc":"2.0","id":1,"result":{"userAgent":"mock-codex/0.0","codexHome":"/tmp/codex-home","platformFamily":"unix","platformOs":"macos"}}\n'
"#;

const MOCK_ERROR: &str = r#"
read -r REQ
printf '{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"method not found"}}\n'
"#;

const MOCK_STALL: &str = r#"
read -r REQ
echo 'codex preboot log line' 1>&2
sleep 30
"#;

const MOCK_EARLY_EXIT: &str = r#"
read -r REQ
echo 'codex fatal: licence missing' 1>&2
exit 7
"#;

const MOCK_LEAKS_SECRET: &str = r#"
echo 'preflight using token lr_tok_xyz' 1>&2
read -r REQ
printf '{"jsonrpc":"2.0","id":1,"result":{"userAgent":"mock","codexHome":"/tmp","platformFamily":"unix","platformOs":"macos"}}\n'
"#;

fn workspace() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn launcher(script: &str, ws: &tempfile::TempDir) -> AppServerLauncher {
    AppServerLauncher::new(script, ws.path().to_path_buf())
        .with_startup_timeout(Duration::from_secs(5))
        .with_client_info(ClientInfo {
            name: "cadenza-test".into(),
            title: None,
            version: "0.0.0".into(),
        })
}

#[tokio::test]
async fn successful_initialize_returns_typed_response() {
    let ws = workspace();
    let client = launcher(MOCK_OK, &ws).launch().await.expect("launch");
    let init = client.initialize_response();
    assert_eq!(init.user_agent, "mock-codex/0.0");
    assert_eq!(init.codex_home, "/tmp/codex-home");
    assert_eq!(init.platform_family, "unix");
    client.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn protocol_error_is_classified() {
    let ws = workspace();
    let err = launcher(MOCK_ERROR, &ws).launch().await.unwrap_err();
    match err {
        LaunchError::Protocol { code, message, .. } => {
            assert_eq!(code, -32601);
            assert!(message.contains("method not found"));
        }
        other => panic!("expected Protocol, got {other:?}"),
    }
}

#[tokio::test]
async fn startup_timeout_kills_child_within_bound() {
    let ws = workspace();
    let start = Instant::now();
    let err = launcher(MOCK_STALL, &ws)
        .with_startup_timeout(Duration::from_millis(300))
        .launch()
        .await
        .unwrap_err();
    let elapsed = start.elapsed();
    assert!(matches!(err, LaunchError::Timeout(_)), "got {err:?}");
    assert!(
        elapsed < Duration::from_secs(2),
        "expected timeout under 2s, took {elapsed:?}",
    );
}

#[tokio::test]
async fn early_exit_reports_stderr_tail() {
    let ws = workspace();
    let err = launcher(MOCK_EARLY_EXIT, &ws).launch().await.unwrap_err();
    match err {
        LaunchError::EarlyExit { stderr_tail } => {
            assert!(stderr_tail.contains("licence missing"), "{stderr_tail}");
        }
        other => panic!("expected EarlyExit, got {other:?}"),
    }
}

#[tokio::test]
async fn rejects_workspace_that_does_not_exist() {
    let bogus = std::path::PathBuf::from("/var/cadenza/this-must-not-exist-xyz");
    let err = AppServerLauncher::new("true", bogus)
        .launch()
        .await
        .unwrap_err();
    assert!(
        matches!(err, LaunchError::WorkspaceMissing(_)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn rejects_relative_workspace_path() {
    let err = AppServerLauncher::new("true", "relative/path")
        .launch()
        .await
        .unwrap_err();
    assert!(
        matches!(err, LaunchError::WorkspaceNotAbsolute(_)),
        "got {err:?}",
    );
}

#[tokio::test]
async fn stderr_snapshot_redacts_known_secret() {
    let ws = workspace();
    let client = launcher(MOCK_LEAKS_SECRET, &ws)
        .with_secrets(vec!["lr_tok_xyz".into()])
        .launch()
        .await
        .expect("launch");
    // Give the stderr capture task a moment to read the preflight line —
    // bounded to avoid hanging if the task never runs.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let snap = client.stderr_snapshot().await;
        if snap.contains("***REDACTED***") {
            assert!(!snap.contains("lr_tok_xyz"), "leaked: {snap}");
            break;
        }
        if Instant::now() >= deadline {
            panic!("stderr snapshot never showed the redacted line");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    client.shutdown().await.expect("shutdown");
}
