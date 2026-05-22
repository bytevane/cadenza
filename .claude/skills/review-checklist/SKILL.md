---
name: review-checklist
description: Pre-merge review checklist for Cadenza. Walks a diff against the project's contract gates (WIT ABI, Codex schema, contract registry), ADR-required surfaces, patch-scope discipline, and the mandatory PR template. Use before opening a PR or as a self-review pass on the current branch.
when_to_use: User says "ready to merge", "pre-merge check", "review my changes",
  "check before merge", "self-review", "review this diff", "ready to ship", or
  is about to open a PR. Also use proactively after a contract-gated file
  changes (wit/runtime.wit, schemas/codex/current/, tools/versions.toml,
  abi/expected/*, ci/expected/codex-schema.sha256).
---

# Cadenza Pre-Merge Review Checklist

Run this against the **current diff** (`git diff origin/main...HEAD`) before opening a PR, or against an open PR before requesting human review. It is organized by the failure modes that Cadenza's gates and `CONTRIBUTING_AI.md` explicitly call out — each section maps to a real CI check or a real "ADR required" trigger, not generic best practices.

For each section: read the diff, walk the boxes, and report which ones the patch satisfies, which fail, and which are not applicable. Do not silently skip — `n/a` is a valid answer, but it must be stated.

## 1. Patch scope (CONTRIBUTING_AI.md → "Patch scope")

- [ ] PR body contains exactly one `Closes #N` (or explains why the work is untracked and uses a `feat/`, `fix/`, `infra/`, `sec/`, `docs/`, or `chore/` branch).
- [ ] Branch name follows `issue-<n>-<slug>` (tracked) or `<type>/<slug>` (untracked).
- [ ] Diff stays inside a single crate **or** a single doc surface. If it crosses crates, the PR description justifies why the issue itself spans crates.
- [ ] No bundled refactor + fix in the same PR. No bundled feature + dependency bump.
- [ ] No opportunistic edits: unused `_var` renames, re-exports, comment deletions, or formatting churn unrelated to the issue.
- [ ] Patch uses the right prompt template (`prompts/codex-runtime.md` for protocol/app-server work, `prompts/claude-dev.md` otherwise) and references it in the PR template's "AI assistance" block.

## 2. Contract gates (CLAUDE.md → "Contract gates")

If any of the three gates' inputs changed, the regen + ADR procedure must be visible in the diff. Silently editing a snapshot is a failure.

### 2a. WIT ABI (`scripts/check-wit-abi.sh`)

- [ ] If `wit/runtime.wit` changed, `abi/expected/runtime.wit` was regenerated in the **same commit**.
- [ ] If the plugin world changed, `abi/expected/cadenza-linear-graphql-plugin.world.wit` was regenerated.
- [ ] WIT package version (`cadenza:runtime@X.Y.Z`) in `wit/runtime.wit` and `tools/versions.toml` (`wasm.wit_version`) agree.
- [ ] An ADR under `decisions/` accompanies the change and explicitly says "additive-only" if the bump is meant to be non-breaking pre-1.0 (per `docs/operations/wit-abi-versioning.md`).

### 2b. Codex app-server schema (`scripts/codex-schema.sh --check`)

- [ ] If `codex.cli_version` in `tools/versions.toml` changed, `schemas/codex/current/` was regenerated with the new pinned CLI and `ci/expected/codex-schema.sha256` was updated in the same PR.
- [ ] No invented protocol fields: every Codex request/response key referenced in code exists in `schemas/codex/current/`.
- [ ] Procedure in `docs/operations/codex-schema-upgrade.md` was followed; ADR present.

### 2c. Contract registry (`cadenza-core::contracts`)

- [ ] `tools/versions.toml` was **not** edited inline by this PR unless it is a dedicated registry-bump PR per `CONTRACTS.md`.
- [ ] Every key in `MVP_CRITICAL_KEYS` is still present and free of `TODO` (the two registry tests cover this — they must pass).
- [ ] If a pinned version moved, `rust-toolchain.toml`, `Cargo.toml` workspace deps, and `.github/workflows/*.yml` were updated to agree.

## 3. ADR-required surfaces (PR template → "Contract impact")

The PR template lists seven boxes. If the diff touches any of them, an ADR link must appear in the "ADR:" field.

- [ ] Codex app-server schema or `codex.cli_version` → ADR + snapshot regen
- [ ] WIT ABI (`wit/runtime.wit`, `abi/expected/*.wit`) → ADR + ABI version bump
- [ ] Secret handling, redaction, or log field surface → ADR + tests
- [ ] Workspace path safety / containment rules → ADR + tests
- [ ] Orchestrator state machine semantics (`claimed`, `running`, `retry_attempts`) → ADR + tests
- [ ] Observability field names or metric labels → ADR + update to `cadenza-obs` constants
- [ ] Pinned dependency versions in `tools/versions.toml` → ADR (dedicated PR per §2c)

## 4. Crate boundaries (CLAUDE.md → "Architecture")

Each crate isolates one contract surface. Patches that blur boundaries usually indicate the wrong crate was edited.

- [ ] `cadenza-core` change is restricted to domain types, `workspace_key` sanitization, or the registry checker. No I/O added.
- [ ] `cadenza-orchestrator` change does not introduce I/O — it remains the single-authority in-memory state owner.
- [ ] FS access in any crate goes through `cadenza-workspace::ensure_inside` (no direct `std::fs` on issue-derived paths). See `SECURITY.md`.
- [ ] Tracker writes do not appear in the orchestrator path — they route through `host-linear` per the WIT host imports.
- [ ] `cadenza-codex` accepts only `CodexTransport::Stdio` and verifies the pinned `schema_sha256`.

## 5. Wasm host / guest boundary

- [ ] No WIT function or world is referenced in code that does not exist in `wit/runtime.wit` (check against the snapshot, not training-data memory).
- [ ] `host-secrets` calls only disclose **presence**; raw secret values never cross into guest memory.
- [ ] Linear credentials are injected host-side via `host-linear`, never passed as guest arguments.
- [ ] `cadenza-wasm-host` constants (`cadenza:runtime@0.2.0`, `tool-runtime`) match `tools/versions.toml`.
- [ ] If the example plugin was rebuilt, the world WIT extracted from the component still matches `abi/expected/cadenza-linear-graphql-plugin.world.wit`.

## 6. Observability (cadenza-obs)

- [ ] Log field names use the constants exported by `cadenza-obs` — no inline string literals for canonical fields.
- [ ] Any value that may carry a token or secret-shaped key is passed through `cadenza-obs::redact_value` before logging or emission.
- [ ] No raw error messages leak credentials, file paths outside the workspace, or full request bodies.

## 7. Workflow rendering (cadenza-workflow)

- [ ] Minijinja rendering remains strict — undefined variables must fail, not silently render empty.
- [ ] New front-matter fields are parsed via the existing YAML path; no ad-hoc string parsing of `WORKFLOW.md`.

## 8. Tests (CLAUDE.md → "Patch discipline")

- [ ] New behavior ships with a **failing-first** test (verifiable from the diff order: test commit precedes implementation, or both are in one commit with a clear before/after note).
- [ ] No `#[ignore]`, no `--no-verify`, no `if false {` gates to suppress failing tests.
- [ ] No mocks where an integration test against a real Codex stub or real workspace path would catch a regression.
- [ ] All four commands from the PR template pass locally: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the relevant `./scripts/check-*.sh` if its surface changed.

## 9. Common AI-patch failure modes

These are the things an AI tool is most likely to do wrong on this repo. Check explicitly.

- [ ] The patch did not invent a Codex schema field, WIT function, or config key absent from the generated schemas / `wit/runtime.wit` / `tools/versions.toml`.
- [ ] The patch did not edit a pinned version inline (`tools/versions.toml`, `rust-toolchain.toml`) as a side effect of unrelated work.
- [ ] The patch did not regenerate a snapshot to make a failing gate "pass" without an accompanying ADR.
- [ ] The patch did not add error-handling, fallbacks, or feature flags for scenarios that cannot occur (CLAUDE.md → "Doing tasks").
- [ ] The patch did not add comments explaining what the code does, references to the current task, or "added for X flow" notes.
- [ ] Cross-component scope (protocol + Wasm in one PR) is justified in the PR description per `CONTRIBUTING_AI.md` → "Tool split".

## Reporting format

When you finish the walk, output:

1. **Block / Allow** verdict on the diff.
2. A list of every checkbox that **failed**, with the file/line and the specific remediation (e.g. "section 2a: `wit/runtime.wit:42` adds a function but `abi/expected/runtime.wit` was not regenerated — run `./scripts/check-wit-abi.sh` and commit the snapshot").
3. A list of every checkbox that is **n/a** with a one-word reason (e.g. "no WIT change").
4. If an ADR is missing for a triggered surface, name the surface and recommend the file path (`decisions/NNNN-<slug>.md` — use `./scripts/new-decision.sh` to scaffold).
