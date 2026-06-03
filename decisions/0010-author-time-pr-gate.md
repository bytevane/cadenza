# ADR 0010: Author-time PR gate for contract impact

## Status

Accepted.

## Context

cadenza's PR template already asks which contract surfaces a change touches, but
the answer is human-checked. The sibling aiops-platform port showed that rules
which are *audit-time judgement* rather than *author-time mechanics* get bypassed:
contract deviations shipped despite the rules existing. cadenza needs the same
"box must be checked + ADR present" discipline enforced mechanically before merge.

## Decision

Add a `cadenza pr-gate` subcommand (a pure `evaluate` over changed files + PR body,
plus an IO driver) and a `pr-metadata.yml` workflow that runs it on every PR.

- Hard paths (`wit/`, `abi/expected/`, `schemas/codex/`, `tools/versions.toml`
  MVP-critical value changes, and the gate's own code) must check the matching
  Contract-impact box and include an ADR, else the gate exits non-zero.
- Soft paths (behaviour crates: orchestrator, workspace, obs) must either check
  the box or carry an explicit `no <area> semantics change` declaration — a forced
  statement, not a machine judgement of semantics.
- An accepted deviation may instead land as a `DEVIATIONS.md` row plus an ADR.
- The gate runs on `pull_request` (not `pull_request_target`) with `fetch-depth: 0`
  and fails closed if the base diff is unavailable.

The gate is a CI tool, not a contract surface, so it lives as a `cadenza-cli`
subcommand rather than a new crate (CONTRIBUTING_AI.md "Anti-over-design
principles", principle 6).

## Rationale

Mechanical author-time enforcement turns "I forgot to flag the contract change"
from a reviewer catch into a red check. Path-level classification (no AST) keeps
it simple and testable; hard paths are zero-false-positive contract files, while
behaviour crates fall back to a forced declaration to avoid false-positive
speed-bumps.

## Consequences

- A change to a hard contract path cannot merge while claiming "no impact".
- The `pr-gate` job must be made a required status check by a repo admin (use the
  exact job name `pr-gate`); see the spec's deployment prerequisites.
- The gate's own code is a hard path, so weakening it requires an ADR.
- If the repo later accepts fork PRs or enables a merge queue, the trigger model
  must be revisited (`merge_group`).
