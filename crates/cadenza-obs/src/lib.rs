//! Observability state API and structured-log field contract.
//!
//! The HTTP routes here are intentionally **read-only** projections of
//! the orchestrator's `RuntimeState`; the only mutating route is
//! `POST /api/v1/refresh`, which asks the orchestrator to re-evaluate
//! its tick (no state writes from the operator side).
//!
//! Field constants are exported as a stable contract — every log site
//! references them so a downstream operator filter (`jq`, Loki, etc.)
//! can rely on canonical names. `redact_value` and `redact_snapshot`
//! ensure secret-shaped values never leave the host even via the
//! state API.

pub mod fields;
pub mod server;
pub mod snapshot;

pub use fields::*;
pub use server::{ObsAppState, SnapshotProvider, default_bind, router};
pub use snapshot::{
    IssueRunningView, LastReloadView, RetryView, RuntimeSnapshot, SkipReasonView, redact_snapshot,
};

/// Redact a single field by key. Returns `"[REDACTED]"` for any key
/// that looks secret-shaped (token / secret / password / authorization
/// or `_key` suffix); everything else passes through.
pub fn redact_value(key: &str, value: &str) -> String {
    if looks_secret(key) {
        "[REDACTED]".to_string()
    } else {
        value.to_string()
    }
}

/// Internal helper — case-insensitive secret-key check.
pub(crate) fn looks_secret(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("authorization")
        || key.ends_with("_key")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_like_keys() {
        assert_eq!(redact_value("LINEAR_API_KEY", "abc"), "[REDACTED]");
        assert_eq!(redact_value("AUTHORIZATION", "bearer x"), "[REDACTED]");
        assert_eq!(redact_value("github_token", "ghp_xyz"), "[REDACTED]");
        assert_eq!(redact_value("title", "abc"), "abc");
    }
}
