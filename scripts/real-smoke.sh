#!/usr/bin/env bash
# Real integration smoke (#23). Opt-in profile that talks to the
# upstream Codex app-server and Linear API. NEVER runs in the
# default `cargo test --workspace` profile.
#
# Required env vars:
#   CADENZA_REAL_SMOKE_CODEX=1
#     Set to opt into the Codex handshake check. The `codex` binary
#     must be on $PATH and signed in.
#   CADENZA_CODEX_COMMAND        (optional, defaults to "codex app-server --listen stdio://")
#     Command Cadenza spawns to reach the app-server.
#
#   CADENZA_LINEAR_TOKEN
#     The operator's Linear API token. Never echoed.
#   CADENZA_LINEAR_PROJECT_SLUG_ID
#     Linear project id the smoke fetches a small page of issues from.
#
#   CADENZA_REAL_SMOKE_SECRETS   (optional, comma-separated)
#     Extra secret values to register with the scrubber so they cannot
#     appear in captured test output.
#
# Missing env vars → the corresponding tests skip with a clear
# message; they do NOT fail.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo '==> cargo test -p cadenza-cli --test real_smoke -- --ignored --nocapture'
exec cargo test -p cadenza-cli --test real_smoke -- --ignored --nocapture
