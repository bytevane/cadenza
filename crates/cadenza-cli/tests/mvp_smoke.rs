//! End-to-end MVP smoke (Issue #22).
//!
//! Wires the moving parts together with mocks:
//!
//! - **Mock Linear tracker** — returns a canned `Issue` list.
//! - **WORKFLOW.md parse** — the shipped `WORKFLOW.example.md` is used
//!   as the workflow fixture so the smoke covers the same YAML the
//!   operator would write.
//! - **Orchestrator** — `RuntimeState::select_dispatch` filters and
//!   capacity-slices the tracker output.
//! - **Mock Codex event stream** — a sequence of JSONL notifications
//!   parsed via `cadenza_codex::parse_notification_line`.
//! - **Lifecycle decision** — `LifecyclePolicy::on_turn_completed`
//!   moves the run forward, and `RuntimeState::apply_lifecycle`
//!   updates the in-memory state.
//! - **Observability snapshot** — the final state is projected into a
//!   `RuntimeSnapshot` and asserted via the cadenza-obs scrubber.
//!
//! The Wasm host-capability hop is intentionally out of scope here —
//! that surface is blocked behind #16/#17 (Wasm host capabilities +
//! plugin rewrite). Everything that is testable today is exercised by
//! this single test.

use async_trait::async_trait;

use cadenza_codex::{TurnEvent, parse_notification_line};
use cadenza_core::{Issue, RunAttempt, RunStatus};
use cadenza_obs::{IssueRunningView, LastReloadView, RuntimeSnapshot, Scrubber, redact_snapshot};
use cadenza_orchestrator::{
    LifecycleDecision, LifecyclePolicy, RuntimeState, lifecycle::reconcile_from_tracker,
};
use cadenza_tracker_linear::{IssueTrackerClient, TrackerError};
use cadenza_workflow::{PromptInput, parse_workflow, render_prompt};

/// Mock Linear tracker: always returns the same canned issue list.
struct MockTracker {
    issues: Vec<Issue>,
}

#[async_trait]
impl IssueTrackerClient for MockTracker {
    async fn fetch_candidate_issues(&self) -> Result<Vec<Issue>, TrackerError> {
        Ok(self.issues.clone())
    }
    async fn fetch_issues_by_states(&self, _states: &[String]) -> Result<Vec<Issue>, TrackerError> {
        Ok(self.issues.clone())
    }
    async fn fetch_issue_states_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<(String, String)>, TrackerError> {
        Ok(self
            .issues
            .iter()
            .filter(|i| ids.contains(&i.id))
            .map(|i| (i.id.clone(), i.state.clone()))
            .collect())
    }
}

fn sample_issue() -> Issue {
    Issue {
        id: "issue-cad-42".into(),
        identifier: "CAD-42".into(),
        title: "Wire MVP smoke".into(),
        description: Some("Drive one turn through the runtime.".into()),
        priority: Some(1),
        state: "todo".into(),
        branch_name: Some("feat/smoke".into()),
        url: Some("https://linear.app/cad/CAD-42".into()),
        labels: vec!["priority:P0".into()],
        blocked_by: vec![],
        created_at: Some("2026-05-22T10:00:00Z".into()),
        updated_at: Some("2026-05-22T10:00:00Z".into()),
    }
}

/// Deterministic JSONL stream for one happy-path turn.
const MOCK_TURN: &str = r#"{"jsonrpc":"2.0","method":"thread/started","params":{"thread":{"id":"thr_smoke"}}}
{"jsonrpc":"2.0","method":"turn/started","params":{"threadId":"thr_smoke","turn":{"id":"turn_1","status":"running","items":[],"itemsView":{},"error":null,"startedAt":null,"completedAt":null,"durationMs":null}}}
{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"thr_smoke","turnId":"turn_1","delta":"Hello "}}
{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"thr_smoke","turnId":"turn_1","delta":"CAD-42"}}
{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"threadId":"thr_smoke","turnId":"turn_1","tokenUsage":{"total":{"totalTokens":42,"inputTokens":30,"cachedInputTokens":0,"outputTokens":12,"reasoningOutputTokens":0},"last":{"totalTokens":42,"inputTokens":30,"cachedInputTokens":0,"outputTokens":12,"reasoningOutputTokens":0},"modelContextWindow":128000}}}
{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"thr_smoke","turn":{"id":"turn_1","status":"completed","items":[],"itemsView":{},"error":null,"startedAt":1,"completedAt":2,"durationMs":1}}}"#;

#[tokio::test]
async fn mvp_smoke_drives_one_issue_end_to_end() {
    // 1. Parse the shipped WORKFLOW.example.md so the smoke covers the
    //    same YAML the operator authors.
    let workflow_text = include_str!("../../../WORKFLOW.example.md");
    let workflow = parse_workflow(workflow_text).expect("example workflow parses");

    // 2. Mock tracker returns one candidate.
    let tracker = MockTracker {
        issues: vec![sample_issue()],
    };
    let candidates = tracker.fetch_candidate_issues().await.unwrap();
    assert_eq!(candidates.len(), 1);

    // 3. Reconcile from a fresh-boot snapshot. No workspaces on disk
    //    yet → the issue is "fresh".
    let plan = reconcile_from_tracker(
        &candidates
            .iter()
            .map(|i| (i.id.clone(), i.state.clone()))
            .collect(),
        &[],
        &workflow
            .config
            .orchestrator
            .terminal_states
            .iter()
            .cloned()
            .collect(),
    );
    assert!(plan.resume.is_empty());
    assert!(plan.cleanup.is_empty());
    assert_eq!(plan.fresh, vec!["issue-cad-42".to_string()]);

    // 4. Orchestrator adopts the workflow and dispatches.
    let mut state = RuntimeState::new(
        workflow.config.poll.interval_ms,
        workflow.config.orchestrator.max_concurrent_agents as usize,
    );
    state.adopt_workflow(
        1,
        workflow.config.orchestrator.active_states.iter().cloned(),
        workflow.config.orchestrator.terminal_states.iter().cloned(),
    );
    let dispatch = state.select_dispatch(&candidates, 0);
    assert_eq!(dispatch.to_dispatch.len(), 1);
    assert_eq!(dispatch.to_dispatch[0].identifier, "CAD-42");
    assert!(dispatch.skipped.is_empty());

    // 5. Render the prompt for the dispatched issue.
    let issue = &dispatch.to_dispatch[0];
    let rendered = render_prompt(
        &workflow.prompt_template,
        &PromptInput { issue, attempt: 1 },
    )
    .expect("prompt render");
    assert!(rendered.contains("CAD-42"), "{rendered}");
    assert!(rendered.contains("Wire MVP smoke"));

    // 6. Mark the run as running in the orchestrator.
    let attempt = RunAttempt {
        issue_id: issue.id.clone(),
        issue_identifier: issue.identifier.clone(),
        attempt: Some(1),
        workspace_path: "/tmp/cadenza/workspaces/issue-cad-42".into(),
        started_at: None,
        status: RunStatus::Running,
        error: None,
    };
    state.start_run(issue.id.clone(), attempt);
    assert_eq!(state.running.len(), 1);

    // 7. Feed the mock Codex JSONL stream through the event parser.
    //    The smoke captures: thread id, turn id, agent delta, token
    //    usage, completion signal.
    let mut thread_id: Option<String> = None;
    let mut turn_id: Option<String> = None;
    let mut completed = false;
    let mut input_tokens = 0u64;
    let mut delta_buffer = String::new();
    for line in MOCK_TURN.lines() {
        match parse_notification_line(line).expect("notification parses") {
            TurnEvent::ThreadStarted { thread_id: tid } => thread_id = Some(tid),
            TurnEvent::TurnStarted { turn_id: id, .. } => turn_id = Some(id),
            TurnEvent::AgentMessageDelta { delta, .. } => delta_buffer.push_str(&delta),
            TurnEvent::TokenUsage {
                input_tokens: it, ..
            } => input_tokens = it,
            TurnEvent::TurnCompleted { .. } => completed = true,
            _ => {}
        }
    }
    assert_eq!(thread_id.as_deref(), Some("thr_smoke"));
    assert_eq!(turn_id.as_deref(), Some("turn_1"));
    assert_eq!(delta_buffer, "Hello CAD-42");
    assert_eq!(input_tokens, 30);
    assert!(completed);

    // 8. Apply the lifecycle decision for "turn completed".
    let policy = LifecyclePolicy::default();
    let decision = policy.on_turn_completed(issue.id.clone(), 1);
    assert!(matches!(
        decision,
        LifecycleDecision::Continuation {
            next_attempt: 2,
            ..
        }
    ));
    state.apply_lifecycle(&decision);
    assert!(state.running.is_empty(), "run slot released");
    assert!(
        state.claimed.contains(&issue.id),
        "claim retained for continuation"
    );

    // 9. Project state into the observability snapshot and verify it
    //    survives redaction. The snapshot has no secret values
    //    registered here, but the scrubber pipeline must still be
    //    exercised so a future test that injects one fails on a leak.
    let mut snapshot = RuntimeSnapshot {
        workflow_version: state.workflow_version,
        max_concurrent_agents: state.max_concurrent_agents,
        running: state
            .running
            .iter()
            .map(|(id, entry)| IssueRunningView {
                issue_id: id.clone(),
                identifier: entry.attempt.issue_identifier.clone(),
                attempt: entry.attempt.attempt.unwrap_or(0),
                thread_id: thread_id.clone(),
                turn_id: turn_id.clone(),
                last_event: Some("turn/completed".into()),
                started_at_ms: None,
            })
            .collect(),
        retry: Vec::new(),
        recent_skips: Vec::new(),
        last_reload: Some(LastReloadView {
            at_ms: 0,
            version: 1,
            outcome: "Loaded".into(),
            error: None,
        }),
    };
    redact_snapshot(&mut snapshot);
    let json = serde_json::to_string_pretty(&snapshot).unwrap();
    assert!(json.contains("\"workflow_version\": 1"), "{json}");

    // 10. Scrubber must catch a hypothetical leak. If a future change
    //     accidentally embeds the registered token into a free-form
    //     field, the snapshot serialisation will redact it.
    let scrubber = Scrubber::with_secrets(vec!["lr_tok_smoke_xyz".into()]);
    let leak_attempt = "config token=lr_tok_smoke_xyz oops";
    let scrubbed = scrubber.scrub_text(leak_attempt);
    assert!(
        !scrubbed.contains("lr_tok_smoke_xyz"),
        "leak should have been scrubbed: {scrubbed}",
    );
    // Either marker is acceptable — value-substring removal emits
    // `***REDACTED***`, and the KEY=VALUE pass that runs after may
    // further rewrite that to `[REDACTED]`. Both are evidence of
    // redaction.
    assert!(
        scrubbed.contains("[REDACTED]") || scrubbed.contains("***REDACTED***"),
        "expected a redaction marker, got: {scrubbed}",
    );
}
