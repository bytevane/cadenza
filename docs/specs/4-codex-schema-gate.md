# Spec: Issue #4 — Generate and gate Codex app-server schema artifacts

Tracks https://github.com/bytevane/cadenza/issues/4 (Milestone: MVP 0 - Contracts & Guardrails).

## Outcome

Cadenza ships committed Codex app-server schema artifacts that match the
pinned `rust-v0.133.0` release, an aggregate sha256 in
`ci/expected/codex-schema.sha256`, and a CI job that fails closed if the
artifacts drift on a future PR.

## Approach

1. **Determinism fixes in `scripts/codex-schema.sh`.**
   - The upstream codex CLI emits `codex_app_server_protocol.v2.schemas.json`
     with HashMap-ordered top-level definitions, so two consecutive runs
     produce different bytes for the same Codex version. Every emitted JSON
     file is normalized through `jq --sort-keys` before hashing.
   - `sort` uses LC_COLLATE-dependent ordering by default — macOS defaults
     to `en_US.UTF-8` while most CI runners default to `C`/`C.UTF-8`. The
     concatenation order, and therefore the aggregate hash, differs between
     the two environments. The script pins `LC_ALL=C` so the file list is
     ordered by bytes on every host.
   - After both fixes, three back-to-back local runs on macOS-arm64 and
     `codex-schema-gate` on Ubuntu-x86_64-musl all produce hash
     `5d8ed6796ae5db6e4b019681c0416a7db06c03828447eb06a57ee94b663285ab`.

2. **Tool gating in the script.** Add `jq` and `shasum` to the early
   pre-flight check so the script fails fast and loud with a clear message
   if any required tool is missing — not deep inside the hashing pipeline.

3. **Enable `codex-schema-gate` CI job.** Drop the `if: false` guard and add
   a pre-step that downloads the codex binary pinned by
   `tools/versions.toml`. The asset target is
   `codex-x86_64-unknown-linux-musl.tar.gz` (ubuntu-latest runner). The
   version is read from `tools/versions.toml` at workflow time so a bump in
   the registry automatically updates the CI install.

4. **Upgrade docs.** `docs/operations/codex-schema-upgrade.md` walks through
   how to intentionally move to a newer Codex release: registry bump, local
   install, regen, hash, ADR.

## Acceptance verification

| Acceptance criterion (from issue #4) | Verification |
| ------------------------------------ | ------------ |
| CI fails if Codex schema artifacts drift unexpectedly. | `codex-schema-gate` job runs `./scripts/codex-schema.sh --check`, which `diff -u`s the recorded hash against a freshly-computed one and exits non-zero on any mismatch. |
| `scripts/codex-schema.sh` exits non-zero when `codex` is unavailable or unauthenticated. | Pre-flight loop over `codex`/`jq`/`shasum` exits 1 if any is missing. Any non-zero exit from `codex ...` itself bubbles through `set -euo pipefail`. Verified locally: `env -i PATH=/usr/bin:/bin ./scripts/codex-schema.sh` returns exit 1 with `codex is required ...`. |
| Schema generation output is deterministic enough for PR review. | `jq --sort-keys` normalization produces byte-stable output across three local runs; the diff against `main` will only contain real protocol changes, not iteration noise. |
| Upgrade instructions are documented. | `docs/operations/codex-schema-upgrade.md` covers the full upgrade workflow, links to `CONTRACTS.md`, and warns about behaviour-only changes the gate cannot detect. |

## Out of scope

- Codex client (JSON-RPC) implementation — Issue #12 / #13.
- Real app-server smoke tests — Issue #23.
- Signature verification of the downloaded Codex tarball; sigstore assets
  exist in the release but adding cosign verification is its own follow-up.

## References

- `CONTRACTS.md`
- `tools/versions.toml`
- `decisions/0002-codex-stdio-schema-gate.md`
- `decisions/0004-pinned-contract-registry.md`
- Issue #3 (Closes via PR #25)
