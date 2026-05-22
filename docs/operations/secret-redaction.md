# Secret redaction policy

Cadenza scrubs secret-shaped values from every observable surface. This
document records **what the scrubber does and does not guarantee** so an
operator can reason about residual risk.

## Surfaces with redaction wired in

| Surface | Mechanism | Where |
| --- | --- | --- |
| `tracing` log fields | Field-key detection (`looks_secret`) at log call sites that use `cadenza_obs::FIELD_*` constants | `cadenza-obs::fields` + log macros |
| Hook stdout/stderr | Exact-substring replacement against secrets registered via `HookRunner::with_secrets` | `cadenza-workspace::hooks::HookRunner` (#11) |
| Codex stderr capture | Same as hook runner | `cadenza-codex::AppServerLauncher::with_secrets` (#12) |
| Observability state API | `redact_snapshot` walks every free-form field in the snapshot and applies the KEY=VALUE scrubber | `cadenza-obs::server` (#20) |
| HTTP response bodies | `redact_snapshot` runs before every response | `cadenza-obs::server::get_state` (#20) |
| Wasm host function `log` | TBD when host capabilities land in #16; will use `cadenza_obs::Scrubber` | future |

## Detection rules

A field key is considered secret-shaped (`looks_secret`) when its
lowercase form:

- contains `token` (covers `*_TOKEN`, `bearer_token`, etc.)
- contains `secret`
- contains `password`
- contains `authorization` (covers `AUTHORIZATION` headers)
- contains `cookie`
- ends with `_key` (covers `LINEAR_API_KEY`, `OPENAI_API_KEY`)

A free-form text is scrubbed by:

1. Replacing every occurrence of every registered secret VALUE
   (longest-first) with `***REDACTED***`. Registered values come from
   the workflow (`tracker.token`, etc.).
2. Replacing every `KEY=VALUE` substring where `KEY` matches the
   detection rules with `KEY=[REDACTED]`. Only the `=` separator is
   honoured; see "What this does not guarantee" below.

## What this does guarantee

- A value registered via `HookRunner::with_secrets` or
  `AppServerLauncher::with_secrets` cannot appear verbatim in the
  captured stdout/stderr that those runners return to the orchestrator.
- A field logged through a `cadenza_obs::FIELD_*` constant whose key
  matches the detection rules is replaced with `[REDACTED]` before the
  log line is emitted.
- The observability state API never returns a snapshot field that
  matches the KEY=VALUE pattern with an exposed value.
- The scrubber preserves enough context (`KEY=[REDACTED]` /
  `***REDACTED***`) for an operator to know **what** was redacted, even
  though they cannot see the value.

## What this does not guarantee

- **`KEY: value` patterns** (colon separator) are intentionally not
  scrubbed by the free-text path. The `authorization: bearer <TOKEN>`
  case is the obvious example — the token is two words past the
  separator and a naive scrubber would only catch `bearer`. Structured
  callers that need this should pass the key/value pair through
  `Scrubber::redact_key_value` instead.
- Secret VALUES that are NOT registered with the scrubber pass through
  free-form text. The orchestrator is responsible for registering the
  workflow's `tracker.token` (and any future secret-shaped config
  fields) before invoking downstream components.
- Bytes captured BEFORE the scrubber sees them (e.g., a debugger
  attached to the host process, a core dump, a kernel-level network
  capture) are out of scope.
- Out-of-band logs written by child processes to their own files are
  out of scope. Cadenza only sees what arrives via the captured stdio
  pipes.
- Memory inspection of the host process (e.g., `gcore`) is out of
  scope.

## Adding a new logged surface

When adding a new logging or capture site:

1. Prefer field-keyed logging using `cadenza_obs::FIELD_*` constants.
   The structured-field scrubber will catch secret-shaped keys
   automatically.
2. For free-form text (subprocess output, error messages from external
   libraries), pass the buffer through `cadenza_obs::Scrubber::scrub_text`
   before logging or returning it.
3. For per-issue runs, build a `Scrubber::with_secrets` carrying the
   workflow's registered tokens before kicking off subprocesses or
   plugin calls.

## Adding a new detection rule

Extend `cadenza_obs::looks_secret`. The change is `type:security` per
`CONTRIBUTING_AI.md`, so it requires an ADR explaining what new
threat is covered. Existing regression tests in
`cadenza-obs::scrubber::tests` and `cadenza-obs::snapshot::tests`
should be extended to lock the new key shape in.

## Out of scope

External secret-manager integration (Vault, AWS Secrets Manager, etc.)
is intentionally not in scope of #21 — the scrubber's job is to keep
already-loaded secrets out of observable output.
