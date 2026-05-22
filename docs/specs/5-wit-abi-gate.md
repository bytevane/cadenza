# Spec: Issue #5 — Establish WIT ABI gate and first component validation

Tracks https://github.com/bytevane/cadenza/issues/5 (Milestone: MVP 0 - Contracts & Guardrails).

## Outcome

Cadenza catches accidental WIT ABI drift on every PR by enforcing two
snapshots under `abi/expected/`. The first protects the source package WIT
in `wit/runtime.wit`; the second protects the world WIT extracted from the
built example component plugin. Snapshot bumps are an intentional ABI
change subject to the pre-1.0 rule documented in
`docs/operations/wit-abi-versioning.md`.

## Approach

1. **`scripts/check-wit-abi.sh` rewrite.** The previous script only diffed
   the source-level snapshot. The new script:
   - Pre-flight checks `wasm-tools`, `cargo`, `diff`.
   - Round-trips `wit/` through `wasm-tools component wit -t` / `--json` so
     the package decodes cleanly.
   - Source-level diff against `abi/expected/runtime.wit` with an explicit
     "WIT mismatch" message plus the regenerate command.
   - Builds the example plugin for `wasm32-wasip2`, validates it is a
     component via `wasm-tools validate`, extracts its world WIT, and
     diffs against `abi/expected/cadenza-linear-graphql-plugin.world.wit`.
2. **Commit the component world snapshot.** Today the plugin is a Rust
   placeholder, so the extracted world is `package root:component; world root {}`.
   That is the snapshot we commit until #16/#17 give the plugin real
   imports/exports — at which point the snapshot bump becomes part of
   that PR.
3. **Pre-1.0 versioning doc.** `docs/operations/wit-abi-versioning.md`
   codifies the rule that minor bumps are breaking and require an ADR and
   PR callout. Errors emitted by the script point at this doc.
4. **Acceptance regression tests.** `cargo test -p cadenza-cli --test wit_abi`:
   - asserts `wit/runtime.wit` is byte-identical to
     `abi/expected/runtime.wit` (TDD-style include_str! comparison);
   - shells out under `env -i PATH=/usr/bin:/bin` and confirms the script
     fails closed with a stderr message naming `wasm-tools is required`.

## Acceptance verification

| Acceptance criterion (from #5) | Verification |
| ------------------------------ | ------------ |
| CI fails on unexpected WIT/ABI drift. | `ci/rust` job runs `./scripts/check-wit-abi.sh` which `diff -u`s both snapshots and exits non-zero on mismatch. |
| The example plugin builds as a component. | Script runs `wasm-tools validate` on the built `.wasm`, which fails closed if the artifact is not a component. |
| ABI changes require an ADR or explicit PR checklist approval. | `docs/operations/wit-abi-versioning.md` codifies the pre-1.0 rule; error messages reference it; CONTRACTS.md links it as part of the contract surface. |
| Error messages clearly identify WIT mismatch. | Each diff branch prints `WIT mismatch: <which> drifted from <path>` followed by the regen command and the unified diff. |

## Boundary tests

- `runtime_wit_matches_abi_snapshot` — TDD edge: byte-level equality. Fails if a single byte drifts.
- `check_wit_abi_script_fails_closed_when_wasm_tools_missing` — paired-edge: no `wasm-tools` on PATH → exit 1 with the stderr hint.

## Out of scope

- Real plugin world implementation (#16, #17).
- Pre-1.0 → 1.0 migration tooling (would land alongside the first `cadenza:runtime@1.0.0` PR).
- Cosign verification of `wasm-tools` distribution.

## References

- `CONTRACTS.md`
- `tools/versions.toml`
- `decisions/0003-native-wasm32-wasip2.md`
- `wit/runtime.wit`
