//! Runtime snapshot model. The orchestrator owns the live state;
//! this module is what gets serialised to JSON for the
//! `GET /api/v1/state` and `GET /api/v1/issues/{id}` routes.
//!
//! Every payload that crosses the HTTP boundary goes through
//! `redact_snapshot` so a misconfigured workflow that put a token
//! into an issue field, error message, or last-event blob still
//! cannot leak the value via the state API.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub workflow_version: u64,
    pub max_concurrent_agents: usize,
    pub running: Vec<IssueRunningView>,
    pub retry: Vec<RetryView>,
    pub recent_skips: Vec<SkipReasonView>,
    pub last_reload: Option<LastReloadView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IssueRunningView {
    pub issue_id: String,
    pub identifier: String,
    pub attempt: u32,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub last_event: Option<String>,
    pub started_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetryView {
    pub issue_id: String,
    pub identifier: String,
    pub attempt: u32,
    pub due_at_ms: u64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkipReasonView {
    pub issue_id: String,
    pub identifier: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LastReloadView {
    pub at_ms: u64,
    pub version: u64,
    pub outcome: String,
    pub error: Option<String>,
}

/// Walk every string field in the snapshot and apply `redact_value`
/// to keys that look secret-shaped. The snapshot doesn't carry any
/// raw-secret fields by design, but free-form strings (`last_event`,
/// `reason`, `error`) can pass through tokens that arrived in
/// upstream logs. This is the last line of defence.
pub fn redact_snapshot(snapshot: &mut RuntimeSnapshot) {
    for running in &mut snapshot.running {
        if let Some(e) = running.last_event.as_mut() {
            *e = scrub_text(e);
        }
    }
    for retry in &mut snapshot.retry {
        if let Some(r) = retry.reason.as_mut() {
            *r = scrub_text(r);
        }
    }
    if let Some(reload) = snapshot.last_reload.as_mut() {
        if let Some(err) = reload.error.as_mut() {
            *err = scrub_text(err);
        }
    }
}

/// Heuristic free-text scrub: replace `KEY=value` substrings (where
/// `KEY` looks secret-shaped) with `KEY=[REDACTED]`. Only the `=`
/// separator is handled — `KEY: value` patterns vary too much (e.g.
/// `authorization: bearer XXX` puts the secret two tokens past the
/// separator) so we conservatively leave them. Structured fields that
/// carry a key/value pair separately should use `redact_value` instead.
///
/// The value boundary is "until end-of-line", not the next whitespace —
/// common credential shapes like `authorization=Bearer <jwt>` or
/// `x-token=foo bar baz` contain internal spaces and stopping at the
/// first whitespace would leak the trailing token material (see #58).
/// Punctuation like `;` / `,` / `&` is intentionally NOT a boundary
/// because opaque secrets can contain those bytes (e.g.
/// `password=abc&def`); stopping there would leak the suffix.
///
/// Whitespace immediately around `=` (`KEY = VALUE` style log dumps)
/// is tolerated: the key scan trims trailing whitespace from `head`
/// before searching for the alnum/underscore run, and leading
/// whitespace from the value is skipped before redaction (see #60).
pub(crate) fn scrub_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let Some(sep_pos) = rest.find('=') else {
            out.push_str(rest);
            break;
        };
        let (head, tail) = rest.split_at(sep_pos);
        let head_trimmed = head.trim_end();
        let trailing_ws_len = head.len() - head_trimmed.len();
        let key_start = head_trimmed
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map(|i| i + 1)
            .unwrap_or(0);
        let key = &head_trimmed[key_start..];
        out.push_str(&head[..key_start]);
        out.push_str(key);
        // Re-emit any whitespace that sat between the key and `=`
        // so the operator-facing output keeps its shape.
        out.push_str(&head[head.len() - trailing_ws_len..]);
        out.push('=');
        let after_sep = &tail[1..];
        if super::looks_secret(key) && !key.is_empty() {
            // Allow whitespace between `=` and the secret value
            // (`KEY = secret`); re-emit it before the marker so the
            // shape of the line is preserved.
            let leading_ws_len = after_sep
                .find(|c: char| !(c == ' ' || c == '\t'))
                .unwrap_or(after_sep.len());
            out.push_str(&after_sep[..leading_ws_len]);
            let value_region = &after_sep[leading_ws_len..];
            let value_end = value_region
                .find(['\n', '\r'])
                .unwrap_or(value_region.len());
            out.push_str("[REDACTED]");
            rest = &value_region[value_end..];
        } else {
            rest = after_sep;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_text_redacts_known_token_keys() {
        let s = scrub_text("foo LINEAR_API_KEY=lr_tok_abc bar");
        assert!(s.contains("LINEAR_API_KEY=[REDACTED]"), "got {s}");
        assert!(!s.contains("lr_tok_abc"));
    }

    #[test]
    fn scrub_text_leaves_colon_separator_alone() {
        // `KEY: value` patterns are deliberately not touched — the
        // bearer-token case (`authorization: bearer XXX`) puts the
        // secret two tokens past the separator and a naive scrub
        // would only catch "bearer". Structured callers should use
        // `redact_value` for these fields instead.
        let s = scrub_text("authorization: bearer ghs_xyz");
        assert_eq!(s, "authorization: bearer ghs_xyz");
    }

    #[test]
    fn scrub_text_leaves_prose_alone() {
        let s = scrub_text("the upstream tracker returned an error");
        assert_eq!(s, "the upstream tracker returned an error");
    }

    #[test]
    fn redact_snapshot_scrubs_running_last_event() {
        let mut snap = RuntimeSnapshot {
            running: vec![IssueRunningView {
                issue_id: "a".into(),
                identifier: "CAD-1".into(),
                attempt: 1,
                thread_id: None,
                turn_id: None,
                last_event: Some("preflight token=ghs_secret".into()),
                started_at_ms: None,
            }],
            ..Default::default()
        };
        redact_snapshot(&mut snap);
        let leaked = snap.running[0]
            .last_event
            .as_ref()
            .map(|s| s.contains("ghs_secret"))
            .unwrap_or(false);
        assert!(!leaked, "leaked: {:?}", snap.running[0].last_event);
    }

    #[test]
    fn redact_snapshot_scrubs_retry_reason() {
        let mut snap = RuntimeSnapshot {
            retry: vec![RetryView {
                issue_id: "a".into(),
                identifier: "CAD-1".into(),
                attempt: 2,
                due_at_ms: 10,
                reason: Some("upstream failed: API_KEY=oops".into()),
            }],
            ..Default::default()
        };
        redact_snapshot(&mut snap);
        assert!(!snap.retry[0].reason.as_ref().unwrap().contains("oops"));
    }

    #[test]
    fn scrub_text_redacts_full_bearer_token_after_whitespace() {
        // Regression for #58: the parser stopped at the first whitespace
        // after `KEY=`, so `authorization=Bearer <token>` only redacted
        // `Bearer` and left the actual JWT in the output.
        let s = scrub_text("authorization=Bearer eyJhbGciOiJIUzI1NiIs.abcdef");
        assert!(
            !s.contains("eyJhbGciOiJIUzI1NiIs.abcdef"),
            "leaked token after Bearer: {s}",
        );
        assert!(!s.contains("Bearer"), "leaked Bearer prefix: {s}");
    }

    #[test]
    fn scrub_text_redacts_multi_token_value_after_secret_key() {
        // Regression for #58: any value with spaces should be redacted
        // in full when the key is secret-shaped.
        let s = scrub_text("x-token=foo bar baz");
        assert!(!s.contains("foo"), "leaked first token: {s}");
        assert!(!s.contains("bar"), "leaked middle token: {s}");
        assert!(!s.contains("baz"), "leaked tail token: {s}");
    }

    #[test]
    fn scrub_text_stops_value_at_newline() {
        // Regression for #58: redact to end of line so non-secret
        // content on the next line is preserved for the operator.
        let s = scrub_text("authorization=Bearer abc xyz\nnext-line content");
        assert!(!s.contains("Bearer"), "leaked secret: {s}");
        assert!(!s.contains("xyz"), "leaked tail token: {s}");
        assert!(
            s.contains("next-line content"),
            "newline boundary lost: {s}",
        );
    }

    #[test]
    fn scrub_text_redacts_entire_cookie_line() {
        // Regression for #58: cookie values can themselves embed
        // session tokens, so the whole cookie value (everything until
        // newline) is redacted. Punctuation like `;` is NOT a boundary
        // because opaque secrets can contain `;` / `,` / `&`.
        let s = scrub_text("cookie=session=v1; lang=en\nplain text");
        assert!(!s.contains("session=v1"), "leaked cookie body: {s}");
        assert!(!s.contains("lang=en"), "leaked tail of cookie line: {s}");
        assert!(s.contains("plain text"), "newline boundary lost: {s}");
    }

    #[test]
    fn scrub_text_handles_whitespace_around_equals() {
        // Regression for #60: `KEY =value` / `KEY = value` produced an
        // empty key string (the parser scanned backwards from `=` and
        // hit the space immediately), so the heuristic missed and the
        // secret leaked.
        for input in [
            "API_KEY =topsecret",
            "API_KEY= topsecret",
            "API_KEY = topsecret",
            "api_key= \"topsecret\"",
        ] {
            let s = scrub_text(input);
            assert!(!s.contains("topsecret"), "leaked secret in {input:?}: {s}",);
            assert!(
                s.contains("[REDACTED]"),
                "no redaction marker in {input:?}: {s}",
            );
        }
        // Sanity: no-space form keeps working.
        let s = scrub_text("API_KEY=topsecret");
        assert!(!s.contains("topsecret"), "regressed no-space form: {s}");
    }

    #[test]
    fn scrub_text_keeps_punctuation_inside_secret_value() {
        // Regression for codex review of #58 fix: opaque secrets can
        // contain `&` / `,` / `;`. The earlier patch made those
        // characters end-of-value markers and leaked the suffix.
        for input in ["password=abc&def", "secret=foo,bar", "token=a;b;c"] {
            let s = scrub_text(input);
            assert!(!s.contains("def"), "leaked `&`-suffix in {input}: {s}");
            assert!(!s.contains("bar"), "leaked `,`-suffix in {input}: {s}");
            assert!(!s.contains("b;c"), "leaked `;`-suffix in {input}: {s}");
        }
    }

    #[test]
    fn redact_snapshot_scrubs_last_reload_error_with_equals_key() {
        // Free-form scrub catches `KEY=VALUE` patterns. The colon
        // variant is intentionally out of scope (see
        // `scrub_text_leaves_colon_separator_alone`); structured
        // callers should pass header-map values through
        // `redact_value` instead.
        let mut snap = RuntimeSnapshot {
            last_reload: Some(LastReloadView {
                at_ms: 0,
                version: 1,
                outcome: "Rejected".into(),
                error: Some("LINEAR_API_KEY=oops_secret_value".into()),
            }),
            ..Default::default()
        };
        redact_snapshot(&mut snap);
        assert!(
            !snap
                .last_reload
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("oops_secret_value"),
        );
    }
}
