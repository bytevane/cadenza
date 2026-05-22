# Spec: Issue #12 — Codex app-server stdio launcher + handshake

Tracks https://github.com/bytevane/cadenza/issues/12 (Milestone: MVP 2 - Runtime Integrations).

## Outcome

`cadenza-codex` ships an `AppServerLauncher` that spawns the pinned
Codex CLI under `bash -lc`, completes a JSON-RPC `initialize`
handshake, and returns a live `AppServerClient` holding the child
process. Stderr is captured into a bounded, redacted buffer on a
background task; stdout is the protocol channel. Mock app-server
fixtures (inline shell scripts) cover every acceptance path without
requiring a real `codex` binary on CI.

## Public surface

```rust
pub mod launcher;
pub mod protocol;

pub use launcher::{AppServerClient, AppServerLauncher, DEFAULT_STDERR_CAP_BYTES, LaunchError};
pub use protocol::{
    ClientInfo, InitializeCapabilities, InitializeParams, InitializeResponse, JsonRpcError,
};
```

`AppServerLauncher` is a builder:

```rust
AppServerLauncher::new(command, workspace)
    .with_startup_timeout(Duration::from_secs(15))
    .with_client_info(ClientInfo { /* … */ })
    .with_capabilities(InitializeCapabilities::default())
    .with_secrets(vec!["lr_tok_…".into()])
    .launch().await
```

`AppServerClient` exposes the parsed `InitializeResponse`, an async
`stderr_snapshot()` (bounded + redacted), and `shutdown().await`.

## Behaviour contract

- **Bash invocation**: `bash -lc <command>` so workflows that say `command: "codex app-server --listen stdio://"` work unchanged.
- **Workspace check**: launcher refuses non-absolute paths (`WorkspaceNotAbsolute`) and missing directories (`WorkspaceMissing`) before spawning anything.
- **Process group**: on unix the child is its own group leader via `process_group(0)` so shutdown SIGKILLs every grandchild — orphaned `sleep` background processes cannot keep stdio pipes open.
- **JSONL protocol**: launcher writes `<request>\n` and reads stdout line-by-line; blank lines are skipped. The `id` field is `1` for the initialize request; further requests will follow in #13.
- **Stderr capture**: a background `tokio::spawn` reads stderr into a `BoundedBuf` (default 64 KiB, overridable via `with_stderr_cap_bytes`). `stderr_snapshot()` returns the buffer redacted against the secrets registered with `with_secrets`. Secrets are sorted longest-first so a shorter prefix cannot leak a longer secret's suffix.
- **Startup timeout**: `tokio::time::timeout` wraps the read+write+decode pipeline. On expiry the child is SIGKILLed, the stderr task is aborted, and `LaunchError::Timeout(d)` is returned.
- **Error classification**: distinct variants for IO failure (`Write`, `Read`), JSON failure (`Encode`, `Decode`), protocol failure (`Protocol { code, message, stderr_tail }`), early exit (`EarlyExit { stderr_tail }`), and timeout. All error variants that involve a running child trigger a SIGKILL before they're returned.

## Acceptance verification

| Acceptance criterion (from #12) | Verification |
| --- | --- |
| Mock app-server handshake passes in CI. | `successful_initialize_returns_typed_response`. |
| Startup timeout kills child process. | `startup_timeout_kills_child_within_bound` (mock sleeps 30s, timeout 300ms, elapsed asserted under 2s). |
| Stderr is captured and redacted. | `stderr_snapshot_redacts_known_secret`. |
| Launcher refuses to run outside the workspace. | `rejects_workspace_that_does_not_exist` + `rejects_relative_workspace_path`. |
| Protocol assumptions live only inside `cadenza-codex`. | All JSON-RPC + Initialize types live in `crates/cadenza-codex/src/protocol.rs`. |

## Boundary tests (per project rule)

- Timeout: `Timeout` (handshake never arrives) paired with `Success` (handshake arrives within budget) — exercises both branches of the `tokio::time::timeout` decision.
- Workspace gate: missing-path (`WorkspaceMissing`) paired with relative-path (`WorkspaceNotAbsolute`) — both rejection branches before the spawn.

## Out of scope (per #12 non-goal)

- Turn lifecycle (#13).
- Real Codex smoke test (#23).
- Workspace-key sanitiser (#10, already shipped).

## References

- `schemas/codex/current/InitializeParams.ts`, `InitializeResponse.ts`, `ClientInfo.ts`, `InitializeCapabilities.ts`.
- `decisions/0002-codex-stdio-schema-gate.md`.
