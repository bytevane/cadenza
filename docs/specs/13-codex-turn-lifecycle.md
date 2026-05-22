# Spec: Issue #13 — Codex turn lifecycle + event stream parser

Tracks https://github.com/bytevane/cadenza/issues/13 (Milestone: MVP 2 - Runtime Integrations).

## Outcome

`cadenza-codex` ships an event-stream parser that converts Codex JSON-RPC notifications into a narrow, orchestration-actionable `TurnEvent` enum. Replay fixtures cover the canonical successful and failed turns; unknown notification methods pass through as `TurnEvent::Other` per a documented policy so future Codex releases don't break parsing.

The full turn-lifecycle wiring (issue request → receive events → release client) is left to the orchestrator (#18); this PR provides the parsing primitive and the typed event surface that the orchestrator will consume.

## Public surface

```rust
pub mod events;
pub use events::{parse_notification_line, EventStreamError, TurnEvent};

pub enum TurnEvent {
    ThreadStarted { thread_id },
    TurnStarted { thread_id, turn_id },
    TurnCompleted { thread_id, turn_id },
    Error { thread_id, turn_id, message, will_retry },
    TokenUsage { thread_id, turn_id, input_tokens, output_tokens },
    AgentMessageDelta { thread_id, turn_id, delta },
    RateLimitsUpdated { params: serde_json::Value },
    Other { method, params },
}

pub fn parse_notification_line(line: &str) -> Result<TurnEvent, EventStreamError>;
```

## Mapping to the frozen schema

| `method` (schemas/codex/current/) | TurnEvent variant |
| --- | --- |
| `thread/started` | `ThreadStarted` |
| `turn/started` | `TurnStarted` |
| `turn/completed` | `TurnCompleted` |
| `error` | `Error` (carries `willRetry`) |
| `thread/tokenUsage/updated` | `TokenUsage` (defaults to 0 on missing) |
| `item/agentMessage/delta` | `AgentMessageDelta` |
| `account/rateLimits/updated` | `RateLimitsUpdated { params }` |
| anything else | `Other { method, params }` |

The narrow variants are validated by `serde` against the relevant `schemas/codex/current/v2/` types; a known method with an unexpected shape returns `EventStreamError::Json` rather than panicking.

## Unknown-event policy (per acceptance)

A notification whose `method` is outside the enumerated set produces `TurnEvent::Other { method, params }`. The orchestrator's responsibility is to log+ignore at `debug` level. This guarantees:

1. New Codex releases that add notification methods do not break the parser. The schema gate (#4) still surfaces the protocol bump as a reviewable diff so structural changes are never *silently* ignored.
2. The orchestrator's lifecycle code only matches on variants it has explicit handling for; everything else passes through with the `method` string available for monitoring.

## Replay fixtures (per acceptance)

Three fixtures live in `crates/cadenza-codex/src/events.rs`:

- `SUCCESSFUL_TURN_JSONL` — `thread/started` → `turn/started` → two `agentMessage/delta` frames → `thread/tokenUsage/updated` → `turn/completed`.
- `FAILED_TURN_JSONL` — `error` with `willRetry=true`.
- `SUCCESSFUL_TURN_WITH_RATE_LIMITS_AND_UNKNOWN` — covers rate-limit and unknown-method pass-through together.

## Acceptance verification

| Acceptance criterion (from #13) | Verification |
| --- | --- |
| A replay fixture can simulate a successful turn. | `replay_fixture_successful_turn`. |
| A replay fixture can simulate a failed turn. | `replay_fixture_failed_turn`. |
| Unknown event types are handled according to a documented policy. | `unknown_event_passes_through_as_other` + the policy section above. |
| Runtime returns enough metadata for retry/continuation decisions. | `Error.will_retry`, `TokenUsage.{input,output}_tokens`, `TurnCompleted.turn_id`. |

## Boundary tests (per project rule)

- Successful turn (5 events, all known) paired with failed turn (3 events, including `error`).
- Known-method-with-missing-field (`tokenUsage` with no fields) paired with malformed JSON — both classify as typed errors / sane defaults instead of panicking.
- Known method (`turn/completed`) paired with unknown method (`future/unknown/event`) — same input shape, different routing.

## Out of scope (per #13 non-goal)

- Dynamic tool invocation (#17).

## References

- `schemas/codex/current/v2/ThreadStartedNotification.ts`, `TurnStartedNotification.ts`, `TurnCompletedNotification.ts`, `ErrorNotification.ts`, `ThreadTokenUsageUpdatedNotification.ts`, `AgentMessageDeltaNotification.ts`.
- `decisions/0002-codex-stdio-schema-gate.md`.
