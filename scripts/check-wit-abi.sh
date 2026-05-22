#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v wasm-tools >/dev/null 2>&1; then
  echo 'wasm-tools is required. Run: cargo install --locked wasm-tools' >&2
  exit 1
fi

# Validate that the WIT package can be encoded and converted back.
wasm-tools component wit ./wit -t >/tmp/cadenza-runtime.witpkg
wasm-tools component wit ./wit --json >/tmp/cadenza-runtime.wit.json

# Source-level ABI snapshot. This intentionally fails on any WIT edit unless the
# expected snapshot is updated in the same PR.
diff -u abi/expected/runtime.wit wit/runtime.wit

echo 'WIT ABI check passed.'
