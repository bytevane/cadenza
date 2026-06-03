# Deviations ledger

The gap/progress half of cadenza's governance, paired with `decisions/` (which
records direction). Every confirmed or accepted divergence from a frozen contract
(SPEC.md, `wit/runtime.wit`, the generated Codex schema, `tools/versions.toml`)
gets one row here. See `decisions/0010-deviation-ledger.md` for why this exists,
and `CONTRIBUTING_AI.md` → "Anti-over-design principles" for what counts as a
deviation.

## Three rules

1. **IDs are never reused.** `D1`, `D2`, … monotonically. A removed deviation
   keeps its ID and its row.
2. **Rows are never deleted.** A `Closed` or `Reverted` row stays visible so
   future audits can see what was resolved and how.
3. **Severity reflects the contract gap, not implementation effort.** A one-line
   fix can be P0; a large refactor can be Low.

## Status vocabulary

| Status | Meaning |
|---|---|
| `Open` | Confirmed deviation, not yet addressed. |
| `Reverting` | Behaviour still ships; removal is planned. |
| `Reverted` | The over-design was deleted. |
| `Closed` | Resolved (aligned to contract). |
| `Closed (accepted deviation)` | A live divergence accepted on purpose; rationale stays visible and **must** be backed by an ADR under `decisions/`. |

## Contract reference anchors

Prefer locally verifiable anchors, in this order:

1. `wit/runtime.wit` function signature.
2. Generated Codex schema field.
3. Symphony SPEC.md § + `symphony_spec_sha` (recorded as `symphony_spec_sha@§path`;
   cadenza does not vendor SPEC.md, so the reference is checked against the pinned
   commit in `tools/versions.toml`).

## Ledger

| ID | Area | Contract reference | Severity | Status | Tracking |
|----|------|--------------------|----------|--------|----------|
| _none yet_ | | | | | |

<!--
Row template (copy, fill, assign the next unused D-number):
| D1 | <what deviates + short fix narrative> | <wit sig / schema field / SPEC.md §> | P0 / P1 / High / Medium / Low | Open / Reverting / Reverted / Closed / Closed (accepted deviation) | #issue / #pr |
-->
