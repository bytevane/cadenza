# Spec: Issue #7 — Implement WORKFLOW.md parser and typed config model

Tracks https://github.com/bytevane/cadenza/issues/7 (Milestone: MVP 1 - Workflow & Workspace).

## Outcome

`cadenza-workflow::parse_workflow` returns a fully-typed
`WorkflowDefinition` with a strict, `deny_unknown_fields` config tree and
field-level validation diagnostics. `cargo run -p cadenza-cli -- doctor
--workflow WORKFLOW.example.md` validates the shipped example and prints
the typed config (defaults applied) so reviewers can inspect what the
parser actually read.

## Typed config tree

```
WorkflowConfig
├─ tracker: TrackerConfig          // kind, optional project_slug_id, token
├─ poll: PollConfig                // interval_ms (default 5000)
├─ workspace: WorkspaceConfig      // root (absolute path)
├─ codex: CodexConfig              // command, optional schema_sha256, turn_timeout_ms (default 600_000)
├─ orchestrator: OrchestratorConfig // max_concurrent_agents (default 1), active_states, terminal_states
├─ hooks: HooksConfig              // optional after_create: HookCommand
└─ prompt: PromptConfig            // strict_undefined (default true)
```

All sub-structs derive `serde(deny_unknown_fields)`. Defaults live in
`Default` impls and `#[serde(default = "…")]` constructors, so callers
never plug in defaults at the consumption site (per project rule).

`TrackerKind` is an enum (only `linear` today); adding a tracker
backend requires extending the enum and an ADR.

## Validation rules (file/field context)

| Field                                  | Rule                                              |
| -------------------------------------- | ------------------------------------------------- |
| `tracker.token`                        | non-empty after trim                              |
| `workspace.root`                       | non-empty, absolute path                          |
| `codex.command`                        | non-empty after trim                              |
| `codex.turn_timeout_ms`                | > 0                                               |
| `poll.interval_ms`                     | > 0                                               |
| `orchestrator.max_concurrent_agents`   | > 0                                               |
| `orchestrator.active_states`           | non-empty                                         |
| `orchestrator.terminal_states`         | non-empty, disjoint from `active_states`          |
| `hooks.after_create.command`           | non-empty after trim (when present)               |
| `hooks.after_create.timeout_ms`        | > 0 (when present)                                |

Errors carry the dotted field path and a short message so operators can
find the right line in `WORKFLOW.md` without grepping source.

## Boundary tests (per project rule)

For every numeric "must be > 0" field — `codex.turn_timeout_ms`,
`poll.interval_ms`, `orchestrator.max_concurrent_agents`,
`hooks.after_create.timeout_ms` — there is a paired test:

- `*_zero_*_is_invalid_boundary` (= 0 fails)
- `*_one_*_is_valid_boundary`    (= 1 succeeds)

For every list "must not be empty" field, there is a paired test:

- empty → invalid
- single-element → valid

This gates against both "cap/2 midpoint" mis-coverage and missing-edge
mistakes.

## Acceptance verification

| Acceptance criterion (from #7)                                       | Verification |
| -------------------------------------------------------------------- | ------------ |
| `cargo run -p cadenza-cli -- doctor --workflow WORKFLOW.example.md` validates. | `parses_workflow_example_md` test parses the shipped example via `include_str!`. Doctor command unchanged in behaviour — it still calls `parse_workflow`. |
| Invalid YAML front matter fails with useful diagnostics.             | `WorkflowError::InvalidYaml` carries the `serde_yaml::Error` message; `invalid_yaml` and `unknown_top_level_field_is_rejected` tests assert. |
| Missing required sections fail fast.                                 | `serde(deny_unknown_fields)` plus required fields (no `#[serde(default)]`) means missing required sections return `InvalidYaml` with a clear path. |
| Tests cover minimal valid workflow and at least 5 invalid workflows. | 24 tests total — 4 "valid" (minimal, example, single-state, paired-edge `= 1` cases) and 20+ "invalid" boundary/validation tests. |

## Non-goals

- Hot reload (#9).
- Strict prompt rendering UX (#8).
- Workspace path containment (#10).
- Hook execution boundary (#11).

## References

- `WORKFLOW.example.md`
- `crates/cadenza-workflow/src/lib.rs`
- `decisions/0001-rust-host-wasm-extensions.md`
