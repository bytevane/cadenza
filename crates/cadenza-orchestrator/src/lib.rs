//! Single-authority orchestrator state machine.
//!
//! `RuntimeState` owns the in-memory scheduler state (claimed, running,
//! retry queue, attempt counts, active config version). `select_dispatch`
//! is the deterministic candidate selector: it filters candidates with
//! explicit `SkipReason`s, sorts by priority/creation/identifier, and
//! returns at most `capacity - running` issues to dispatch.
//!
//! All mutating methods take `&mut self`; the orchestrator runs as a
//! single-task owner so concurrent state writers are impossible by
//! construction. Lifecycle rules (retry / continuation / stall /
//! reconcile) live in [`lifecycle`]. Real Codex/Wasm execution wiring
//! lands in later PRs.

pub mod lifecycle;
pub use lifecycle::{
    DEFAULT_MAX_RETRIES, LifecycleDecision, LifecyclePolicy, ReconcilePlan, backoff_delay_ms,
    reconcile_from_tracker,
};

use std::collections::{BTreeMap, BTreeSet};

use cadenza_core::{Issue, RunAttempt, RunStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeState {
    pub poll_interval_ms: u64,
    pub max_concurrent_agents: usize,
    pub running: BTreeMap<String, RunningEntry>,
    pub claimed: BTreeSet<String>,
    pub retry_attempts: BTreeMap<String, RetryEntry>,
    /// Active workflow version. Bumped by the `WorkflowSource` reload
    /// hook in #9; orchestrator decisions are tagged with this for
    /// audit. `0` means "no workflow has been loaded yet" — the
    /// orchestrator refuses to dispatch until at least version 1.
    pub workflow_version: u64,
    /// Active state names (workflow `orchestrator.active_states`).
    pub active_states: BTreeSet<String>,
    /// Terminal state names (workflow `orchestrator.terminal_states`).
    pub terminal_states: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningEntry {
    pub attempt: RunAttempt,
    pub session_id: Option<String>,
    pub last_event: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryEntry {
    pub issue_id: String,
    pub identifier: String,
    pub attempt: u32,
    pub due_at_ms: u64,
    pub error: Option<String>,
}

/// Documented reason a candidate was filtered out of dispatch. Kept
/// observable so an operator can answer "why didn't CAD-42 dispatch
/// this tick" by reading the most recent `select_dispatch` outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkipReason {
    /// The orchestrator has not yet loaded a workflow (version 0).
    NoWorkflow,
    /// `running.len() == max_concurrent_agents`.
    AtCapacity,
    /// Issue state is not in `active_states`.
    InactiveState { state: String },
    /// Issue state is in `terminal_states`.
    TerminalState { state: String },
    /// Already claimed within this owner (in `claimed` set).
    AlreadyClaimed,
    /// Already running.
    AlreadyRunning,
    /// In retry queue, retry not yet due.
    RetryPending { due_at_ms: u64 },
    /// Blocked by another unresolved issue.
    BlockedBy { blocker_identifier: String },
}

/// One candidate the orchestrator considered. `outcome=None` means the
/// candidate was selected; otherwise the reason it was skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateOutcome {
    pub issue_id: String,
    pub identifier: String,
    pub outcome: Option<SkipReason>,
}

/// Result of `select_dispatch`. Selected issues are in `to_dispatch` in
/// the order they should be claimed; `skipped` records every candidate
/// the filter rejected.
#[derive(Debug, Clone)]
pub struct DispatchPlan {
    pub to_dispatch: Vec<Issue>,
    pub skipped: Vec<CandidateOutcome>,
}

impl RuntimeState {
    pub fn new(poll_interval_ms: u64, max_concurrent_agents: usize) -> Self {
        Self {
            poll_interval_ms,
            max_concurrent_agents,
            ..Self::default()
        }
    }

    pub fn has_capacity(&self) -> bool {
        self.claimed.len() < self.max_concurrent_agents
    }

    /// In-flight work count = `claimed.len()`. `start_run` is an upsert
    /// into both `claimed` and `running`, and `complete_run` /
    /// lifecycle release paths remove from both, so `claimed` is always
    /// a superset of `running`. Using `claimed.len()` here closes the
    /// window where a claim has been recorded but `start_run` has not
    /// yet flipped `running` — without this, a second `select_dispatch`
    /// tick would treat the slot as free and over-dispatch (see #55).
    pub fn remaining_capacity(&self) -> usize {
        self.max_concurrent_agents
            .saturating_sub(self.claimed.len())
    }

    pub fn is_claimed(&self, issue_id: &str) -> bool {
        self.claimed.contains(issue_id)
    }

    pub fn claim(&mut self, issue_id: impl Into<String>) -> bool {
        self.claimed.insert(issue_id.into())
    }

    pub fn start_run(&mut self, issue_id: String, attempt: RunAttempt) {
        self.claimed.insert(issue_id.clone());
        self.running.insert(
            issue_id,
            RunningEntry {
                attempt: RunAttempt {
                    status: RunStatus::Running,
                    ..attempt
                },
                session_id: None,
                last_event: None,
            },
        );
    }

    pub fn complete_run(&mut self, issue_id: &str) {
        self.running.remove(issue_id);
        self.claimed.remove(issue_id);
    }

    pub fn enqueue_retry(&mut self, entry: RetryEntry) {
        self.retry_attempts.insert(entry.issue_id.clone(), entry);
    }

    /// Adopt a freshly-loaded workflow. Atomic from the orchestrator's
    /// point of view because `&mut self` enforces single-writer.
    pub fn adopt_workflow(
        &mut self,
        version: u64,
        active_states: impl IntoIterator<Item = String>,
        terminal_states: impl IntoIterator<Item = String>,
    ) {
        self.workflow_version = version;
        self.active_states = active_states.into_iter().collect();
        self.terminal_states = terminal_states.into_iter().collect();
    }

    /// Deterministic candidate filter + sort + capacity slice.
    ///
    /// Filter order matters because `SkipReason` is operator-facing — we
    /// stop at the first reason that applies so the answer is unique.
    /// Sort order is:
    ///   1. priority ascending (Linear treats 1 = urgent, 4 = low);
    ///      None counts as MAX so unprioritised issues land last.
    ///   2. created_at ascending (older first).
    ///   3. identifier ascending (final tie-break — fully deterministic).
    pub fn select_dispatch(&self, candidates: &[Issue], now_ms: u64) -> DispatchPlan {
        let mut to_dispatch: Vec<Issue> = Vec::new();
        let mut skipped: Vec<CandidateOutcome> = Vec::new();

        if self.workflow_version == 0 {
            for issue in candidates {
                skipped.push(CandidateOutcome {
                    issue_id: issue.id.clone(),
                    identifier: issue.identifier.clone(),
                    outcome: Some(SkipReason::NoWorkflow),
                });
            }
            return DispatchPlan {
                to_dispatch,
                skipped,
            };
        }

        // Step 1: per-issue admission filter.
        let mut admitted: Vec<&Issue> = Vec::new();
        for issue in candidates {
            if let Some(reason) = self.classify(issue, now_ms) {
                skipped.push(CandidateOutcome {
                    issue_id: issue.id.clone(),
                    identifier: issue.identifier.clone(),
                    outcome: Some(reason),
                });
            } else {
                admitted.push(issue);
            }
        }

        // Step 2: deterministic sort.
        admitted.sort_by(|a, b| {
            a.priority
                .unwrap_or(i64::MAX)
                .cmp(&b.priority.unwrap_or(i64::MAX))
                .then_with(|| a.created_at.cmp(&b.created_at))
                .then_with(|| a.identifier.cmp(&b.identifier))
        });

        // Step 3: capacity slice. Anything past capacity is "AtCapacity".
        let cap = self.remaining_capacity();
        for (idx, issue) in admitted.into_iter().enumerate() {
            if idx < cap {
                to_dispatch.push(issue.clone());
            } else {
                skipped.push(CandidateOutcome {
                    issue_id: issue.id.clone(),
                    identifier: issue.identifier.clone(),
                    outcome: Some(SkipReason::AtCapacity),
                });
            }
        }

        DispatchPlan {
            to_dispatch,
            skipped,
        }
    }

    fn classify(&self, issue: &Issue, now_ms: u64) -> Option<SkipReason> {
        // 1. Already running — strongest claim.
        if self.running.contains_key(&issue.id) {
            return Some(SkipReason::AlreadyRunning);
        }
        // 2. Already claimed by this owner (between candidate fetch and
        //    actual dispatch).
        if self.claimed.contains(&issue.id) {
            return Some(SkipReason::AlreadyClaimed);
        }
        // 3. Terminal states never dispatch.
        if self.terminal_states.contains(&issue.state) {
            return Some(SkipReason::TerminalState {
                state: issue.state.clone(),
            });
        }
        // 4. Must be in active_states.
        if !self.active_states.contains(&issue.state) {
            return Some(SkipReason::InactiveState {
                state: issue.state.clone(),
            });
        }
        // 5. Blocked by an unresolved sibling. An unresolved blocker is
        //    one whose `state` is not in `terminal_states` (e.g.
        //    blocker is still "in progress" / "todo"). If `state` is
        //    missing, the blocker is conservatively treated as
        //    unresolved.
        for blocker in &issue.blocked_by {
            let blocker_state = blocker.state.as_deref().unwrap_or("");
            let resolved = self.terminal_states.contains(blocker_state);
            if !resolved {
                let blocker_id = blocker
                    .identifier
                    .clone()
                    .or_else(|| blocker.id.clone())
                    .unwrap_or_else(|| "<unknown>".to_string());
                return Some(SkipReason::BlockedBy {
                    blocker_identifier: blocker_id,
                });
            }
        }
        // 6. Retry pending and not yet due.
        if let Some(retry) = self.retry_attempts.get(&issue.id) {
            if retry.due_at_ms > now_ms {
                return Some(SkipReason::RetryPending {
                    due_at_ms: retry.due_at_ms,
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadenza_core::BlockerRef;

    fn issue(id: &str, identifier: &str, state: &str, priority: Option<i64>) -> Issue {
        Issue {
            id: id.into(),
            identifier: identifier.into(),
            title: format!("issue {identifier}"),
            description: None,
            priority,
            state: state.into(),
            branch_name: None,
            url: None,
            labels: vec![],
            blocked_by: vec![],
            created_at: Some("2026-05-22T00:00:00Z".into()),
            updated_at: None,
        }
    }

    fn state_with(active: &[&str], terminal: &[&str], capacity: usize) -> RuntimeState {
        let mut s = RuntimeState::new(5_000, capacity);
        s.adopt_workflow(
            1,
            active.iter().map(|x| x.to_string()),
            terminal.iter().map(|x| x.to_string()),
        );
        s
    }

    #[test]
    fn no_workflow_skips_all_candidates() {
        let s = RuntimeState::new(5_000, 2);
        let plan = s.select_dispatch(&[issue("a", "CAD-1", "todo", Some(1))], 0);
        assert!(plan.to_dispatch.is_empty());
        assert_eq!(plan.skipped.len(), 1);
        assert!(matches!(
            plan.skipped[0].outcome,
            Some(SkipReason::NoWorkflow)
        ));
    }

    #[test]
    fn dispatches_up_to_capacity_in_priority_order() {
        let s = state_with(&["todo"], &["done"], 2);
        let candidates = vec![
            issue("a", "CAD-1", "todo", Some(3)),
            issue("b", "CAD-2", "todo", Some(1)),
            issue("c", "CAD-3", "todo", Some(2)),
        ];
        let plan = s.select_dispatch(&candidates, 0);
        assert_eq!(plan.to_dispatch.len(), 2);
        // CAD-2 priority 1, CAD-3 priority 2 → highest priority first.
        assert_eq!(plan.to_dispatch[0].identifier, "CAD-2");
        assert_eq!(plan.to_dispatch[1].identifier, "CAD-3");
        // CAD-1 skipped at capacity.
        assert_eq!(plan.skipped.len(), 1);
        assert!(matches!(
            plan.skipped[0].outcome,
            Some(SkipReason::AtCapacity)
        ));
    }

    #[test]
    fn sort_tiebreaks_by_created_at_then_identifier() {
        let s = state_with(&["todo"], &["done"], 4);
        let mut a = issue("a", "CAD-1", "todo", Some(1));
        let mut b = issue("b", "CAD-2", "todo", Some(1));
        let mut c = issue("c", "CAD-3", "todo", Some(1));
        // Same priority. Different created_at order, then identifier.
        a.created_at = Some("2026-05-22T10:00:00Z".into());
        b.created_at = Some("2026-05-22T09:00:00Z".into());
        c.created_at = Some("2026-05-22T09:00:00Z".into());
        let plan = s.select_dispatch(&[a, b, c], 0);
        let order: Vec<_> = plan
            .to_dispatch
            .iter()
            .map(|i| i.identifier.clone())
            .collect();
        // b + c are older (same time) — sorted by identifier between them; a comes last.
        assert_eq!(order, vec!["CAD-2", "CAD-3", "CAD-1"]);
    }

    #[test]
    fn terminal_state_is_skipped() {
        let s = state_with(&["todo"], &["done"], 2);
        let plan = s.select_dispatch(&[issue("a", "CAD-1", "done", Some(1))], 0);
        assert!(plan.to_dispatch.is_empty());
        assert!(matches!(
            plan.skipped[0].outcome,
            Some(SkipReason::TerminalState { ref state }) if state == "done"
        ));
    }

    #[test]
    fn inactive_state_is_skipped() {
        let s = state_with(&["todo"], &["done"], 2);
        let plan = s.select_dispatch(&[issue("a", "CAD-1", "icebox", Some(1))], 0);
        assert!(plan.to_dispatch.is_empty());
        assert!(matches!(
            plan.skipped[0].outcome,
            Some(SkipReason::InactiveState { ref state }) if state == "icebox"
        ));
    }

    #[test]
    fn already_running_is_skipped_with_no_capacity_consumed() {
        let mut s = state_with(&["todo"], &["done"], 2);
        let attempt = RunAttempt {
            issue_id: "a".into(),
            issue_identifier: "CAD-1".into(),
            attempt: Some(1),
            workspace_path: "/tmp/ws".into(),
            started_at: None,
            status: RunStatus::Running,
            error: None,
        };
        s.start_run("a".into(), attempt);
        let plan = s.select_dispatch(
            &[
                issue("a", "CAD-1", "todo", Some(1)),
                issue("b", "CAD-2", "todo", Some(1)),
            ],
            0,
        );
        // a is filtered with AlreadyRunning; b can take the remaining
        // slot (capacity 2 - running 1 = 1).
        assert_eq!(plan.to_dispatch.len(), 1);
        assert_eq!(plan.to_dispatch[0].identifier, "CAD-2");
        let a_skip = plan
            .skipped
            .iter()
            .find(|s| s.identifier == "CAD-1")
            .unwrap();
        assert!(matches!(a_skip.outcome, Some(SkipReason::AlreadyRunning)));
    }

    #[test]
    fn duplicate_dispatch_is_impossible_within_one_owner() {
        // Two consecutive select_dispatch calls with the same candidate
        // list. The first claims; the second sees AlreadyClaimed for
        // that issue.
        let mut s = state_with(&["todo"], &["done"], 2);
        let candidates = vec![
            issue("a", "CAD-1", "todo", Some(1)),
            issue("b", "CAD-2", "todo", Some(1)),
        ];
        let plan_1 = s.select_dispatch(&candidates, 0);
        for issue in &plan_1.to_dispatch {
            s.claim(issue.id.clone());
        }
        let plan_2 = s.select_dispatch(&candidates, 0);
        assert!(plan_2.to_dispatch.is_empty());
        for outcome in &plan_2.skipped {
            assert!(matches!(outcome.outcome, Some(SkipReason::AlreadyClaimed)));
        }
    }

    #[test]
    fn claimed_but_not_running_consumes_capacity() {
        // Regression for #58: capacity must include claimed-not-yet-running
        // work or a second tick (with fresh candidates) can dispatch past
        // `max_concurrent_agents`. Capacity 2: claim A and B (no
        // start_run), then ask to dispatch C and D — expect 0 dispatched
        // and both reported `AtCapacity`.
        let mut s = state_with(&["todo"], &["done"], 2);
        s.claim("a");
        s.claim("b");
        let plan = s.select_dispatch(
            &[
                issue("c", "CAD-3", "todo", Some(1)),
                issue("d", "CAD-4", "todo", Some(1)),
            ],
            0,
        );
        assert!(
            plan.to_dispatch.is_empty(),
            "over-dispatched past max_concurrent_agents: {plan:?}",
        );
        for outcome in &plan.skipped {
            assert!(
                matches!(outcome.outcome, Some(SkipReason::AtCapacity)),
                "wrong skip reason for {}: {:?}",
                outcome.identifier,
                outcome.outcome,
            );
        }
    }

    #[test]
    fn retry_due_in_the_future_is_skipped() {
        let mut s = state_with(&["todo"], &["done"], 2);
        s.enqueue_retry(RetryEntry {
            issue_id: "a".into(),
            identifier: "CAD-1".into(),
            attempt: 2,
            due_at_ms: 10_000,
            error: Some("upstream timeout".into()),
        });
        let plan = s.select_dispatch(&[issue("a", "CAD-1", "todo", Some(1))], 5_000);
        assert!(plan.to_dispatch.is_empty());
        assert!(matches!(
            plan.skipped[0].outcome,
            Some(SkipReason::RetryPending { due_at_ms: 10_000 })
        ));
    }

    #[test]
    fn retry_due_in_the_past_is_dispatched() {
        // Paired-edge to retry_due_in_the_future_is_skipped.
        let mut s = state_with(&["todo"], &["done"], 2);
        s.enqueue_retry(RetryEntry {
            issue_id: "a".into(),
            identifier: "CAD-1".into(),
            attempt: 2,
            due_at_ms: 1_000,
            error: None,
        });
        let plan = s.select_dispatch(&[issue("a", "CAD-1", "todo", Some(1))], 5_000);
        assert_eq!(plan.to_dispatch.len(), 1);
    }

    #[test]
    fn blocked_by_unresolved_blocker_is_skipped() {
        let s = state_with(&["todo"], &["done"], 2);
        let mut issue_b = issue("b", "CAD-2", "todo", Some(1));
        issue_b.blocked_by = vec![BlockerRef {
            id: Some("a".into()),
            identifier: Some("CAD-1".into()),
            state: Some("todo".into()),
        }];
        let plan = s.select_dispatch(&[issue_b], 0);
        assert!(plan.to_dispatch.is_empty());
        let outcome = &plan.skipped[0].outcome;
        assert!(matches!(
            outcome,
            Some(SkipReason::BlockedBy { blocker_identifier }) if blocker_identifier == "CAD-1"
        ));
    }

    #[test]
    fn blocked_by_done_blocker_dispatches() {
        let s = state_with(&["todo"], &["done"], 2);
        let mut issue_b = issue("b", "CAD-2", "todo", Some(1));
        issue_b.blocked_by = vec![BlockerRef {
            id: Some("a".into()),
            identifier: Some("CAD-1".into()),
            state: Some("done".into()),
        }];
        let plan = s.select_dispatch(&[issue_b], 0);
        assert_eq!(plan.to_dispatch.len(), 1);
    }

    #[test]
    fn capacity_zero_dispatches_nothing() {
        // Boundary: capacity = 0 is a valid configuration; everything is
        // AtCapacity.
        let s = state_with(&["todo"], &["done"], 0);
        let plan = s.select_dispatch(&[issue("a", "CAD-1", "todo", Some(1))], 0);
        assert!(plan.to_dispatch.is_empty());
        assert!(matches!(
            plan.skipped[0].outcome,
            Some(SkipReason::AtCapacity)
        ));
    }

    #[test]
    fn capacity_one_dispatches_one() {
        // Boundary pair to capacity_zero.
        let s = state_with(&["todo"], &["done"], 1);
        let plan = s.select_dispatch(
            &[
                issue("a", "CAD-1", "todo", Some(1)),
                issue("b", "CAD-2", "todo", Some(1)),
            ],
            0,
        );
        assert_eq!(plan.to_dispatch.len(), 1);
        assert_eq!(plan.skipped.len(), 1);
        assert!(matches!(
            plan.skipped[0].outcome,
            Some(SkipReason::AtCapacity)
        ));
    }

    #[test]
    fn none_priority_lands_after_prioritised_issues() {
        let s = state_with(&["todo"], &["done"], 3);
        let plan = s.select_dispatch(
            &[
                issue("a", "CAD-1", "todo", None),
                issue("b", "CAD-2", "todo", Some(2)),
                issue("c", "CAD-3", "todo", Some(1)),
            ],
            0,
        );
        let order: Vec<_> = plan
            .to_dispatch
            .iter()
            .map(|i| i.identifier.clone())
            .collect();
        assert_eq!(order, vec!["CAD-3", "CAD-2", "CAD-1"]);
    }
}
