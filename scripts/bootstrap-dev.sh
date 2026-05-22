#!/usr/bin/env bash
set -euo pipefail

printf '==> Rust toolchain\n'
rustup target add wasm32-wasip2

printf '==> wasm-tools\n'
if ! command -v wasm-tools >/dev/null 2>&1; then
  cargo install --locked wasm-tools
else
  wasm-tools --version
fi

printf '==> Optional external tools\n'
if command -v codex >/dev/null 2>&1; then
  codex --version || true
else
  echo 'codex CLI not found. Install/pin it before generating app-server schemas.'
fi

if command -v claude >/dev/null 2>&1; then
  claude -v || true
else
  echo 'Claude Code CLI not found. It is optional and used only for development-time generation.'
fi

printf '==> Rust checks\n'
cargo fmt --all -- --check
cargo test --workspace

printf 'Bootstrap complete.\n'
