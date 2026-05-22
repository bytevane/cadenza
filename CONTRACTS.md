# Cadenza Contract Registry

`tools/versions.toml` is the single ledger pinning the upstream facts that the
codebase depends on. Treat it like a schema, not like a casual config file.

## What is pinned

| Key                              | Owner concern                                                          |
| -------------------------------- | ---------------------------------------------------------------------- |
| `upstream.symphony_spec_sha`     | Behavioral baseline: `openai/symphony/SPEC.md` at a specific commit.    |
| `rust.toolchain_version`         | Exact toolchain used by CI and developer environments.                  |
| `rust.minimum_workspace_rust_version` | Published-crate MSRV (independent from `toolchain_version`).       |
| `codex.cli_version`              | Exact Codex CLI / app-server release driving the protocol contracts.    |
| `codex.schema_sha256_file`       | File path holding the generated app-server schema hash (Issue #4).     |
| `wasm.wasmtime_family` / `wasmtime_version` | Wasmtime crate family and exact crate version.              |
| `wasm.wit_package` / `wit_version` | Authoritative WIT package identifier and version (`wit/runtime.wit`). |
| `wasm.wasm_tools_version`        | `wasm-tools` CLI used to validate WIT/ABI and components.               |
| `wasm.wit_bindgen_version`       | `wit-bindgen` version (if generated bindings ship).                     |

The MVP-critical subset — the ones that must never carry a `TODO` placeholder
— lives in code: `cadenza_core::contracts::MVP_CRITICAL_KEYS`. Two tests guard
the registry from drift:

- `cadenza_core::contracts::tests::registry_text_has_no_pending_critical_keys`
- `cadenza_core::contracts::tests::registry_text_documents_every_critical_key`

Both run inside the standard `cargo test --workspace` job in `ci.yml`.

## How to change a pinned contract

1. Open a dedicated PR. **One PR may touch only the registry plus the
   artifacts directly generated from the new pin** (schema artifacts,
   ABI snapshots, generated bindings). It must not bundle feature work.
2. Update the relevant key(s) in `tools/versions.toml`.
3. Update `rust-toolchain.toml`, `Cargo.toml` workspace deps, or the relevant
   `.github/workflows/*.yml` step so the rest of the repo agrees with the new
   pin.
4. Add or amend the matching ADR under `decisions/` describing **why** the
   contract moved — link the upstream release notes or commit.
5. Re-run any generator that depends on the contract (e.g.
   `scripts/codex-schema.sh` after a Codex version bump,
   `scripts/check-wit-abi.sh` after a Wasmtime/wasm-tools bump).
6. Ensure both contract registry tests still pass.

## What this registry deliberately does not pin

- The Claude Code CLI / Agent SDK version. Claude Code is a development-time
  generator/reviewer, not a runtime dependency, and is intentionally marked
  `unpinned-dev-only` in `tools/versions.toml`. If Claude Code is ever wired
  into CI for autonomous patching, lift it into the MVP-critical set and pin.
- Container base image digests and OS package versions. Those belong in the
  build-system manifest once images exist.
