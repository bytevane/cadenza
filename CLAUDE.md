# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this repo is

Cadenza is a **contract-first** Rust + WebAssembly orchestration runtime for Symphony-style Codex workflows. It is an early scaffold: WIT/ABI surfaces, Codex app-server schema, and `tools/versions.toml` are frozen contracts that gate CI before any feature code lands. Treat patches as additions to these contracts, not free-form Rust.

The full AI policy lives in `CONTRIBUTING_AI.md` — read it before opening a PR. The PR template in `.github/pull_request_template.md` is mandatory and asks which contracts were touched.

## Common commands

```bash
# One-shot local equivalent of CI rust job
./scripts/check-all.sh

# Individual steps
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p cadenza-linear-graphql-plugin --target wasm32-wasip2 --release

# Single test
cargo test -p cadenza-core --lib contracts
cargo test -p cadenza-workflow parses_workflow_frontmatter_and_body

# CLI smoke
cargo run -p cadenza-cli -- doctor --workflow WORKFLOW.example.md
cargo run -p cadenza-cli -- workspace-key ABC-123/foo
cargo run -p cadenza-cli -- workspace-path --root /abs/root ABC-123
```

## Contract gates (do not silently break)

Three gates run in CI. Each one fails loudly with a regen command; do not paper over a failure by editing the snapshot without an ADR.

1. **WIT ABI** — `./scripts/check-wit-abi.sh` diffs `wit/runtime.wit` against `abi/expected/runtime.wit` AND extracts the world WIT from the built plugin component against `abi/expected/cadenza-linear-graphql-plugin.world.wit`. Updating either snapshot is an ABI change — pair with an ADR under `decisions/` and bump the WIT package version per `docs/operations/wit-abi-versioning.md`. Pre-1.0, every minor bump is treated as breaking unless the ADR explicitly says additive-only.

2. **Codex schema** — `./scripts/codex-schema.sh --check` regenerates `schemas/codex/current/` with the pinned `codex` CLI, normalizes JSON via `jq --sort-keys`, and compares the aggregate sha256 against `ci/expected/codex-schema.sha256`. Run without `--check` to record a new hash. Procedure for an intentional bump lives in `docs/operations/codex-schema-upgrade.md`.

3. **Contract registry** — `crates/cadenza-core/src/contracts.rs` has two tests (`registry_text_has_no_pending_critical_keys`, `registry_text_documents_every_critical_key`) that read `tools/versions.toml` at compile time via `include_str!`. Every key in `MVP_CRITICAL_KEYS` must be present and free of `TODO`. Changing pinned versions requires a dedicated PR + ADR per `CONTRACTS.md` — do not bundle with feature work.

The `tools/versions.toml` ledger is the single source of truth for: Rust toolchain (mirrored in `rust-toolchain.toml`), Codex CLI version, Wasmtime family/version, WIT package version (`cadenza:runtime@0.2.0`), wasm-tools and wit-bindgen versions. The CI Codex install step parses `codex.cli_version` directly out of this file.

## Architecture

One workspace, one crate per boundary. Each crate isolates a single contract surface so an AI patch can stay within one crate.

- `cadenza-core` — domain types (`Issue`, `RunAttempt`, `RunStatus`), `workspace_key` sanitization, the contract-registry checker. Everything downstream depends on it; keep it small.
- `cadenza-workflow` — `WORKFLOW.md` parser (YAML front matter + Jinja prompt body) and **strict** Minijinja rendering (undefined vars fail).
- `cadenza-workspace` — maps an issue identifier to a path under `workspace.root`. `ensure_inside` enforces containment; all FS access must go through it (see `SECURITY.md`).
- `cadenza-orchestrator` — the **single-authority** in-memory state owner (`RuntimeState`: `claimed`, `running`, `retry_attempts`). No I/O lives here.
- `cadenza-codex` — Codex app-server client boundary. MVP only accepts `CodexTransport::Stdio` and requires a pinned `schema_sha256`. Protocol-adjacent code is Codex's lane per `CONTRIBUTING_AI.md`.
- `cadenza-tracker-linear` — `IssueTrackerClient` trait + Linear config. **Tracker writes are intentionally kept out of the orchestrator path** — they go through Wasm `host-linear` instead.
- `cadenza-wasm-host` — Wasmtime limits, expected WIT package/world constants (`cadenza:runtime@0.2.0` / `tool-runtime`), capability-policy error type. Actual component loading is unimplemented until the WIT freeze is stable.
- `cadenza-obs` — canonical log field name constants and `redact_value` for token/secret-shaped keys. Use these constants instead of inventing strings.
- `cadenza-cli` — operator entrypoint (`doctor`, `workspace-key`, `workspace-path`).
- `plugins/cadenza-linear-graphql-plugin` — built as `wasm32-wasip2` `cdylib`; its component world WIT is one of the ABI snapshots.

The host imports `host-log`, `host-time`, `host-workspace`, `host-http`, `host-secrets`, `host-linear`, `host-tools` and the guest exports `tool`. **Raw secrets never cross into guest memory** — `host-secrets` only discloses presence; `host-linear` injects credentials host-side. See `wit/runtime.wit` for the authoritative signatures.

## Patch discipline

- **One PR = one issue** with `Closes #N`. One crate or one doc surface per PR is the default; cross-crate patches need justification in the PR description.
- Don't bundle a refactor with a fix. Don't opportunistically rename unused vars, re-export types, or delete unrelated comments while implementing a feature.
- Branch naming: `issue-<n>-<slug>` when tracked, otherwise `feat/`, `fix/`, `infra/`, `sec/`, `docs/`, `chore/` (see `CONTRIBUTING_AI.md`).
- A patch that touches Codex schema, WIT ABI, secret handling, workspace path safety, orchestrator state semantics, or observability field names **requires an ADR** under `decisions/`.
- Use `prompts/codex-runtime.md` for protocol/app-server work and `prompts/claude-dev.md` for general implementation. Never invent protocol fields not present in the generated schemas or WIT functions not present in `wit/runtime.wit`.
- Do not skip tests via `#[ignore]`, `--no-verify`, or `if false`. New behaviour ships with a failing-first test (TDD).
