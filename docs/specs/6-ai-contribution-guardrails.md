# Spec: Issue #6 — Add AI contribution workflow guardrails

Tracks https://github.com/bytevane/cadenza/issues/6 (Milestone: MVP 0 - Contracts & Guardrails).

## Outcome

`.github/pull_request_template.md` and `CONTRIBUTING_AI.md` together
codify what an AI-generated PR must contain, how it must be scoped, and
what a reviewer must verify before merge. The checklists are enforced
through the PR body (template) and a `cargo test -p cadenza-cli --test
ai_contribution_guardrails` gate that fails if either file loses a
required marker.

## Changes

1. **`.github/pull_request_template.md`** — adds:
   - `## Linked issue` with mandatory `Closes #` reference.
   - Expanded `## Contract impact` covering Codex schema, WIT ABI,
     secrets/logs, workspace, orchestrator, observability, and pinned
     dependency versions; each box requires a matching ADR link when
     ticked.
   - `## Tests` extended with `scripts/codex-schema.sh --check`,
     `scripts/check-wit-abi.sh`, and TDD-first assertion.
   - `## AI assistance` now mandates tool/prompt/scope fields.
   - New `## Reviewer checklist (for AI-generated patches)` that
     reviewers (not authors) tick.

2. **`CONTRIBUTING_AI.md`** — adds:
   - "Patch scope" section discouraging cross-component AI patches and
     opportunistic refactors.
   - "Branch naming" table covering `issue-<n>-<slug>`, `feat/`,
     `fix/`, `infra/`, `sec/`, `docs/`, `chore/`.
   - Expanded required-context list including `CONTRACTS.md` and
     `docs/operations/wit-abi-versioning.md`.
   - "When the AI tool is wrong" section — practical rules for pushing
     back on hallucinated API surface and scope creep.

3. **`crates/cadenza-cli/tests/ai_contribution_guardrails.rs`** — five
   integration tests that read the two files and assert the presence of
   the required markers. These run inside the normal `cargo test
   --workspace` step in CI.

## Acceptance verification

| Acceptance criterion (from #6) | Verification |
| ------------------------------ | ------------ |
| PRs have mandatory checkboxes for schema / WIT / security / log changes. | Template `## Contract impact` section; `pr_template_requires_linked_issue_and_contract_impact` test. |
| AI-generated patches must reference the prompt template and target issue. | Template `## Linked issue` (`Closes #N`) + `## AI assistance` (`Prompt template used`, `Scope of the AI patch`); `pr_template_requires_ai_assistance_metadata` test. |
| Documentation discourages broad cross-component AI patches. | `CONTRIBUTING_AI.md` "Patch scope" section; `contributing_ai_codifies_branch_naming_and_patch_scope` test asserts the `Cross-component patches` and `One PR = one issue` markers. |

## Boundary tests

- `pr_template_requires_linked_issue_and_contract_impact` — TDD edge: missing any of the 7 contract-impact markers fails.
- `pr_template_requires_ai_assistance_metadata` — paired edge to the contract-impact test.
- `pr_template_carries_reviewer_checklist_for_ai_patches` — separates author duties from reviewer duties.
- `contributing_ai_codifies_branch_naming_and_patch_scope` — covers the branch-naming table and patch-scope rules.
- `contributing_ai_references_current_contract_docs` — keeps the required-context list in sync with the contract surface.

## Non-goals

- No automation that calls Claude Code or Codex in CI.
- No GitHub Actions enforcement of the checklist — that is enforced socially via the reviewer checklist.

## References

- `CONTRACTS.md`
- `docs/VERSIONING.md`, `docs/WIT_ABI.md`, `docs/operations/wit-abi-versioning.md`
- `prompts/claude-dev.md`, `prompts/codex-runtime.md`
