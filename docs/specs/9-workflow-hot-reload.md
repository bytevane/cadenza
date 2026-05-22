# Spec: Issue #9 — Implement defensive WORKFLOW.md hot reload

Tracks https://github.com/bytevane/cadenza/issues/9 (Milestone: MVP 1 - Workflow & Workspace).

## Outcome

`cadenza-workflow` exposes a thread-safe `WorkflowSource` that owns the
active workflow definition and refuses to lose its last-known-good
config on any reload failure. `WorkflowWatcher` wraps a `notify`
recommended watcher so file edits drive `try_reload` automatically.

## Public surface (new)

```rust
pub struct WorkflowSource { /* … */ }
pub struct WorkflowWatcher { /* … */ }

#[derive(Debug, Clone)] pub struct ReloadEvent { pub at: SystemTime, pub outcome: ReloadOutcome }
#[derive(Debug, Clone)] pub enum ReloadOutcome { Loaded { version: u64 }, Rejected { reason: String } }

pub enum WorkflowSourceError { Io { path, source }, Workflow(WorkflowError) }
pub enum WatchError { Notify(notify::Error) }

impl WorkflowSource {
    pub fn load_initial(path: impl Into<PathBuf>) -> Result<Self, WorkflowSourceError>;
    pub fn path(&self) -> &Path;
    pub fn current(&self) -> Arc<WorkflowDefinition>;  // cheap clone
    pub fn version(&self) -> u64;                       // monotonic
    pub fn last_event(&self) -> ReloadEvent;
    pub fn history(&self) -> Vec<ReloadEvent>;          // bounded to 16
    pub fn try_reload(&self) -> ReloadOutcome;          // atomic swap on success, LKG-preserving on failure
}

impl WorkflowWatcher {
    pub fn spawn(source: Arc<WorkflowSource>) -> Result<Self, WatchError>;
    pub fn source(&self) -> &Arc<WorkflowSource>;
}
```

## Invariants

1. **Atomic swap.** `current()` is an `Arc<WorkflowDefinition>`. On a
   successful reload, the source replaces its internal `Arc` inside a
   `RwLock`; existing readers' clones continue to see the old version
   until they release them.
2. **Last-known-good.** On any error during read+parse+validate, the
   `current` arc is untouched and the version is unchanged. The
   failure is recorded as a `Rejected` event with the error text.
3. **Monotonic versioning.** Every successful reload bumps the version
   by one — even when the file content is byte-identical (the operator
   asked for a reload and it succeeded, that itself is an event).
4. **Bounded history.** The history ring keeps the last 16 events, both
   `Loaded` and `Rejected`, so an operator can see "what was the last
   failed reload" without scanning logs.
5. **Read-only from the orchestrator.** The orchestrator only sees the
   immutable `Arc<WorkflowDefinition>` through `current()`. The source
   itself has no `&mut self` surface that the orchestrator could call;
   reloads happen on the watcher thread or via a direct caller of
   `try_reload`.

## Watcher behaviour

`WorkflowWatcher::spawn` builds a `notify::RecommendedWatcher` that
calls `source.try_reload()` on `Modify`, `Create`, or `Remove` events
for the watched path. Drop the watcher to stop watching. Notify event
errors are logged at `WARN` via `tracing` and otherwise ignored —
they cannot move the active config.

## Tests (10 new, in `source::tests`)

| Test | Verifies |
| --- | --- |
| `load_initial_succeeds_on_valid_file_with_version_one` | Initial happy path; version starts at 1, last_event is `Loaded { version: 1 }`. |
| `load_initial_fails_on_missing_file` | Returns `Io` on `ENOENT`. |
| `load_initial_fails_on_invalid_workflow` | Returns `Workflow(_)` on parse failure. |
| `reload_with_valid_change_bumps_version_and_swaps` | Version → 2; old `Arc` snapshot still readable. |
| `reload_with_invalid_yaml_preserves_last_known_good` | Version unchanged; `current` Arc unchanged; `Rejected` event. |
| `reload_with_invalid_validation_preserves_last_known_good` | Same, but the failure is validation rather than YAML parse. |
| `reload_after_file_removed_preserves_last_known_good` | IO-level failure still preserves LKG. |
| `history_at_capacity_keeps_all_events` | =N boundary: history capacity exact-fit. |
| `history_beyond_capacity_drops_oldest` | =N+1 boundary: oldest event evicted on overflow. |
| `watcher_picks_up_valid_change` | End-to-end smoke: tempfile + notify; bounded 5-second deadline so it can never hang. |

## Acceptance verification

| Acceptance criterion (from #9)                                | Verification |
| ------------------------------------------------------------- | ------------ |
| Valid reload changes the active config version.               | `reload_with_valid_change_bumps_version_and_swaps`. |
| Invalid reload keeps last-known-good.                         | `reload_with_invalid_yaml_preserves_last_known_good` + `_invalid_validation_` + `_file_removed_`. |
| Reload errors are visible in logs/state snapshot.             | `Rejected { reason }` carries the error text; `tracing::warn!` mirrors it. `last_event()` and `history()` expose the snapshot. |
| Hot reload has no write access to orchestrator correctness state. | `current()` returns `Arc<WorkflowDefinition>` — read-only from the orchestrator's perspective. The source has no `&mut self` surface that the orchestrator could call. |

## Out of scope

- Runtime HTTP refresh endpoint (per #9 non-goal).
- Wiring the watcher into the orchestrator (lands with #18 / #19).
- Persistent history (the ring is in-memory only).

## References

- `notify` crate — debounced FS watcher.
- `crates/cadenza-workflow/src/source.rs`
