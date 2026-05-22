//! Centralized secret scrubber.
//!
//! `Scrubber` combines two redaction modes:
//!
//! 1. **Key-shape detection** (`looks_secret`) — catches any field whose
//!    key contains `token` / `secret` / `password` / `authorization` /
//!    `cookie` or ends with `_key`. Used by structured-field redaction
//!    (`redact_key_value`).
//! 2. **Value substring matching** (`with_secrets`) — when the caller
//!    knows a specific value is sensitive (e.g. the operator's Linear
//!    API token), the value is removed by exact-substring replacement
//!    from any output. Longest values are applied first so a shorter
//!    prefix cannot leak the longer secret's suffix.
//!
//! The scrubber preserves enough context for debugging (`KEY=[REDACTED]`,
//! `***REDACTED***` markers in free text) so an operator can still see
//! *what* was redacted without seeing the value.

use std::sync::Arc;

const KEY_VALUE_MARKER: &str = "[REDACTED]";
const FREE_TEXT_MARKER: &str = "***REDACTED***";

/// Reusable scrubber. Cheap to clone via `Arc`.
#[derive(Debug, Clone, Default)]
pub struct Scrubber {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    secrets: Vec<String>,
}

impl Scrubber {
    /// New scrubber with no registered value secrets — only the
    /// key-shape detector fires. Equivalent to `Default`.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Register zero or more value-substring secrets. Empty strings
    /// are filtered (an empty needle would explode the output). The
    /// resulting list is sorted longest-first so a shorter prefix
    /// cannot leak a longer secret's suffix.
    pub fn with_secrets<I: IntoIterator<Item = String>>(secrets: I) -> Self {
        let mut secrets: Vec<String> = secrets.into_iter().filter(|s| !s.is_empty()).collect();
        secrets.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        Self {
            inner: Arc::new(Inner { secrets }),
        }
    }

    /// Number of registered value secrets (test/inspection only).
    pub fn registered_secret_count(&self) -> usize {
        self.inner.secrets.len()
    }

    /// Structured-field redaction. Returns `[REDACTED]` when the key
    /// looks secret-shaped; otherwise passes the value through after
    /// applying value-substring scrubbing.
    pub fn redact_key_value(&self, key: &str, value: &str) -> String {
        if super::looks_secret(key) {
            return KEY_VALUE_MARKER.to_string();
        }
        self.scrub_text(value)
    }

    /// Free-form text scrub. Replaces every occurrence of every
    /// registered secret value with `***REDACTED***`, then applies the
    /// `KEY=VALUE` pattern scrub from `snapshot::scrub_text`. Order
    /// matters: value substrings are removed first so the pattern
    /// matcher cannot see them.
    pub fn scrub_text(&self, text: &str) -> String {
        let mut out = text.to_string();
        for secret in &self.inner.secrets {
            if !secret.is_empty() && out.contains(secret) {
                out = out.replace(secret, FREE_TEXT_MARKER);
            }
        }
        crate::snapshot::scrub_text(&out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_scrubber_passes_clean_text_through() {
        let s = Scrubber::empty();
        assert_eq!(s.scrub_text("hello world"), "hello world");
        assert_eq!(s.redact_key_value("title", "hello"), "hello");
    }

    #[test]
    fn registered_value_is_redacted_from_text() {
        let s = Scrubber::with_secrets(vec!["lr_tok_abc".into()]);
        let out = s.scrub_text("preflight using lr_tok_abc and then more");
        assert!(!out.contains("lr_tok_abc"), "{out}");
        assert!(out.contains("***REDACTED***"));
    }

    #[test]
    fn longest_secret_is_applied_before_shorter_prefix() {
        // Paired-edge: `abc` and `abcdef` both registered. Naive
        // ordering would leak `def`.
        let s = Scrubber::with_secrets(vec!["abc".into(), "abcdef".into()]);
        let out = s.scrub_text("abcdef-and-abc");
        assert!(!out.contains("abcdef"), "{out}");
        assert!(!out.contains("def"), "{out}");
    }

    #[test]
    fn empty_registered_secret_does_not_explode_output() {
        let s = Scrubber::with_secrets(vec!["".into()]);
        assert_eq!(s.scrub_text("plain"), "plain");
        assert_eq!(s.registered_secret_count(), 0);
    }

    #[test]
    fn key_shape_detection_catches_documented_set() {
        let s = Scrubber::empty();
        for key in [
            "LINEAR_API_KEY",
            "GITHUB_TOKEN",
            "secret",
            "OPENAI_SECRET",
            "PASSWORD",
            "Authorization",
            "Cookie",
        ] {
            assert_eq!(
                s.redact_key_value(key, "real_value"),
                "[REDACTED]",
                "key {key} should be redacted",
            );
        }
        // Negative control.
        assert_eq!(s.redact_key_value("title", "real_value"), "real_value");
    }

    #[test]
    fn pattern_redaction_in_free_text() {
        let s = Scrubber::empty();
        let out = s.scrub_text("config LINEAR_API_KEY=lr_secret_xyz boot ok");
        assert!(!out.contains("lr_secret_xyz"), "{out}");
        assert!(out.contains("LINEAR_API_KEY=[REDACTED]"), "{out}");
    }
}
