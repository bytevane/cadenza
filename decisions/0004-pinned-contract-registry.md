# ADR 0004: Pinned upstream contract registry

## Status

Accepted.

## Decision

`tools/versions.toml` is the canonical, machine-readable ledger pinning the
upstream facts Cadenza targets (Symphony SPEC commit, Codex CLI version,
Wasmtime/wasm-tools/wit-bindgen versions, Rust toolchain). The MVP-critical
subset is enforced in code via `cadenza_core::contracts::MVP_CRITICAL_KEYS`
and two `cargo test` checks that run in CI.

Any change to a pinned contract must land in a dedicated PR that updates the
registry, any downstream config that consumes it, and a paired ADR explaining
why the contract moved.

## Rationale

Contract-first development requires a single place to answer "which upstream
SPEC and which Codex schema does this commit target?" Scattering versions
across `Cargo.toml`, `rust-toolchain.toml`, README prose, and CI workflows
made drift invisible. Centralizing the pins and asserting on them at test
time turns silent drift into a CI failure.

## Consequences

- `tools/versions.toml` is part of the contract surface, not a hint file.
- CI fails if an MVP-critical pin carries a `TODO` placeholder or is removed.
- Adding a new MVP-critical contract means appending to `MVP_CRITICAL_KEYS`
  and pinning a value in the same PR.
- Codex schema hash (`ci/expected/codex-schema.sha256`) is referenced via
  `codex.schema_sha256_file` rather than duplicated in the registry. Issue #4
  owns generating and gating that artifact.
- Claude Code stays explicitly unpinned because it is a development-time
  generator, not a runtime dependency.
