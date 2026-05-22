# Spec: Issue #3 — Freeze upstream versions and contract registry

Tracks https://github.com/bytevane/cadenza/issues/3 (Milestone: MVP 0 - Contracts & Guardrails).

## Outcome

`tools/versions.toml` is a machine-readable, fully-populated contract registry; the Cargo workspace advertises the real repository URL; future contract changes are funnelled through a single registry plus an ADR review.

## Pinned values

| Field                   | Value                                                | Source                                                            |
| ----------------------- | ---------------------------------------------------- | ----------------------------------------------------------------- |
| `symphony_spec_sha`     | `2c1851830477434100fdb8980fcc1fce1a8af81d`           | `gh api repos/openai/symphony/commits/main`                       |
| `codex.cli_version`     | `rust-v0.133.0`                                      | Latest non-prerelease `openai/codex` release at lock time         |
| `rust.toolchain_version`| `1.95.0`                                             | Latest stable from `rust-lang/rust` releases at lock time         |
| `wasm.wasmtime_version` | `45.0.0`                                             | Latest `bytecodealliance/wasmtime` v45 release matching `Cargo.toml` family pin |
| `wasm.wasm_tools_version` | `1.250.0`                                          | Latest `bytecodealliance/wasm-tools` release                      |
| `wasm.wit_bindgen_version` | `0.57.1`                                          | Latest `bytecodealliance/wit-bindgen` release                     |
| `wit_version`           | `0.2.0`                                              | Parsed from `wit/runtime.wit` package declaration                 |
| `workspace.repository`  | `https://github.com/bytevane/cadenza`                | Real GitHub repository for this project                           |

Codex schema artifacts and their canonical hash live in `ci/expected/codex-schema.sha256` and are owned by Issue #4. `tools/versions.toml` references the file location via `schema_sha256_file` so the registry is non-`TODO` while the actual hash remains in its dedicated artifact.

The Claude Code CLI version stays unpinned in `tools/versions.toml` because Claude Code is a development-time generator/reviewer, not a runtime contract. It is not part of the MVP-critical contract set.

## Acceptance verification

1. **No `TODO` in MVP-critical contracts.**
   `cargo test -p cadenza-core --lib contracts::tests::registry_text_has_no_pending_critical_keys`
   and
   `cargo test -p cadenza-cli --test contracts`
   read the actual `tools/versions.toml` and fail closed if any of the MVP-critical keys still carries a `TODO` placeholder.

2. **Correct repository URL exposed through `cargo metadata`.**
   `cargo test -p cadenza-cli --test contracts repository_metadata_uses_bytevane_url`
   shells out to `cargo metadata --no-deps --format-version 1` and asserts every workspace package reports the bytevane URL.

3. **Fresh clone can identify upstream targets.**
   `cargo test -p cadenza-cli --test contracts registry_documents_targeted_upstreams`
   asserts that `tools/versions.toml` carries entries for `symphony_spec_sha`, `cli_version`, and the file pointer to the Codex schema hash. Combined with the spec/CONTRACTS.md it lets a reader answer "which SPEC and which Codex schema does this commit target?" without grep-archaeology.

4. **Contract changes require a dedicated PR + ADR update.**
   Enforced through documentation: `CONTRACTS.md` states the rule, `decisions/0004-pinned-contract-registry.md` records the policy, and the contract-registry tests are wired into the existing `ci.yml` Rust job. CI does not auto-detect off-PR drift — that is a review-process invariant.

## Boundary tests

The detection helpers in `cadenza-core::contracts` are unit-tested at:

- **= N** — registry holds exactly the MVP-critical keys, no `TODO` → `pending_mvp_critical_keys()` empty, `missing_mvp_critical_keys()` empty.
- **= N + 1** — extra unrelated key — still no pending/missing.
- **paired-edge: any key carrying `TODO`** — single `TODO` returns single offender.
- **paired-edge: any key absent** — removing one MVP-critical key returns it from `missing_mvp_critical_keys()`.

## Non-goals (out of scope of #3)

- Generating Codex schema artifacts (owned by Issue #4).
- Adding a separate `CONTRACTS.md` content check beyond a static existence assertion.
- Replacing `tools/versions.toml` with a different format.

## References

- `README.md`, `ARCHITECTURE.md`, `REFERENCE_SOURCES.md`
- `docs/research/08-first-milestones.md`
- Existing ADRs `0001` – `0003`
