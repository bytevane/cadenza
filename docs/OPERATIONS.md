# Operations

## Default deployment posture

- Run the Rust host as a dedicated non-root user.
- Mount `workspace.root` and log directories explicitly.
- Keep the HTTP status API bound to loopback until authentication is implemented.
- Prefer `stdio://` for Codex app-server in MVP.
- Avoid WebSocket app-server transport in production until explicit auth and threat modeling are complete.

## First alerts

- `codex_schema_mismatch`
- `wit_abi_mismatch`
- `workspace_escape_denied`
- `codex_start_failed`
- `hook_timeout`
- `retry_queue_growth`
- `wasm_epoch_timeout`
- `secret_scrub_hit`

## Rollback

The orchestrator should not rely on a durable correctness database. Preserve:

- issue tracker state
- per-issue workspace directories
- structured logs
- version tuple

A rollback should be a container/image rollback plus preserved workspace/log volumes.
