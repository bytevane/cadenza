#!/usr/bin/env bash
# MVP smoke test — runs the in-process mock-driven end-to-end test
# defined in `crates/cadenza-cli/tests/mvp_smoke.rs`. No external
# services required. The test exercises:
#
#   tracker mock → workflow parse → orchestrator dispatch
#   → mock Codex event stream → lifecycle decision
#   → observability snapshot + redaction
#
# The Wasm host capability hop is intentionally out of scope while
# issues #16 / #17 are blocked.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo '==> cargo test -p cadenza-cli --test mvp_smoke'
exec cargo test -p cadenza-cli --test mvp_smoke -- --nocapture
