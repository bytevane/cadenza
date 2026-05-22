# Spec: Issue #8 — Implement strict prompt renderer

Tracks https://github.com/bytevane/cadenza/issues/8 (Milestone: MVP 1 - Workflow & Workspace).

## Outcome

`cadenza-workflow::render_prompt` renders an issue prompt from a validated
workflow in strict mode. Inputs are typed (`PromptInput { issue, attempt }`)
so a caller cannot forget to provide the two required surfaces. Failure
modes are split into a dedicated `PromptRenderError` enum so the
orchestrator can route compile / undefined / unknown-item / other errors
without conflating them with workflow parsing (`WorkflowError`).

## Public surface

```rust
pub struct PromptInput<'a> {
    pub issue: &'a cadenza_core::Issue,
    pub attempt: u32,
}

pub fn render_prompt(
    template: &str,
    input: &PromptInput<'_>,
) -> Result<String, PromptRenderError>;

pub enum PromptRenderError {
    Compile { message: String },
    UndefinedVariable { message: String },
    UnknownItem { message: String },
    Other { message: String },
}
```

`render_prompt_strict(template, json_context) -> minijinja::Error` is
removed; it was internal-only (no out-of-crate callers).

The renderer hard-codes `UndefinedBehavior::Strict`. Pushing the toggle
into the consumer (with a `&PromptConfig` parameter) would create a dead
branch — `parse_workflow` already rejects `prompt.strict_undefined: false`
at the workflow boundary, so any value reaching the renderer is strict by
construction. Defaults stay at the parse boundary (project drop-fallback
rule), the renderer stays single-purpose.

Minijinja's default filter / test / function table is left unmodified —
no custom filters are exposed. Unknown filters, tests, functions, and
methods all classify as `UnknownItem` so a typo can never silently render
as missing.

## Error mapping

| `minijinja::ErrorKind`                                                  | `PromptRenderError`     |
| ----------------------------------------------------------------------- | ----------------------- |
| `UndefinedError`                                                        | `UndefinedVariable`     |
| `UnknownFilter` / `UnknownTest` / `UnknownFunction` / `UnknownMethod`   | `UnknownItem`           |
| `SyntaxError` / `TemplateNotFound` / `BadEscape` / `InvalidOperation` (at compile time) | `Compile`     |
| everything else                                                         | `Other`                 |

## Acceptance verification

| Acceptance criterion (from #8)                              | Verification |
| ----------------------------------------------------------- | ------------ |
| Unknown variable test fails deterministically.              | `undefined_variable_classifies_as_undefined_variable`. |
| Unknown filter test fails deterministically.                | `unknown_filter_classifies_as_unknown_item` (`{{ x \| nonexistent_filter }}`); `unknown_function_classifies_as_unknown_item`; `unknown_test_classifies_as_unknown_item`. |
| Valid issue/attempt input renders a stable prompt.          | `renders_issue_and_attempt_into_prompt`; `snapshot_matches_full_workflow_example_template` runs the real `WORKFLOW.example.md` body and pins the rendered output verbatim. |
| The renderer can be called from unit tests without external services. | The whole suite is `cargo test -p cadenza-workflow` — no I/O, no subprocesses. |

## Boundary tests (per project rule)

- `attempt = 0` paired with `attempt = u32::MAX` — both render without error so a valid run number is never rejected.
- Snapshot coverage for the full template (`snapshot_matches_full_workflow_example_template`) and for the `description: None` branch (`snapshot_handles_missing_optional_description`).

## Disjoint error types

`PromptRenderError` and `WorkflowError` deliberately do not implement
`From` for each other. The separation is enforced by the source — no
`#[from]` annotation, no `impl From<…>` block, no transitive
`anyhow::Error` flattening at the boundary. A compile-time assertion was
considered but added complexity (custom trait helpers or a
`static_assertions` dep) without changing reviewer behaviour, so it was
dropped per project YAGNI rule.

## Out of scope

- Custom prompt filters (e.g. tracker-specific markdown sanitisation).
- Non-strict rendering (`prompt.strict_undefined: false` still rejected at parse time pending follow-up issue).
- Tracker integration; Codex app-server launch.

## References

- `crates/cadenza-workflow/src/lib.rs`
- `WORKFLOW.example.md`
- Closes #7 (PR #29) — provides the typed `PromptConfig` consumed here.
