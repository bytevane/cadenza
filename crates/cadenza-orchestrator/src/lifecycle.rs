//! Run-lifecycle policy: continuation / retry / stall / reconcile.
//!
//! These helpers are *pure* — they take the current `RuntimeState`
//! snapshot, a clock reading (`now_ms`), and the latest event from the
//! Codex client, and they return a `LifecycleDecision` that the
//! orchestrator translates into a state mutation. Pure separation
//! keeps the lifecycle rules unit-testable without spinning up a
//! Codex server or a workspace.

use std::collections::BTreeMap;

use crate::{RetryEntry, RuntimeState};

/// Hard cap on retry attempts before the orchestrator declares
/// permanent failure. Exposed so tests can assert the bound and so a
/// workflow override can be added without churning the type signature.
pub const DEFAULT_MAX_RETRIES: u32 = 5;

/// Exponential backoff: `base_ms * 2^(attempt - 1)`, clamped at
/// `cap_ms`. `attempt` starts at 1 for the FIRST retry; attempt 0 is
/// reserved for "first run, no retries yet". Saturating arithmetic
/// prevents an overflow if a workflow asks for a 100-attempt retry.
pub fn backoff_delay_ms(attempt: u32, base_ms: u64, cap_ms: u64) -> u64 {
    if attempt == 0 {
        return 0;
    }
    let shift = (attempt - 1).min(63);
    let multiplier = 1u64 << shift;
    base_ms.saturating_mul(multiplier).min(cap_ms)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleDecision {
    /// Normal exit. Schedule continuation = a fresh dispatch on the
    /// next poll tick. `attempt` is the new attempt counter (1-indexed
    /// for the next continuation).
    Continuation { issue_id: String, next_attempt: u32 },
    /// Failure (or stall-induced kill). Schedule a retry with a
    /// deterministic backoff delay.
    Retry {
        issue_id: String,
        attempt: u32,
        due_at_ms: u64,
        reason: String,
    },
    /// `attempt > max_retries`. Release the slot, record permanent
    /// failure for observability, do not re-enqueue.
    GiveUp { issue_id: String, attempt: u32 },
    /// The issue's tracker state moved into a terminal state. Release
    /// the slot and let workspace cleanup happen out-of-band.
    Cleanup {
        issue_id: String,
        terminal_state: String,
    },
}

/// Builder-style policy config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecyclePolicy {
    pub max_retries: u32,
    pub base_backoff_ms: u64,
    pub backoff_cap_ms: u64,
    /// A run with no progress event in this window is considered
    /// stalled and gets killed.
    pub stall_timeout_ms: u64,
}

impl Default for LifecyclePolicy {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            base_backoff_ms: 1_000,
            backoff_cap_ms: 5 * 60 * 1_000,
            stall_timeout_ms: 10 * 60 * 1_000,
        }
    }
}

impl LifecyclePolicy {
    /// Normal turn completion. Continuation = a fresh dispatch on the
    /// next tick under a bumped attempt counter (so the orchestrator
    /// can observe "this is a continuation of CAD-42, attempt N+1").
    pub fn on_turn_completed(
        &self,
        issue_id: impl Into<String>,
        prior_attempt: u32,
    ) -> LifecycleDecision {
        LifecycleDecision::Continuation {
            issue_id: issue_id.into(),
            next_attempt: prior_attempt.saturating_add(1),
        }
    }

    /// Failure with error message. Bumps the retry counter and
    /// schedules a backoff. When `attempt + 1 > max_retries` the
    /// decision is `GiveUp`.
    pub fn on_turn_failed(
        &self,
        issue_id: impl Into<String>,
        prior_attempt: u32,
        now_ms: u64,
        reason: impl Into<String>,
    ) -> LifecycleDecision {
        let issue_id = issue_id.into();
        let next_attempt = prior_attempt.saturating_add(1);
        if next_attempt > self.max_retries {
            return LifecycleDecision::GiveUp {
                issue_id,
                attempt: next_attempt,
            };
        }
        let delay = backoff_delay_ms(next_attempt, self.base_backoff_ms, self.backoff_cap_ms);
        LifecycleDecision::Retry {
            issue_id,
            attempt: next_attempt,
            due_at_ms: now_ms.saturating_add(delay),
            reason: reason.into(),
        }
    }

    /// Stall detected. The orchestrator must kill the child process
    /// out-of-band; the decision is identical to a failure with
    /// `reason="stall"`.
    pub fn on_stall_detected(
        &self,
        issue_id: impl Into<String>,
        prior_attempt: u32,
        now_ms: u64,
    ) -> LifecycleDecision {
        self.on_turn_failed(issue_id, prior_attempt, now_ms, "stall")
    }

    /// Tracker moved the issue to a terminal state. Release the slot
    /// and (caller-side) clean up the workspace.
    pub fn on_issue_terminal(
        &self,
        issue_id: impl Into<String>,
        terminal_state: impl Into<String>,
    ) -> LifecycleDecision {
        LifecycleDecision::Cleanup {
            issue_id: issue_id.into(),
            terminal_state: terminal_state.into(),
        }
    }

    /// Is `last_progress_ms` older than `stall_timeout_ms` relative to
    /// `now_ms`? Pure predicate so the orchestrator can call it cheaply
    /// per running issue per tick.
    pub fn is_stalled(&self, now_ms: u64, last_progress_ms: u64) -> bool {
        now_ms.saturating_sub(last_progress_ms) >= self.stall_timeout_ms
    }
}

/// Startup reconcile plan: what to do with each tracker-known issue,
/// given the local workspace directory state. Pure data — the caller
/// (orchestrator main) applies the actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcilePlan {
    /// Issues the tracker reports as active and that the orchestrator
    /// should resume tracking. The orchestrator may treat these as
    /// new candidates on the next poll tick.
    pub resume: Vec<String>,
    /// Workspaces that exist on disk but the tracker has moved to a
    /// terminal state. Caller cleans these up out-of-band.
    pub cleanup: Vec<String>,
    /// Issues the tracker still knows about but for which no workspace
    /// exists locally. Nothing to do; informational so the caller can
    /// log "freshly seen since last boot".
    pub fresh: Vec<String>,
}

/// `tracker_states` maps `issue_id -> current tracker state`.
/// `local_workspaces` is the set of workspace-key directories the host
/// found on disk (one per per-issue workspace). `terminal_states` is
/// the workflow-configured set of states that mean "do not run".
///
/// The contract is purely lexical — no DB, no journal, no in-flight
/// process inspection. That matches the issue acceptance ("Startup
/// recovery reconciles tracker + workspace, not a durable DB").
pub fn reconcile_from_tracker(
    tracker_states: &BTreeMap<String, String>,
    local_workspaces: &[String],
    terminal_states: &std::collections::BTreeSet<String>,
) -> ReconcilePlan {
    let mut resume = Vec::new();
    let mut cleanup = Vec::new();
    let mut fresh = Vec::new();

    let local: std::collections::BTreeSet<String> = local_workspaces.iter().cloned().collect();

    for (id, state) in tracker_states {
        if terminal_states.contains(state) {
            if local.contains(id) {
                cleanup.push(id.clone());
            }
            // Terminal + no local workspace = nothing to do; not even
            // worth reporting.
        } else if local.contains(id) {
            resume.push(id.clone());
        } else {
            fresh.push(id.clone());
        }
    }

    // Workspaces present locally but missing from the tracker entirely
    // ("orphaned") are also cleanup candidates.
    for ws in &local {
        if !tracker_states.contains_key(ws) {
            cleanup.push(ws.clone());
        }
    }

    cleanup.sort();
    cleanup.dedup();
    resume.sort();
    fresh.sort();

    ReconcilePlan {
        resume,
        cleanup,
        fresh,
    }
}

/// Apply a `LifecycleDecision` to runtime state. Mutating cousin of
/// the pure policy fns. Single-writer by `&mut self`.
impl RuntimeState {
    pub fn apply_lifecycle(&mut self, decision: &LifecycleDecision) {
        match decision {
            LifecycleDecision::Continuation { issue_id, .. } => {
                // Continuation releases the running slot but keeps
                // the claim for the next dispatch tick.
                self.running.remove(issue_id);
                self.retry_attempts.remove(issue_id);
            }
            LifecycleDecision::Retry {
                issue_id,
                attempt,
                due_at_ms,
                reason,
            } => {
                self.running.remove(issue_id);
                // Hold the claim so the next select_dispatch sees
                // AlreadyClaimed until the retry fires.
                self.retry_attempts.insert(
                    issue_id.clone(),
                    RetryEntry {
                        issue_id: issue_id.clone(),
                        identifier: issue_id.clone(),
                        attempt: *attempt,
                        due_at_ms: *due_at_ms,
                        error: Some(reason.clone()),
                    },
                );
            }
            LifecycleDecision::GiveUp { issue_id, .. } => {
                self.running.remove(issue_id);
                self.claimed.remove(issue_id);
                self.retry_attempts.remove(issue_id);
            }
            LifecycleDecision::Cleanup { issue_id, .. } => {
                self.running.remove(issue_id);
                self.claimed.remove(issue_id);
                self.retry_attempts.remove(issue_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> LifecyclePolicy {
        LifecyclePolicy {
            max_retries: 3,
            base_backoff_ms: 100,
            backoff_cap_ms: 10_000,
            stall_timeout_ms: 5_000,
        }
    }

    // ---------- backoff_delay_ms ----------

    #[test]
    fn backoff_at_attempt_zero_is_zero() {
        assert_eq!(backoff_delay_ms(0, 100, 10_000), 0);
    }

    #[test]
    fn backoff_doubles_each_attempt() {
        // base=100ms: 1→100, 2→200, 3→400, 4→800
        assert_eq!(backoff_delay_ms(1, 100, 10_000), 100);
        assert_eq!(backoff_delay_ms(2, 100, 10_000), 200);
        assert_eq!(backoff_delay_ms(3, 100, 10_000), 400);
        assert_eq!(backoff_delay_ms(4, 100, 10_000), 800);
    }

    #[test]
    fn backoff_clamps_at_cap() {
        // Boundary: attempt large enough that the exponential blows
        // past the cap. Result must be exactly cap_ms.
        assert_eq!(backoff_delay_ms(20, 100, 5_000), 5_000);
    }

    #[test]
    fn backoff_saturates_on_extreme_attempt_without_panic() {
        // Overflow safety: a malicious or buggy workflow passing
        // attempt=u32::MAX must not panic. We don't care which value
        // comes back — we care that the call returns.
        let _ = backoff_delay_ms(u32::MAX, 1, 10_000);
        let _ = backoff_delay_ms(u32::MAX, u64::MAX, u64::MAX);
    }

    // ---------- on_turn_completed ----------

    #[test]
    fn continuation_increments_attempt() {
        let p = policy();
        let d = p.on_turn_completed("cad-42", 3);
        assert!(
            matches!(d, LifecycleDecision::Continuation { ref issue_id, next_attempt: 4 } if issue_id == "cad-42")
        );
    }

    // ---------- on_turn_failed ----------

    #[test]
    fn failure_first_retry_uses_base_delay() {
        let p = policy();
        let d = p.on_turn_failed("a", 0, 1_000, "boom");
        match d {
            LifecycleDecision::Retry {
                attempt,
                due_at_ms,
                reason,
                ..
            } => {
                assert_eq!(attempt, 1);
                assert_eq!(due_at_ms, 1_100); // 1000 + 100ms backoff
                assert_eq!(reason, "boom");
            }
            other => panic!("expected Retry, got {other:?}"),
        }
    }

    #[test]
    fn failure_beyond_max_retries_is_give_up() {
        let p = policy();
        // max_retries=3 → attempt 1,2,3 retry; attempt 4 = give up.
        match p.on_turn_failed("a", 3, 1_000, "x") {
            LifecycleDecision::GiveUp { attempt: 4, .. } => {}
            other => panic!("expected GiveUp, got {other:?}"),
        }
    }

    #[test]
    fn failure_at_max_retries_boundary_still_retries() {
        // Paired-edge to "beyond max". Prior=2 → next=3 ≤ max=3, retry.
        let p = policy();
        match p.on_turn_failed("a", 2, 0, "x") {
            LifecycleDecision::Retry { attempt: 3, .. } => {}
            other => panic!("expected Retry attempt=3, got {other:?}"),
        }
    }

    // ---------- on_stall_detected ----------

    #[test]
    fn stall_becomes_retry_with_stall_reason() {
        let p = policy();
        match p.on_stall_detected("a", 1, 1_000) {
            LifecycleDecision::Retry { reason, .. } => assert_eq!(reason, "stall"),
            other => panic!("expected Retry, got {other:?}"),
        }
    }

    #[test]
    fn is_stalled_paired_edges() {
        let p = policy();
        // Last progress 4_999ms ago — still inside the 5s window.
        assert!(!p.is_stalled(5_000, 1));
        // Boundary: last progress 5_000ms ago — exactly at the timeout.
        assert!(p.is_stalled(5_000, 0));
    }

    // ---------- on_issue_terminal ----------

    #[test]
    fn terminal_decision_records_state_name() {
        let p = policy();
        match p.on_issue_terminal("a", "done") {
            LifecycleDecision::Cleanup { terminal_state, .. } => {
                assert_eq!(terminal_state, "done");
            }
            other => panic!("got {other:?}"),
        }
    }

    // ---------- apply_lifecycle ----------

    #[test]
    fn continuation_releases_running_slot_but_keeps_claim() {
        let mut s = RuntimeState::new(5_000, 2);
        s.adopt_workflow(1, ["todo".into()], ["done".into()]);
        let attempt = cadenza_core::RunAttempt {
            issue_id: "a".into(),
            issue_identifier: "CAD-1".into(),
            attempt: Some(1),
            workspace_path: "/tmp".into(),
            started_at: None,
            status: cadenza_core::RunStatus::Running,
            error: None,
        };
        s.start_run("a".into(), attempt);
        assert_eq!(s.running.len(), 1);
        s.apply_lifecycle(&LifecycleDecision::Continuation {
            issue_id: "a".into(),
            next_attempt: 2,
        });
        assert_eq!(s.running.len(), 0);
        // Claim retained — the continuation will dispatch again next tick.
        assert!(s.claimed.contains("a"));
    }

    #[test]
    fn retry_decision_releases_running_and_enqueues_retry() {
        let mut s = RuntimeState::new(5_000, 2);
        s.adopt_workflow(1, ["todo".into()], ["done".into()]);
        let attempt = cadenza_core::RunAttempt {
            issue_id: "a".into(),
            issue_identifier: "CAD-1".into(),
            attempt: Some(1),
            workspace_path: "/tmp".into(),
            started_at: None,
            status: cadenza_core::RunStatus::Running,
            error: None,
        };
        s.start_run("a".into(), attempt);
        s.apply_lifecycle(&LifecycleDecision::Retry {
            issue_id: "a".into(),
            attempt: 2,
            due_at_ms: 2_500,
            reason: "boom".into(),
        });
        assert!(s.running.is_empty());
        let retry = s.retry_attempts.get("a").unwrap();
        assert_eq!(retry.attempt, 2);
        assert_eq!(retry.due_at_ms, 2_500);
        assert_eq!(retry.error.as_deref(), Some("boom"));
    }

    #[test]
    fn give_up_clears_running_claim_and_retry() {
        let mut s = RuntimeState::new(5_000, 2);
        s.adopt_workflow(1, ["todo".into()], ["done".into()]);
        let attempt = cadenza_core::RunAttempt {
            issue_id: "a".into(),
            issue_identifier: "CAD-1".into(),
            attempt: Some(1),
            workspace_path: "/tmp".into(),
            started_at: None,
            status: cadenza_core::RunStatus::Running,
            error: None,
        };
        s.start_run("a".into(), attempt);
        s.apply_lifecycle(&LifecycleDecision::GiveUp {
            issue_id: "a".into(),
            attempt: 5,
        });
        assert!(s.running.is_empty());
        assert!(!s.claimed.contains("a"));
        assert!(!s.retry_attempts.contains_key("a"));
    }

    #[test]
    fn cleanup_clears_running_claim_and_retry() {
        let mut s = RuntimeState::new(5_000, 2);
        s.adopt_workflow(1, ["todo".into()], ["done".into()]);
        let attempt = cadenza_core::RunAttempt {
            issue_id: "a".into(),
            issue_identifier: "CAD-1".into(),
            attempt: Some(1),
            workspace_path: "/tmp".into(),
            started_at: None,
            status: cadenza_core::RunStatus::Running,
            error: None,
        };
        s.start_run("a".into(), attempt);
        s.apply_lifecycle(&LifecycleDecision::Cleanup {
            issue_id: "a".into(),
            terminal_state: "done".into(),
        });
        assert!(s.running.is_empty());
        assert!(!s.claimed.contains("a"));
    }

    // ---------- reconcile_from_tracker ----------

    fn states(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn terminal(s: &[&str]) -> std::collections::BTreeSet<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn reconcile_active_with_local_workspace_is_resume() {
        let plan = reconcile_from_tracker(
            &states(&[("a", "todo"), ("b", "in progress")]),
            &["a".into(), "b".into()],
            &terminal(&["done"]),
        );
        assert_eq!(plan.resume, vec!["a", "b"]);
        assert!(plan.cleanup.is_empty());
        assert!(plan.fresh.is_empty());
    }

    #[test]
    fn reconcile_terminal_with_local_workspace_is_cleanup() {
        let plan = reconcile_from_tracker(
            &states(&[("a", "done")]),
            &["a".into()],
            &terminal(&["done"]),
        );
        assert_eq!(plan.cleanup, vec!["a"]);
    }

    #[test]
    fn reconcile_active_without_workspace_is_fresh() {
        let plan = reconcile_from_tracker(&states(&[("a", "todo")]), &[], &terminal(&["done"]));
        assert_eq!(plan.fresh, vec!["a"]);
    }

    #[test]
    fn reconcile_orphan_workspace_without_tracker_is_cleanup() {
        // Workspace exists but tracker has no record at all.
        let plan = reconcile_from_tracker(&states(&[]), &["orphan".into()], &terminal(&["done"]));
        assert_eq!(plan.cleanup, vec!["orphan"]);
    }

    #[test]
    fn reconcile_uses_no_persistent_db() {
        // Smoke: feeding two distinct snapshots gives outputs that
        // depend ONLY on the inputs, not on any cross-call state.
        let plan_a = reconcile_from_tracker(
            &states(&[("a", "todo")]),
            &["a".into()],
            &terminal(&["done"]),
        );
        let plan_b = reconcile_from_tracker(&states(&[]), &[], &terminal(&["done"]));
        assert_eq!(plan_a.resume, vec!["a"]);
        assert_eq!(plan_b.resume.len(), 0);
        // Re-running plan_a yields the same answer (idempotent + stateless).
        let plan_a_again = reconcile_from_tracker(
            &states(&[("a", "todo")]),
            &["a".into()],
            &terminal(&["done"]),
        );
        assert_eq!(plan_a, plan_a_again);
    }
}
