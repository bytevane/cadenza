#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-generate}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/schemas/codex/current"
EXPECTED="$ROOT/ci/expected/codex-schema.sha256"
TMP_SHA="$ROOT/.codex-schema.sha256.tmp"

if ! command -v codex >/dev/null 2>&1; then
  echo 'codex CLI is required to generate app-server schemas.' >&2
  exit 1
fi

rm -rf "$OUT"
mkdir -p "$OUT"

codex --version | tee "$OUT/CODEX_VERSION.txt"
codex app-server generate-ts --out "$OUT"
codex app-server generate-json-schema --out "$OUT"

find "$OUT" -type f -print0 \
  | sort -z \
  | xargs -0 cat \
  | shasum -a 256 \
  | awk '{print $1}' > "$TMP_SHA"

if [[ "$MODE" == "--check" ]]; then
  diff -u "$EXPECTED" "$TMP_SHA"
else
  mkdir -p "$(dirname "$EXPECTED")"
  cp "$TMP_SHA" "$EXPECTED"
  echo "Wrote $EXPECTED"
fi

cat "$TMP_SHA"
rm -f "$TMP_SHA"
