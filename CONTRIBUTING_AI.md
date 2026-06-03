# AI-assisted contribution policy

Cadenza allows Claude Code and Codex to generate implementation patches,
but the project is contract-first. Every AI-authored PR must satisfy the
checks below before a human reviewer marks it ready to merge.

## Required context for every AI coding task

Every generated patch must reference:

- `ARCHITECTURE.md`
- `CONTRACTS.md`
- `docs/VERSIONING.md`
- `docs/WIT_ABI.md`
- `docs/operations/wit-abi-versioning.md`
- `SECURITY.md`
- the relevant crate README or module docs

The PR description must list which of these the patch was conditioned
on; reviewers verify that the patch never silently invents API surface
absent from those documents.

## Tool split

- **Codex**: protocol-adjacent code, app-server client code, replay
  fixtures, schema-bound tests.
- **Claude Code**: scaffolding, Rust module implementation, docs, CI
  templates, refactoring, test data.

Cross-component patches that touch both protocol code AND extension/Wasm
code in the same PR are discouraged. Land each contract surface as its
own PR with its own ADR (see "Patch scope" below).

## Patch scope

AI tools tend to "fix everything they notice." Keep them on a leash:

- One PR = one issue. The PR template requires `Closes #N`.
- Prefer a single crate or a single doc surface per PR. Reach across
  crates only when the issue itself spans crates (e.g. a contract bump
  whose downstream consumers live in two crates).
- Do not bundle a refactor with a bug fix; ship the fix first so it can
  be reverted cleanly if the refactor regresses something.
- Do not opportunistically rename `_var` locals, re-export types, or
  delete comments while implementing an unrelated feature.

When you ask an AI tool for a patch, restrict it to the files relevant
to the issue and surface the constraint in the prompt (e.g. "modify
only `crates/cadenza-codex/` and `crates/cadenza-codex/tests/`").

## Anti-over-design principles

Earned by the aiops-platform port (a sibling Symphony runtime), which spent a
dozen `remove/drop` PRs unwinding gates, caps, and config the AI added that the
contract never asked for. These principles exist to keep cadenza from repeating
that rework.

- **Contract absence is an over-design signal.** Before adding any
  orchestrator/host-side stage, gate, artifact, or config that acts on agent
  output, confirm the behaviour is actually permitted there by `wit/runtime.wit`,
  the generated Codex schema, or SPEC.md. If the contract has no equivalent, that is
  a strong signal the component is over-design, not a feature gap to fill.
  **Delete it — do not relocate it (move-to-prompt) or merely document it (a
  `DEVIATIONS.md` row).** Relocating or documenting preserves scaffolding that no
  longer earns its place.

- **Research to a verdict before proposing; bring the verdict, not a menu.** When
  SPEC.md + WIT + reference research settles whether a component belongs, decide and
  act on it. Do not hand a keep / relocate / document multiple-choice back to the
  reviewer — that menu is usually a symptom that the research which would rule out
  "keep" was not finished. Reserve genuine choices for scope, intent, or safety
  forks the contract leaves open.

- **Unbounded semantics: no *new* caps without an ADR.** Symphony SPEC.md retries
  are unbounded with backoff (no give-up branch). The orchestrator's existing
  lifecycle policy (`max_retries`/`GiveUp`, #19) already diverges from that — it is
  tracked as a deviation (`DEVIATIONS.md` D1) pending a SPEC ruling, not a
  precedent to extend. Do not add further terminal states, retry caps, or
  continuation caps; any new cap requires an ADR citing the contract basis. Don't
  "make it safer" by inventing a ceiling.

- **Earn your rules.** A new discipline rule should trace to a specific observed
  failure. Annotate it `Earned by: #PR (symptom)` so it can be audited and
  removed when a future model no longer needs it. When in doubt, leave a rule out
  until a failure demands it.

## Branch naming

| Branch prefix      | Use when …                                                  |
| ------------------ | ----------------------------------------------------------- |
| `issue-<n>-<slug>` | An issue tracks the work; the slug repeats the issue title. |
| `feat/<slug>`      | Net-new feature without a tracking issue (rare).            |
| `fix/<slug>`       | Bug fix without a tracking issue.                           |
| `infra/<slug>`     | CI, tooling, dependency, repo-infrastructure changes.       |
| `sec/<slug>`       | Security-sensitive changes (secret handling, sandbox, ABI). |
| `docs/<slug>`      | Documentation-only changes.                                 |
| `chore/<slug>`     | Mechanical cleanup (formatting, version bumps).             |

Slugs are short kebab-case (`feat/workflow-loader`,
`infra/codex-schema-gate`, `sec/secret-scrubber`). Do not put bug
identifiers in the branch name unless the issue tracker uses them.

## PR checklist for AI patches

Every AI-authored PR ticks the matching boxes in
`.github/pull_request_template.md`. The author confirms the **Contract
impact**, **Tests**, and **AI assistance** sections; the reviewer
confirms the **Reviewer checklist** section.

If the patch touches any of the contract surfaces below, the PR must
add or amend an ADR under `decisions/`:

- Codex app-server schema bump.
- WIT ABI changes (source or plugin component world).
- Secret-handling, redaction, or log-field changes.
- Workspace path safety rules.
- Orchestrator state machine semantics.

## Prompt discipline

- Use `prompts/codex-runtime.md` for Codex app-server work.
- Use `prompts/claude-dev.md` for general implementation work.
- Do not ask either tool to invent protocol fields not present in
  generated schemas.
- Do not ask either tool to skip tests via `#[ignore]`, `--no-verify`,
  or `if false` guards in CI.
- The TDD discipline applies to AI-generated code: a failing test
  before the implementation, paired-edge boundary cases, no `cap/2`
  midpoint coverage as a stand-in for boundary tests.

## When the AI tool is wrong

- Push back on tool output that invents API surface, expands scope, or
  silently changes pinned versions.
- If the tool keeps drifting, narrow the prompt or split the work into
  smaller patches. Do not merge a "mostly-right" patch and intend to
  follow up.
- Record recurring mistakes in `docs/research/` or in a dedicated ADR
  so future prompts avoid them.
- When you confirm a deviation from a frozen contract, log it as a row in
  `DEVIATIONS.md` (do not silently make the discrepancy disappear); an
  accepted deviation also needs an ADR under `decisions/`.
