# Release notes — `vX.Y.Z`

> **Template.** Copy this file into `docs/releases/vX.Y.Z.md`, fill in
> every section, and update `docs/operations/compatibility-matrix.md`
> in the SAME PR. Do not delete unfilled headers — write
> "no changes" instead so the absence is explicit.

**Release date:** `YYYY-MM-DD`
**Cadenza commit:** `<full SHA>`
**Cadenza tag:** `vX.Y.Z`

## Contract versions

These MUST match `tools/versions.toml` at the tagged commit. Diff
against the previous release to surface contract movement.

| Surface | This release | Previous release |
| --- | --- | --- |
| Symphony SPEC commit |   |   |
| Rust toolchain |   |   |
| Workspace MSRV |   |   |
| Codex CLI / app-server |   |   |
| Codex schema hash |   |   |
| WIT package |   |   |
| Wasmtime |   |   |
| wasm-tools |   |   |
| wit-bindgen |   |   |

## Highlights

Two or three sentences. What is the most operator-visible change?
What new capability does this unlock, or what risk does it close?

## Changes by component

### Orchestrator
- _no changes_ / bullet list

### Workspace
- _no changes_ / bullet list

### Codex client
- _no changes_ / bullet list

### Linear tracker
- _no changes_ / bullet list

### Wasm host
- _no changes_ / bullet list

### Observability
- _no changes_ / bullet list

### CLI
- _no changes_ / bullet list

## Compatibility notes

- **Breaking?** Yes / No. If yes, link the ADR.
- **Workflow YAML changes:** _none_ / bullet list.
- **WIT package bump:** _none_ / bullet list.
- **Codex schema bump:** _none_ / bullet list.

## Known limitations carried over

Copy any unresolved blocker / known-regression lines from the prior
release. Add new ones explicitly.

- Wasm host capability set (#16) and `host-linear` (#17) are still
  blocked until `wit-bindgen` is pinned and the example plugin is
  rewritten. The MVP smoke does not exercise these surfaces.
- Real production deployment, multi-node scheduler, and durable
  orchestrator persistence remain out of scope.

## Upgrade steps

```bash
git fetch --tags
git checkout vX.Y.Z
./scripts/bootstrap-dev.sh
cargo build --release
./scripts/check-wit-abi.sh
./scripts/codex-schema.sh --check
./scripts/mvp-smoke.sh
```

For each pinned contract bump in the table above, follow the matching
"How to bump a row" section in `docs/operations/compatibility-matrix.md`.

## Rollback

See `docs/operations/rollback-drill.md`. Critical preservations:

- Per-issue workspaces under `<workspace.root>/`.
- Structured logs.

The orchestrator reconciles from tracker + workspace on boot; no
durable persistence is involved.

## Acknowledgements

Optional. PR authors, agents (Claude Code, Codex), reviewers.
