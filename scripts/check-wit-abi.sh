#!/usr/bin/env bash
# Cadenza WIT ABI gate.
#
# Enforces two ABI snapshots living under abi/expected/:
#
# 1. runtime.wit — the canonical package-level WIT source under wit/.
#    Any edit to wit/runtime.wit without a matching update of the snapshot
#    fails this check.
#
# 2. cadenza-linear-graphql-plugin.world.wit — the world WIT extracted from
#    the built example component plugin. Any change to the plugin's
#    component exports/imports without a snapshot update fails this check.
#
# Bumping either snapshot is an ABI change: pair it with an ADR in
# decisions/ and call out the break in the PR body.
# See docs/operations/wit-abi-versioning.md.
set -euo pipefail
export LC_ALL=C

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

for tool in wasm-tools cargo diff; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "$tool is required by scripts/check-wit-abi.sh" >&2
    if [ "$tool" = "wasm-tools" ]; then
      echo "Install with: cargo install --locked wasm-tools" >&2
    fi
    exit 1
  fi
done

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

# 1. Validate that the WIT package round-trips through the binary encoder.
wasm-tools component wit ./wit -t > "$tmp_dir/runtime.witpkg"
wasm-tools component wit ./wit --json > "$tmp_dir/runtime.wit.json"

# 2. Source-level ABI snapshot diff.
expected_source="abi/expected/runtime.wit"
if ! diff_output=$(diff -u "$expected_source" wit/runtime.wit 2>&1); then
  {
    echo "WIT mismatch: wit/runtime.wit drifted from $expected_source."
    echo "Intentional change? Copy wit/runtime.wit -> $expected_source in the same PR and bump the WIT package version + ADR."
    echo
    echo "$diff_output"
  } >&2
  exit 1
fi

# 3. Build the example component plugin so we can extract its world WIT.
plugin_wasm="target/wasm32-wasip2/release/cadenza_linear_graphql_plugin.wasm"
cargo build -p cadenza-linear-graphql-plugin --target wasm32-wasip2 --release >&2
if [ ! -f "$plugin_wasm" ]; then
  echo "expected component plugin at $plugin_wasm but cargo build did not produce it" >&2
  exit 1
fi

# 4. The plugin must be a valid component, not a raw module.
wasm-tools validate "$plugin_wasm"

# 5. Extract world WIT and compare against the snapshot.
expected_world="abi/expected/cadenza-linear-graphql-plugin.world.wit"
extracted="$tmp_dir/cadenza-linear-graphql-plugin.world.wit"
wasm-tools component wit "$plugin_wasm" > "$extracted"

if ! diff_output=$(diff -u "$expected_world" "$extracted" 2>&1); then
  {
    echo "WIT mismatch: cadenza-linear-graphql-plugin world drifted from $expected_world."
    echo "Regenerate with: wasm-tools component wit $plugin_wasm > $expected_world"
    echo "Bumping plugin ABI is a breaking change pre-1.0 — see docs/operations/wit-abi-versioning.md."
    echo
    echo "$diff_output"
  } >&2
  exit 1
fi

echo 'WIT ABI check passed.'
