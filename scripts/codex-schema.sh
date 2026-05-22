#!/usr/bin/env bash
# Regenerate Codex app-server schema artifacts and either record (default)
# or verify (--check) the aggregate sha256.
#
# Determinism notes:
#  - `codex app-server generate-json-schema` emits the aggregated
#    `codex_app_server_protocol.v2.schemas.json` with HashMap-ordered top-level
#    definitions, so the raw output is not byte-stable between runs. Every
#    JSON file is normalized through `jq --sort-keys` before hashing.
#  - `sort` orders bytes differently under `en_US.UTF-8` (macOS default) and
#    `C`/`C.UTF-8` (most CI runners). The hash pipeline pins `LC_ALL=C` so
#    the file list ordering — and therefore the aggregate hash — is identical
#    on every host.
set -euo pipefail
export LC_ALL=C

MODE="${1:-generate}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/schemas/codex/current"
EXPECTED="$ROOT/ci/expected/codex-schema.sha256"
TMP_SHA="$ROOT/.codex-schema.sha256.tmp"

for tool in codex jq shasum; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "$tool is required by scripts/codex-schema.sh" >&2
    exit 1
  fi
done

rm -rf "$OUT"
mkdir -p "$OUT"

codex --version | tee "$OUT/CODEX_VERSION.txt"
codex app-server generate-ts --out "$OUT"
codex app-server generate-json-schema --out "$OUT"

# Normalize every emitted JSON file so HashMap iteration order in the
# upstream generator cannot flip the aggregate hash on identical content.
while IFS= read -r -d '' json; do
  jq --sort-keys . "$json" > "$json.normalized"
  mv "$json.normalized" "$json"
done < <(find "$OUT" -type f -name '*.json' -print0)

find "$OUT" -type f -print0 \
  | sort -z \
  | xargs -0 cat \
  | shasum -a 256 \
  | awk '{print $1}' > "$TMP_SHA"

if [[ "$MODE" == "--check" ]]; then
  if ! diff -u "$EXPECTED" "$TMP_SHA"; then
    echo "Codex schema hash differs from $EXPECTED — regenerate with scripts/codex-schema.sh and commit the result." >&2
    rm -f "$TMP_SHA"
    exit 1
  fi
else
  mkdir -p "$(dirname "$EXPECTED")"
  cp "$TMP_SHA" "$EXPECTED"
  echo "Wrote $EXPECTED"
fi

cat "$TMP_SHA"
rm -f "$TMP_SHA"
