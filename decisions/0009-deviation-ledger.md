# ADR 0009: Deviation ledger for frozen-contract gaps

## Status

Accepted.

## Context

cadenza is contract-first: SPEC.md, `wit/runtime.wit`, the generated Codex schema,
and `tools/versions.toml` are frozen contracts. `decisions/` records *direction*
(why a contract is shaped the way it is), but there is no single place that
tracks *gaps* — confirmed or accepted divergences from those contracts and their
resolution status. The sibling aiops-platform port demonstrated the cost of that
omission: deviations accumulated invisibly until a batch audit, then took a dozen
`remove/drop` PRs to unwind.

## Decision

Introduce `DEVIATIONS.md`, a single-table ledger that is the gap/progress half of
governance, complementing `decisions/`:

- `decisions/` (ADRs) = direction and rationale.
- `DEVIATIONS.md` = per-deviation gap tracking: ID, area, contract reference,
  severity, status, tracking issue/PR.

Three rules govern it: IDs are never reused, rows are never deleted, and severity
reflects the contract gap rather than implementation effort. A
`Closed (accepted deviation)` row must be backed by an ADR.

## Rationale

Turning silent drift into a visible, append-only ledger lets audits see what was
resolved and stops over-design from reappearing under a new name. Keeping the
ledger separate from `decisions/` preserves the direction-vs-progress distinction
that a single ADR list blurs. Severity is decoupled from effort so a hard-to-fix
deviation cannot be quietly down-graded.

## Consequences

- Any change that violates a frozen contract must either close an existing row,
  add a new tracked row, or be reverted — it cannot silently disappear.
- `Closed (accepted deviation)` rows pair with an ADR.
- The author-time PR gate (a later batch) will treat a `DEVIATIONS.md` row as one
  compliant way to land a contract-touching change.
- The ledger starts empty; it is a prevention framework, not a backlog.
