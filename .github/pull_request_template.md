<!--
Cadenza PR template. See CONTRIBUTING_AI.md for the full guardrails. Every
section below is mandatory — delete a bullet by checking it or by writing
"n/a" inline, but do not delete the headings.
-->

## Linked issue

Closes #
<!--
A PR must reference exactly one tracking issue. Branch names should match
`<type>/<short-slug>` or `issue-<n>-<slug>` — see CONTRIBUTING_AI.md
"Branch naming".
-->

## Summary

<!-- One paragraph. What changes, why now. -->

## Contract impact

Tick every box that applies. An unchecked list means "I confirm this PR
does NOT touch the area". An ADR is mandatory for any breaking-by-default
contract change (see `CONTRACTS.md`, `docs/operations/wit-abi-versioning.md`).

- [ ] Codex app-server schema or `codex.cli_version` (regen `schemas/codex/current/` + `ci/expected/codex-schema.sha256`)
- [ ] WIT ABI (`wit/runtime.wit`, `abi/expected/*.wit`)
- [ ] Secret handling, redaction, or log field surface
- [ ] Workspace path safety / containment rules
- [ ] Orchestrator state machine semantics
- [ ] Observability field names or metric labels
- [ ] Pinned dependency versions in `tools/versions.toml`

If any box is checked, link the ADR you added or amended:

- ADR:

## Tests

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `./scripts/check-wit-abi.sh` (if WIT or example plugin changed)
- [ ] `./scripts/codex-schema.sh --check` (if Codex schema or `codex.cli_version` changed)
- [ ] New behaviour has at least one failing-first test (TDD)

## AI assistance

Mandatory — fill every field or write `none` for human-authored PRs.

- Tool used: <!-- Claude Code / Codex / none -->
- Prompt template used: <!-- prompts/claude-dev.md, prompts/codex-runtime.md, or n/a -->
- Schema / WIT version consulted: <!-- e.g. codex-cli 0.133.0 + cadenza:runtime@0.2.0, or n/a -->
- Scope of the AI patch: <!-- single crate / single doc / cross-component (last requires CONTRIBUTING_AI.md justification) -->

## Reviewer checklist (for AI-generated patches)

Reviewers tick this section, not the PR author.

- [ ] Patch references the prompt template and the target issue.
- [ ] No invented protocol fields, WIT functions, or config keys.
- [ ] Tests fail before the implementation lands (verified via the diff).
- [ ] Patch stays inside the declared scope; no opportunistic refactors.
- [ ] Snapshots regenerated where claimed (`abi/expected/`, `schemas/codex/current/`).
- [ ] Pinned dependency versions in `tools/versions.toml` were not edited inline by the tool.
