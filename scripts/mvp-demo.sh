#!/usr/bin/env bash
# MVP demo (#24). A guided walkthrough of the cadenza loop for a new
# contributor or stakeholder — every step prints what it is about to do
# and which crate it is exercising.
#
# The demo is fully offline and credentials-free: it runs the mock
# smoke (#22), exercises the CLI's helper commands, and exits.
# Operators who want to talk to the real Linear / Codex services should
# instead read `docs/operations/real-smoke.md` and run
# `./scripts/real-smoke.sh`.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

bold() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }

bold 'Step 1/4: print the pinned contract matrix'
echo 'See docs/operations/compatibility-matrix.md for the full table.'
awk '/^\[/{section=$0} section ~ /upstream|codex|wasm|rust/{print}' tools/versions.toml

bold 'Step 2/4: run the workspace gate (fmt + clippy + tests)'
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --quiet

bold 'Step 3/4: run the mock MVP smoke (mock tracker → orchestrator → mock Codex → snapshot)'
./scripts/mvp-smoke.sh

bold 'Step 4/4: exercise the CLI helper commands'
cargo run --quiet -p cadenza-cli -- doctor --workflow WORKFLOW.example.md
cargo run --quiet -p cadenza-cli -- workspace-key ABC-123/scratch

bold 'Demo complete'
echo 'Next: read docs/operations/rollback-drill.md and docs/operations/release-notes-template.md'
