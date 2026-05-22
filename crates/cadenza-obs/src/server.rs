//! Operator-facing HTTP routes.
//!
//! - `GET /api/v1/state` → JSON `RuntimeSnapshot` (redacted).
//! - `GET /api/v1/issues/{issue_identifier}` → `IssueRunningView | RetryView | 404`.
//! - `POST /api/v1/refresh` → asks the orchestrator to re-evaluate;
//!   200 OK on success, never writes orchestrator state.
//!
//! The server defaults to loopback only (`127.0.0.1`); operators
//! who explicitly want LAN visibility must override the bind address
//! and accept the trade-off.

use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use serde::Serialize;

use crate::snapshot::{IssueRunningView, RetryView, RuntimeSnapshot, redact_snapshot};

/// The orchestrator implements this so the obs server can read state
/// without coupling to its concrete type.
#[async_trait]
pub trait SnapshotProvider: Send + Sync + 'static {
    async fn snapshot(&self) -> RuntimeSnapshot;
    async fn refresh(&self) -> Result<(), String>;
}

pub struct ObsAppState<P: SnapshotProvider> {
    pub provider: Arc<P>,
}

impl<P: SnapshotProvider> Clone for ObsAppState<P> {
    fn clone(&self) -> Self {
        Self {
            provider: Arc::clone(&self.provider),
        }
    }
}

impl<P: SnapshotProvider> ObsAppState<P> {
    pub fn new(provider: P) -> Self {
        Self {
            provider: Arc::new(provider),
        }
    }
}

/// Default loopback bind. Operators who want LAN visibility set their own.
pub fn default_bind() -> IpAddr {
    IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
}

/// Build the axum router. The router is generic over the
/// `SnapshotProvider` so tests can pass an in-memory stub.
pub fn router<P: SnapshotProvider>(state: ObsAppState<P>) -> Router {
    Router::new()
        .route("/api/v1/state", get(get_state::<P>))
        .route("/api/v1/issues/:identifier", get(get_issue::<P>))
        .route("/api/v1/refresh", post(post_refresh::<P>))
        .with_state(state)
}

async fn get_state<P: SnapshotProvider>(
    State(state): State<ObsAppState<P>>,
) -> Json<RuntimeSnapshot> {
    let mut snap = state.provider.snapshot().await;
    redact_snapshot(&mut snap);
    Json(snap)
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum IssueView {
    Running(IssueRunningView),
    Retry(RetryView),
}

async fn get_issue<P: SnapshotProvider>(
    State(state): State<ObsAppState<P>>,
    Path(identifier): Path<String>,
) -> impl IntoResponse {
    let mut snap = state.provider.snapshot().await;
    redact_snapshot(&mut snap);
    if let Some(running) = snap
        .running
        .iter()
        .find(|r| r.identifier == identifier || r.issue_id == identifier)
    {
        return (StatusCode::OK, Json(IssueView::Running(running.clone()))).into_response();
    }
    if let Some(retry) = snap
        .retry
        .iter()
        .find(|r| r.identifier == identifier || r.issue_id == identifier)
    {
        return (StatusCode::OK, Json(IssueView::Retry(retry.clone()))).into_response();
    }
    (StatusCode::NOT_FOUND, "issue not found").into_response()
}

#[derive(Debug, Serialize)]
struct RefreshResponse<'a> {
    status: &'a str,
}

async fn post_refresh<P: SnapshotProvider>(
    State(state): State<ObsAppState<P>>,
) -> impl IntoResponse {
    match state.provider.refresh().await {
        Ok(()) => (StatusCode::OK, Json(RefreshResponse { status: "ok" })).into_response(),
        Err(msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RefreshResponse {
                status: msg.as_str(),
            }),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{IssueRunningView, LastReloadView, RetryView};
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    struct StubProvider {
        snapshot: RuntimeSnapshot,
        refresh_count: tokio::sync::Mutex<u32>,
        fail_refresh: bool,
    }

    #[async_trait]
    impl SnapshotProvider for StubProvider {
        async fn snapshot(&self) -> RuntimeSnapshot {
            self.snapshot.clone()
        }
        async fn refresh(&self) -> Result<(), String> {
            *self.refresh_count.lock().await += 1;
            if self.fail_refresh {
                Err("simulated".into())
            } else {
                Ok(())
            }
        }
    }

    fn stub_snapshot() -> RuntimeSnapshot {
        RuntimeSnapshot {
            workflow_version: 7,
            max_concurrent_agents: 2,
            running: vec![IssueRunningView {
                issue_id: "issue-a".into(),
                identifier: "CAD-1".into(),
                attempt: 1,
                thread_id: Some("thr_1".into()),
                turn_id: Some("turn_1".into()),
                last_event: Some("preflight LINEAR_API_KEY=secret_x".into()),
                started_at_ms: Some(1_000),
            }],
            retry: vec![RetryView {
                issue_id: "issue-b".into(),
                identifier: "CAD-2".into(),
                attempt: 2,
                due_at_ms: 5_000,
                reason: Some("upstream timeout".into()),
            }],
            recent_skips: vec![],
            last_reload: Some(LastReloadView {
                at_ms: 0,
                version: 7,
                outcome: "Loaded".into(),
                error: None,
            }),
        }
    }

    fn app() -> Router {
        let state = ObsAppState::new(StubProvider {
            snapshot: stub_snapshot(),
            refresh_count: tokio::sync::Mutex::new(0),
            fail_refresh: false,
        });
        router(state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn get_state_returns_redacted_snapshot() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["workflow_version"], 7);
        let last_event = v["running"][0]["last_event"].as_str().unwrap();
        assert!(!last_event.contains("secret_x"), "leaked: {last_event}");
        assert!(
            last_event.contains("[REDACTED]"),
            "no redaction marker: {last_event}"
        );
    }

    #[tokio::test]
    async fn get_issue_by_identifier_returns_running_view() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/issues/CAD-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["identifier"], "CAD-1");
    }

    #[tokio::test]
    async fn get_issue_by_retry_id() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/issues/CAD-2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["due_at_ms"], 5_000);
    }

    #[tokio::test]
    async fn get_unknown_issue_is_404() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/issues/CAD-99")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn refresh_endpoint_calls_provider_and_returns_ok() {
        let state = ObsAppState::new(StubProvider {
            snapshot: stub_snapshot(),
            refresh_count: tokio::sync::Mutex::new(0),
            fail_refresh: false,
        });
        let counter = Arc::clone(&state.provider);
        let app = router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/refresh")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(*counter.refresh_count.lock().await, 1);
    }

    #[tokio::test]
    async fn refresh_endpoint_surfaces_provider_error() {
        let state = ObsAppState::new(StubProvider {
            snapshot: stub_snapshot(),
            refresh_count: tokio::sync::Mutex::new(0),
            fail_refresh: true,
        });
        let resp = router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/refresh")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn default_bind_is_loopback() {
        let bind = default_bind();
        assert!(bind.is_loopback(), "default bind not loopback: {bind}");
    }
}
