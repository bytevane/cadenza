//! Codex turn lifecycle event parser.
//!
//! The Codex app-server emits JSON-RPC notifications over stdout once a
//! turn is in flight. Cadenza is interested in a narrow slice of that
//! traffic for orchestration decisions: which thread/turn we are on,
//! whether the turn completed, whether the server reported an error,
//! and the latest token/rate-limit signal. Other notifications pass
//! through as `TurnEvent::Other` so the parser never panics on a
//! schema bump.
//!
//! The variants here mirror the relevant fields from
//! `schemas/codex/current/v2/` — `ThreadStartedNotification`,
//! `TurnStartedNotification`, `TurnCompletedNotification`,
//! `ErrorNotification`, `ThreadTokenUsageUpdatedNotification`,
//! `AgentMessageDeltaNotification`, `AccountRateLimitsUpdatedNotification`.

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EventStreamError {
    #[error("notification line is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("notification missing `method` field")]
    MissingMethod,
}

/// Narrowly-typed view of a Codex server notification, scoped to what
/// Cadenza orchestration needs. Anything outside this set falls into
/// `Other`; the orchestrator can log+ignore those without surprises.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnEvent {
    ThreadStarted {
        thread_id: String,
    },
    TurnStarted {
        thread_id: String,
        turn_id: String,
    },
    TurnCompleted {
        thread_id: String,
        turn_id: String,
    },
    /// Server-side error during a turn. `will_retry` carries Codex's own
    /// retry signal; the orchestrator decides what to do with it (#19).
    Error {
        thread_id: String,
        turn_id: String,
        message: String,
        will_retry: bool,
    },
    /// Per-turn aggregate token usage. Optional fields are mapped to 0
    /// when missing so callers do not need to deal with `Option<u64>`.
    TokenUsage {
        thread_id: String,
        turn_id: String,
        input_tokens: u64,
        output_tokens: u64,
    },
    AgentMessageDelta {
        thread_id: String,
        turn_id: String,
        delta: String,
    },
    /// Account-scope rate-limit observation. Passed through raw because
    /// the shape varies; orchestrator surfaces relevant fields when it
    /// needs them.
    RateLimitsUpdated {
        params: serde_json::Value,
    },
    /// Anything else. Documented policy: do not panic, do not error;
    /// log+ignore at the orchestrator level. The orchestrator may
    /// match on `method` for forward-compatibility with newer Codex
    /// notifications without breaking parsing.
    Other {
        method: String,
        params: serde_json::Value,
    },
}

/// Parse a single JSONL line from the Codex stdout stream.
pub fn parse_notification_line(line: &str) -> Result<TurnEvent, EventStreamError> {
    let envelope: NotificationEnvelope = serde_json::from_str(line.trim())?;
    let params = envelope.params.unwrap_or(serde_json::Value::Null);
    Ok(match envelope.method.as_deref() {
        Some("thread/started") => {
            let parsed: ThreadStarted = serde_json::from_value(params.clone())?;
            TurnEvent::ThreadStarted {
                thread_id: parsed.thread.id,
            }
        }
        Some("turn/started") => {
            let parsed: TurnStartedOrCompleted = serde_json::from_value(params.clone())?;
            TurnEvent::TurnStarted {
                thread_id: parsed.thread_id,
                turn_id: parsed.turn.id,
            }
        }
        Some("turn/completed") => {
            let parsed: TurnStartedOrCompleted = serde_json::from_value(params.clone())?;
            TurnEvent::TurnCompleted {
                thread_id: parsed.thread_id,
                turn_id: parsed.turn.id,
            }
        }
        Some("error") => {
            let parsed: ErrorNotification = serde_json::from_value(params.clone())?;
            TurnEvent::Error {
                thread_id: parsed.thread_id,
                turn_id: parsed.turn_id,
                message: parsed.error.message,
                will_retry: parsed.will_retry,
            }
        }
        Some("thread/tokenUsage/updated") => {
            let parsed: TokenUsageNotification = serde_json::from_value(params.clone())?;
            // `tokenUsage` is nested per the v2 schema:
            // `{ total: TokenUsageBreakdown, last: TokenUsageBreakdown, … }`
            // We report `total` so the orchestrator sees the cumulative
            // counters; `last` would only describe the latest delta.
            TurnEvent::TokenUsage {
                thread_id: parsed.thread_id,
                turn_id: parsed.turn_id,
                input_tokens: parsed.token_usage.total.input_tokens,
                output_tokens: parsed.token_usage.total.output_tokens,
            }
        }
        Some("item/agentMessage/delta") => {
            let parsed: AgentMessageDelta = serde_json::from_value(params.clone())?;
            TurnEvent::AgentMessageDelta {
                thread_id: parsed.thread_id,
                turn_id: parsed.turn_id,
                delta: parsed.delta,
            }
        }
        Some("account/rateLimits/updated") => {
            // Validate the envelope so a truncated payload (e.g. `{}`) fails
            // loud; pass the raw params through afterwards because the
            // RateLimitSnapshot fields are all nullable and the orchestrator
            // wants the original shape for forwarding.
            let _envelope: RateLimitsEnvelope = serde_json::from_value(params.clone())?;
            TurnEvent::RateLimitsUpdated { params }
        }
        Some(other) => TurnEvent::Other {
            method: other.to_string(),
            params,
        },
        None => return Err(EventStreamError::MissingMethod),
    })
}

#[derive(Debug, Deserialize)]
struct NotificationEnvelope {
    #[allow(dead_code)]
    #[serde(default)]
    jsonrpc: Option<String>,
    method: Option<String>,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ThreadStarted {
    thread: ThreadView,
}

#[derive(Debug, Deserialize)]
struct ThreadView {
    id: String,
}

#[derive(Debug, Deserialize)]
struct TurnStartedOrCompleted {
    #[serde(rename = "threadId")]
    thread_id: String,
    turn: TurnView,
}

/// Schema-required Turn fields that Cadenza validates. The full `Turn`
/// struct in `schemas/codex/current/v2/Turn.ts` is
/// `{ id, items, itemsView, status, error, startedAt, completedAt, ... }`.
/// The orchestrator only consumes `id`, but every schema-required field
/// is declared here (even when nullable) so a truncated payload — missing
/// a required key — fails closed with `EventStreamError::Json`. Nullable
/// fields use `serde_json::Value` (without `#[serde(default)]`) so the
/// key must be present even when its value is `null`.
#[derive(Debug, Deserialize)]
struct TurnView {
    id: String,
    #[allow(dead_code)]
    status: serde_json::Value,
    #[allow(dead_code)]
    items: serde_json::Value,
    #[serde(rename = "itemsView")]
    #[allow(dead_code)]
    items_view: serde_json::Value,
    // Nullable per schema, but the key must exist.
    #[allow(dead_code)]
    error: serde_json::Value,
    #[serde(rename = "startedAt")]
    #[allow(dead_code)]
    started_at: serde_json::Value,
    #[serde(rename = "completedAt")]
    #[allow(dead_code)]
    completed_at: serde_json::Value,
}

/// Structural canary for `account/rateLimits/updated`. The schema is
/// `{ rateLimits: RateLimitSnapshot }`. Every field on the inner
/// snapshot is nullable, so the only fixed structural assertion is
/// that the `rateLimits` key exists at all.
#[derive(Debug, Deserialize)]
struct RateLimitsEnvelope {
    #[serde(rename = "rateLimits")]
    #[allow(dead_code)]
    rate_limits: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct ErrorNotification {
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(rename = "turnId")]
    turn_id: String,
    error: ErrorPayload,
    #[serde(rename = "willRetry")]
    will_retry: bool,
}

#[derive(Debug, Deserialize)]
struct ErrorPayload {
    message: String,
}

#[derive(Debug, Deserialize)]
struct TokenUsageNotification {
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(rename = "turnId")]
    turn_id: String,
    #[serde(rename = "tokenUsage")]
    token_usage: ThreadTokenUsage,
}

/// Mirrors `schemas/codex/current/v2/ThreadTokenUsage.ts`:
/// `{ total: TokenUsageBreakdown, last: TokenUsageBreakdown, modelContextWindow: number | null }`.
/// `last` and `modelContextWindow` act as structural canaries even though
/// only `total` is surfaced — a truncated payload missing either should
/// fail loud per the parser contract.
#[derive(Debug, Deserialize)]
struct ThreadTokenUsage {
    total: TokenUsageBreakdown,
    #[allow(dead_code)]
    last: TokenUsageBreakdown,
    #[serde(rename = "modelContextWindow")]
    #[allow(dead_code)]
    model_context_window: Option<u64>,
}

/// Mirrors `schemas/codex/current/v2/TokenUsageBreakdown.ts`. Only the
/// fields Cadenza decisions consume are extracted; the rest are ignored
/// but tolerated.
#[derive(Debug, Deserialize)]
struct TokenUsageBreakdown {
    #[serde(rename = "inputTokens")]
    input_tokens: u64,
    #[serde(rename = "outputTokens")]
    output_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct AgentMessageDelta {
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(rename = "turnId")]
    turn_id: String,
    delta: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replay fixture: a successful turn from start to completion. Each
    /// JSONL line mirrors the v2 schema — `turn.{id,status}` are both
    /// required by `TurnView`; `tokenUsage` carries the nested
    /// `total`/`last` breakdowns.
    const SUCCESSFUL_TURN_JSONL: &str = r#"{"jsonrpc":"2.0","method":"thread/started","params":{"thread":{"id":"thr_42"}}}
{"jsonrpc":"2.0","method":"turn/started","params":{"threadId":"thr_42","turn":{"id":"turn_1","status":"running","items":[],"itemsView":{},"error":null,"startedAt":null,"completedAt":null}}}
{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"thr_42","turnId":"turn_1","delta":"Hello "}}
{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"thr_42","turnId":"turn_1","delta":"world"}}
{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"threadId":"thr_42","turnId":"turn_1","tokenUsage":{"total":{"totalTokens":49,"inputTokens":42,"cachedInputTokens":0,"outputTokens":7,"reasoningOutputTokens":0},"last":{"totalTokens":49,"inputTokens":42,"cachedInputTokens":0,"outputTokens":7,"reasoningOutputTokens":0},"modelContextWindow":128000}}}
{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"thr_42","turn":{"id":"turn_1","status":"completed","items":[],"itemsView":{},"error":null,"startedAt":1,"completedAt":2}}}"#;

    /// Replay fixture: a turn that fails with retry recommended.
    const FAILED_TURN_JSONL: &str = r#"{"jsonrpc":"2.0","method":"thread/started","params":{"thread":{"id":"thr_43"}}}
{"jsonrpc":"2.0","method":"turn/started","params":{"threadId":"thr_43","turn":{"id":"turn_2","status":"running","items":[],"itemsView":{},"error":null,"startedAt":null,"completedAt":null}}}
{"jsonrpc":"2.0","method":"error","params":{"threadId":"thr_43","turnId":"turn_2","error":{"message":"upstream timeout"},"willRetry":true}}"#;

    /// Replay fixture: a successful turn that also includes a rate-limit
    /// observation and one unknown method that must pass through as Other.
    const SUCCESSFUL_TURN_WITH_RATE_LIMITS_AND_UNKNOWN: &str = r#"{"jsonrpc":"2.0","method":"thread/started","params":{"thread":{"id":"thr_44"}}}
{"jsonrpc":"2.0","method":"account/rateLimits/updated","params":{"rateLimits":{"limitId":"rpm","limitName":"per_minute","primary":null,"secondary":null,"credits":null,"planType":null,"rateLimitReachedType":null}}}
{"jsonrpc":"2.0","method":"future/unknown/event","params":{"opaque":true}}
{"jsonrpc":"2.0","method":"turn/started","params":{"threadId":"thr_44","turn":{"id":"turn_3","status":"running","items":[],"itemsView":{},"error":null,"startedAt":null,"completedAt":null}}}
{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"thr_44","turn":{"id":"turn_3","status":"completed","items":[],"itemsView":{},"error":null,"startedAt":1,"completedAt":2}}}"#;

    fn parse_stream(s: &str) -> Vec<TurnEvent> {
        s.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| parse_notification_line(l).expect("parse"))
            .collect()
    }

    #[test]
    fn replay_fixture_successful_turn() {
        let events = parse_stream(SUCCESSFUL_TURN_JSONL);
        assert_eq!(events.len(), 6);
        assert!(
            matches!(events[0], TurnEvent::ThreadStarted { ref thread_id } if thread_id == "thr_42")
        );
        assert!(
            matches!(events[1], TurnEvent::TurnStarted { ref thread_id, ref turn_id } if thread_id == "thr_42" && turn_id == "turn_1")
        );
        assert!(
            matches!(events[2], TurnEvent::AgentMessageDelta { ref delta, .. } if delta == "Hello ")
        );
        match &events[4] {
            TurnEvent::TokenUsage {
                input_tokens,
                output_tokens,
                ..
            } => {
                assert_eq!(*input_tokens, 42);
                assert_eq!(*output_tokens, 7);
            }
            other => panic!("expected TokenUsage, got {other:?}"),
        }
        assert!(
            matches!(events[5], TurnEvent::TurnCompleted { ref turn_id, .. } if turn_id == "turn_1")
        );
    }

    #[test]
    fn replay_fixture_failed_turn() {
        let events = parse_stream(FAILED_TURN_JSONL);
        assert_eq!(events.len(), 3);
        match &events[2] {
            TurnEvent::Error {
                thread_id,
                turn_id,
                message,
                will_retry,
            } => {
                assert_eq!(thread_id, "thr_43");
                assert_eq!(turn_id, "turn_2");
                assert_eq!(message, "upstream timeout");
                assert!(will_retry);
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn unknown_event_passes_through_as_other() {
        let events = parse_stream(SUCCESSFUL_TURN_WITH_RATE_LIMITS_AND_UNKNOWN);
        let unknown = events
            .iter()
            .find(|e| matches!(e, TurnEvent::Other { .. }))
            .expect("expected an Other event");
        match unknown {
            TurnEvent::Other { method, .. } => assert_eq!(method, "future/unknown/event"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn rate_limits_classified_as_rate_limits_updated() {
        let events = parse_stream(SUCCESSFUL_TURN_WITH_RATE_LIMITS_AND_UNKNOWN);
        let rl = events
            .iter()
            .find(|e| matches!(e, TurnEvent::RateLimitsUpdated { .. }))
            .expect("expected RateLimitsUpdated");
        match rl {
            TurnEvent::RateLimitsUpdated { params } => {
                assert!(
                    params.get("rateLimits").is_some(),
                    "params should carry rateLimits envelope: {params}",
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn rate_limits_with_missing_envelope_is_typed_error() {
        // Truncated payload — schema requires `rateLimits`.
        let line = r#"{"jsonrpc":"2.0","method":"account/rateLimits/updated","params":{}}"#;
        let err = parse_notification_line(line).unwrap_err();
        assert!(matches!(err, EventStreamError::Json(_)), "got {err:?}");
    }

    #[test]
    fn token_usage_with_missing_last_breakdown_is_typed_error() {
        // The v2 schema requires both `total` and `last`; this fixture
        // provides only `total`. Must fail loud.
        let line = r#"{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"threadId":"t","turnId":"u","tokenUsage":{"total":{"totalTokens":1,"inputTokens":1,"cachedInputTokens":0,"outputTokens":0,"reasoningOutputTokens":0},"modelContextWindow":null}}}"#;
        let err = parse_notification_line(line).unwrap_err();
        assert!(matches!(err, EventStreamError::Json(_)), "got {err:?}");
    }

    #[test]
    fn turn_started_missing_nullable_required_key_is_typed_error() {
        // Schema requires `error` to be present (can be null). A turn
        // payload missing the key entirely is truncated.
        let line = r#"{"jsonrpc":"2.0","method":"turn/started","params":{"threadId":"t","turn":{"id":"u","status":"running","items":[],"itemsView":{},"startedAt":null,"completedAt":null}}}"#;
        let err = parse_notification_line(line).unwrap_err();
        assert!(matches!(err, EventStreamError::Json(_)), "got {err:?}");
    }

    #[test]
    fn turn_started_with_status_but_no_items_is_typed_error() {
        // `items` and `itemsView` are required by the v2 Turn schema in
        // addition to `id`/`status`. A turn payload that ships only
        // `{id, status}` is still considered truncated.
        let line = r#"{"jsonrpc":"2.0","method":"turn/started","params":{"threadId":"t","turn":{"id":"u","status":"running"}}}"#;
        let err = parse_notification_line(line).unwrap_err();
        assert!(matches!(err, EventStreamError::Json(_)), "got {err:?}");
    }

    #[test]
    fn malformed_json_returns_typed_error() {
        let err = parse_notification_line("{not json").unwrap_err();
        assert!(matches!(err, EventStreamError::Json(_)));
    }

    #[test]
    fn missing_method_returns_typed_error() {
        let err = parse_notification_line(r#"{"jsonrpc":"2.0","params":{}}"#).unwrap_err();
        assert!(
            matches!(err, EventStreamError::MissingMethod),
            "got {err:?}"
        );
    }

    #[test]
    fn token_usage_with_missing_nested_breakdown_is_typed_error() {
        // Per the v2 schema `tokenUsage.total.{inputTokens,outputTokens}`
        // are required. Sending `{tokenUsage: {}}` is a truncated payload
        // and must fail loud, not silently report zeros.
        let line = r#"{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"threadId":"t","turnId":"u","tokenUsage":{}}}"#;
        let err = parse_notification_line(line).unwrap_err();
        assert!(matches!(err, EventStreamError::Json(_)), "got {err:?}");
    }

    #[test]
    fn token_usage_extracts_total_breakdown_not_last() {
        // Boundary: `total` and `last` carry different counters. The
        // parser must surface the cumulative `total.*`, not the most-
        // recent-delta `last.*`.
        let line = r#"{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"threadId":"t","turnId":"u","tokenUsage":{"total":{"totalTokens":150,"inputTokens":100,"cachedInputTokens":0,"outputTokens":50,"reasoningOutputTokens":0},"last":{"totalTokens":30,"inputTokens":20,"cachedInputTokens":0,"outputTokens":10,"reasoningOutputTokens":0},"modelContextWindow":128000}}}"#;
        match parse_notification_line(line).unwrap() {
            TurnEvent::TokenUsage {
                input_tokens,
                output_tokens,
                ..
            } => {
                assert_eq!(
                    input_tokens, 100,
                    "expected total.inputTokens (100), not last.inputTokens",
                );
                assert_eq!(
                    output_tokens, 50,
                    "expected total.outputTokens (50), not last.outputTokens",
                );
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn turn_started_with_missing_status_is_typed_error() {
        // Truncated `turn` object (only `id`, no required `status`) must
        // fail loud per the documented "known method, unexpected shape ->
        // typed JSON error" rule.
        let line = r#"{"jsonrpc":"2.0","method":"turn/started","params":{"threadId":"t","turn":{"id":"u"}}}"#;
        let err = parse_notification_line(line).unwrap_err();
        assert!(matches!(err, EventStreamError::Json(_)), "got {err:?}");
    }
}
