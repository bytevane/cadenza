# Spec: Issue #11 — shell hook execution boundary

Tracks https://github.com/bytevane/cadenza/issues/11 (Milestone: MVP 1 - Workflow & Workspace).

## Outcome

Cadenza ships a trusted-shell hook runner that honours the four lifecycle
phases workflows can opt into, runs each hook with `cwd` pinned to the
per-issue workspace, kills the process on timeout, caps stdout/stderr
capture, and redacts known secret values before returning. Orchestrator
wiring (which phase aborts dispatch on failure) lands later — this PR
provides the primitive and the per-phase policy hint.

## Public surface

`cadenza-workflow`:

```rust
pub struct HooksConfig {
    pub after_create: Option<HookCommand>,
    pub before_run: Option<HookCommand>,
    pub after_run: Option<HookCommand>,
    pub before_remove: Option<HookCommand>,
}

pub enum HookPhase { AfterCreate, BeforeRun, AfterRun, BeforeRemove }
impl HookPhase {
    pub const ALL: [HookPhase; 4];
    pub fn as_str(self) -> &'static str;
    pub fn is_fatal_by_default(self) -> bool;
}

impl HooksConfig {
    pub fn get(&self, phase: HookPhase) -> Option<&HookCommand>;
}
```

`cadenza-workspace::hooks`:

```rust
pub struct HookRunner { /* … */ }
impl HookRunner {
    pub fn new(workspace: impl Into<PathBuf>) -> Self;
    pub fn with_secrets(self, secrets: Vec<String>) -> Self;
    pub fn with_capture_bytes(self, bytes: usize) -> Self;
    pub fn run(&self, hook: &HookCommand) -> Result<HookOutcome, HookLaunchError>;
}

pub enum HookOutcome { Success { stdout, stderr }, Failed { exit, stdout, stderr }, TimedOut { stdout, stderr } }
pub enum HookLaunchError { WorkspaceMissing(PathBuf), Spawn(io::Error), Capture(io::Error) }
```

## Runner contract

- **Shell**: `sh -c <command>` so workflows like `command: "git init"` work. Trusted-hook model relies on workspace containment + timeout + redaction, not on argv splitting.
- **cwd**: always the per-issue workspace. Verified by `cwd_is_the_issue_workspace` (`pwd` + canonicalise to dodge macOS `/var` vs `/private/var`). `WorkspaceMissing` returned early if the dir does not exist.
- **Timeout**: poll-and-kill loop with 20 ms granularity. On expiry the child is killed and the outcome is `TimedOut`. `timeout_kills_long_running_process` asserts the runner returns within 2 s for `sleep 5` + `timeout_ms = 200`.
- **Output capture**: per-stream cap (`DEFAULT_CAPTURE_BYTES = 64 KiB`, overridable). Truncated output is suffixed with `<truncated; output exceeded capture cap>` so the operator sees that truncation happened.
- **Secret redaction**: `with_secrets(values)` filters empty strings (so an empty needle cannot expand the output) and replaces every occurrence of each remaining value with `***REDACTED***`.
- **Typed outcomes**: launch problems (`WorkspaceMissing`, `Spawn`, `Capture`) come back as `Err(HookLaunchError)`; process-level results (`Success`, `Failed`, `TimedOut`) are `Ok(HookOutcome)` so callers cannot conflate "child exit 1" with "could not even start the child".

## Per-phase policy

`HookPhase::is_fatal_by_default` returns `true` for `AfterCreate` and `BeforeRun` (preconditions) and `false` for `AfterRun` and `BeforeRemove` (post-run cleanup). The orchestrator consults this when it lands in #18 — keeping the policy next to the phase enum prevents the orchestrator from reinventing it.

## Acceptance verification

| Acceptance criterion (from #11) | Verification |
| --- | --- |
| Hook cwd is always the per-issue workspace. | `cwd_is_the_issue_workspace`. |
| Hook timeout kills the process. | `timeout_kills_long_running_process`. |
| Fatal hook failure prevents dispatch where required. | `HookPhase::is_fatal_by_default` + `fatal_phases_match_documentation`; orchestrator wiring is #18. |
| Non-fatal hook failure is logged and does not crash the service. | Runner never panics on `Failed`/`TimedOut`; `tracing::warn!` mirrors timeouts. |
| Hook output is bounded and redacted. | `output_under_cap_is_not_truncated` + `output_at_cap_plus_one_is_truncated`; `secret_value_is_redacted_from_stdout` + `empty_secret_string_is_ignored`. |

## Boundary tests (per project rule)

- Output cap: `=N` (under cap) paired with `=N+1` (over cap).
- Timeout: `sleep 5` with `timeout_ms = 200` (TimedOut) plus the `Success` cases so both branches are exercised.
- Secrets: redaction enabled with one secret paired with redaction enabled with an empty needle (would have crashed under naïve `replace("")`).

## Out of scope

- Wasm safe-hook replacement (per non-goal).
- Per-hook environment injection beyond the workspace cwd.
- Hook scheduling / chaining inside the orchestrator (lands with #18).
- Async tokio-based runner; synchronous is enough for ≤30 s hooks.

## References

- `crates/cadenza-workflow/src/lib.rs` — `HooksConfig`, `HookPhase`.
- `crates/cadenza-workspace/src/hooks.rs` — `HookRunner`.
- `decisions/0001-rust-host-wasm-extensions.md`.
