# Upgrading the Codex app-server schema

Cadenza tracks a specific Codex CLI release. Generated schema artifacts live
in `schemas/codex/current/` and their aggregate hash is committed to
`ci/expected/codex-schema.sha256`. The `codex-schema-gate` CI job
regenerates them on every PR and fails if anything drifts.

This document is the procedure for **intentionally** moving to a newer
Codex release.

## 1. Pick the new Codex version

Choose a non-prerelease tag from
<https://github.com/openai/codex/releases>. Cadenza only tracks `rust-v*`
releases (the Rust app-server). Read the release notes for protocol-level
changes — added fields, renamed types, removed methods — so the PR can call
them out.

## 2. Bump the contract registry

1. Update `codex.cli_version` in `tools/versions.toml` to the new tag.
2. Run the cadenza-core contract tests:

   ```bash
   cargo test -p cadenza-core --lib contracts
   ```

   They must stay green. (`MVP_CRITICAL_KEYS` still requires
   `cli_version` to be pinned without a `TODO` placeholder.)

## 3. Install the matching Codex CLI locally

The release ships pre-built binaries. On macOS/Linux the canonical install
is the platform tarball linked from the release page. Once installed:

```bash
codex --version
# codex-cli <new-version>
```

The CLI version must match `codex.cli_version` exactly, otherwise the
schema artifacts will reflect a different release and the gate will fail
in CI for a different reason than upstream drift.

## 4. Regenerate schema artifacts and hash

```bash
./scripts/codex-schema.sh
```

The script:

- Wipes and rewrites `schemas/codex/current/`.
- Runs `codex app-server generate-ts` and `codex app-server generate-json-schema`.
- Normalizes every emitted `*.json` file through `jq --sort-keys` to remove
  HashMap-iteration non-determinism in the upstream generator.
- Aggregates the sorted file list and writes the sha256 to
  `ci/expected/codex-schema.sha256`.

Run the script twice and confirm the hash is identical between runs. If it
is not, **stop and investigate** before opening a PR — the normalization
step (`jq --sort-keys`) is the only deterministic source-of-truth.

## 5. Diff against `main`

```bash
git diff --stat schemas/codex/current/
```

A protocol bump usually touches every aggregated file. Spot-check the
diff for fields that affect Cadenza's Codex client surface (event names,
required params, approval-flow types). Call those out in the PR body so
reviewers know which downstream code might need to follow.

## 6. Open a contract upgrade PR

Per `CONTRACTS.md`, a Codex version bump is a registry change:

- One PR.
- Update `tools/versions.toml`, `schemas/codex/current/`, and
  `ci/expected/codex-schema.sha256` together.
- Add or amend an ADR under `decisions/` describing why the bump is
  happening (security fix, new capability we want, dropped feature we
  relied on, etc.).
- Do **not** bundle Codex client code changes in this PR; land them
  separately so the contract bump can be reverted cleanly if it breaks.

## What this gate does not catch

- Behavioural changes that keep the schema stable. The hash compares
  generated types, not runtime semantics. Manual smoke tests against a
  real `codex app-server` instance are still required for protocol
  changes that move logic without renaming types.
- Non-Cadenza features inside the Codex CLI (e.g. terminal UI changes).
  The gate only protects the app-server boundary.
