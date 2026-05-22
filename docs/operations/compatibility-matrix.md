# Compatibility matrix (MVP)

Cadenza pins every upstream contract it depends on. This page is the
canonical answer to "what does this build link against?" — the source
of truth is `tools/versions.toml`; this table is a human-readable
projection kept in sync with it.

| Surface | Pinned value | Source of truth |
| --- | --- | --- |
| Cadenza release | `0.1.0-mvp` | `Cargo.toml` workspace `version` (per-crate) |
| Symphony SPEC commit | `2c1851830477434100fdb8980fcc1fce1a8af81d` | `tools/versions.toml` `[upstream] symphony_spec_sha` |
| Rust toolchain | `1.95.0` (channel `stable`) | `rust-toolchain.toml` + `tools/versions.toml` `[rust] toolchain_version` |
| Workspace MSRV | `1.85` | `tools/versions.toml` `[rust] minimum_workspace_rust_version` |
| Wasm target | `wasm32-wasip2` | `tools/versions.toml` `[rust] wasm_target` |
| Codex CLI / app-server | `rust-v0.133.0` | `tools/versions.toml` `[codex] cli_version` |
| Codex transport | `stdio://` | `tools/versions.toml` `[codex] transport` |
| Codex schema hash | tracked in `ci/expected/codex-schema.sha256` | `scripts/codex-schema.sh --check` (CI gate) |
| WIT package | `cadenza:runtime@0.2.0` | `tools/versions.toml` `[wasm] wit_package` |
| WIT/ABI snapshot | `abi/expected/` source + component world hashes | `scripts/check-wit-abi.sh` (CI gate) |
| Wasmtime family | `45` (exact crate version `45.0.0`) | `tools/versions.toml` `[wasm] wasmtime_*` |
| wasm-tools | `1.250.0` | `tools/versions.toml` `[wasm] wasm_tools_version` |
| wit-bindgen | `0.57.1` (unwired pending #16) | `tools/versions.toml` `[wasm] wit_bindgen_version` |
| Claude Code | unpinned (dev-only generator) | `tools/versions.toml` `[claude_code]` |

## How to bump a row

Each row corresponds to a single ADR commit and a single PR. Bumping
multiple pinned values in one PR is explicitly disallowed by
`CONTRACTS.md` because it makes rollback harder.

The required steps for any row above:

1. Open an ADR under `decisions/`, named after the bump.
2. Update `tools/versions.toml` (single key change).
3. Update any code or scripts that read that key.
4. Run the full gate locally (`cargo fmt`, `cargo clippy -- -D warnings`,
   `cargo test --workspace`, `./scripts/mvp-smoke.sh`,
   `./scripts/check-wit-abi.sh`, `./scripts/codex-schema.sh --check`).
5. Run the real smoke at least once (`./scripts/real-smoke.sh`) for
   bumps that touch Codex or Linear.
6. Mirror the change into this table within the same PR.

## What's NOT in the matrix

- Build-time-only crates whose surface does not cross any cadenza
  contract (e.g. `serde`, `thiserror`, `tracing`). These can move
  freely through normal dependabot/manual bumps.
- Local dev tooling that is not invoked by CI.
- Production secret managers — none are integrated in the MVP.
