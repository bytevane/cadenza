//! Linear tracker read adapter for Cadenza.
//!
//! `IssueTrackerClient` is the stable trait the orchestrator depends on.
//! `LinearClient` is the GraphQL implementation; it is parameterised
//! over a `LinearTransport` so tests can stub the HTTP layer with mock
//! responses without spinning up a server. Tracker writes are
//! intentionally NOT exposed here — they go through the
//! `host-linear` Wasm capability per `ARCHITECTURE.md`.

pub mod queries;

use std::sync::Arc;

use async_trait::async_trait;
use cadenza_core::{BlockerRef, Issue};
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("linear token is missing")]
    MissingToken,
    #[error("upstream tracker error: {0}")]
    Upstream(String),
    #[error("upstream is rate-limited (retry hint: {0})")]
    RateLimited(String),
    #[error("invalid tracker response: {0}")]
    InvalidResponse(String),
    #[error("transport error: {0}")]
    Transport(String),
}

/// Minimal tracker contract required by the Symphony-style orchestrator.
/// Implementations normalize Linear payloads into stable `Issue` records;
/// the orchestrator never sees GraphQL JSON shapes.
#[async_trait]
pub trait IssueTrackerClient: Send + Sync {
    async fn fetch_candidate_issues(&self) -> Result<Vec<Issue>, TrackerError>;
    async fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<Issue>, TrackerError>;
    async fn fetch_issue_states_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<(String, String)>, TrackerError>;
}

/// Async GraphQL transport. `execute` issues a single `query` with
/// `variables` and returns the raw `data` payload. Implementations are
/// responsible for surfacing rate-limit hints and upstream errors as
/// the appropriate `TrackerError` variants.
#[async_trait]
pub trait LinearTransport: Send + Sync {
    async fn execute(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<serde_json::Value, TrackerError>;
}

#[derive(Debug, Clone)]
pub struct LinearClientConfig {
    pub endpoint: String,
    pub project_slug_id: Option<String>,
    pub token_env: String,
    pub page_size: u32,
}

impl Default for LinearClientConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://api.linear.app/graphql".to_string(),
            project_slug_id: None,
            token_env: "LINEAR_API_KEY".to_string(),
            page_size: 50,
        }
    }
}

#[derive(Clone)]
pub struct LinearClient<T: LinearTransport> {
    pub config: LinearClientConfig,
    pub transport: Arc<T>,
}

impl<T: LinearTransport> LinearClient<T> {
    pub fn new(config: LinearClientConfig, transport: T) -> Self {
        Self {
            config,
            transport: Arc::new(transport),
        }
    }

    async fn paginate<F>(
        &self,
        query: &str,
        base_vars: serde_json::Value,
        mut extract: F,
    ) -> Result<Vec<Issue>, TrackerError>
    where
        F: FnMut(serde_json::Value) -> Result<(Vec<Issue>, PageInfo), TrackerError>,
    {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut vars = base_vars.clone();
            if let Some(c) = &cursor {
                vars["after"] = serde_json::Value::String(c.clone());
            }
            let payload = self.transport.execute(query, vars).await?;
            let (page, info) = extract(payload)?;
            out.extend(page);
            if !info.has_next_page {
                break;
            }
            cursor = info.end_cursor;
            if cursor.is_none() {
                break;
            }
        }
        Ok(out)
    }
}

#[async_trait]
impl<T: LinearTransport + 'static> IssueTrackerClient for LinearClient<T> {
    async fn fetch_candidate_issues(&self) -> Result<Vec<Issue>, TrackerError> {
        let project = self.config.project_slug_id.clone().ok_or_else(|| {
            TrackerError::Upstream("workflow tracker.project_slug_id required".into())
        })?;
        let base = serde_json::json!({
            "projectId": project,
            "first": self.config.page_size,
        });
        self.paginate(queries::CANDIDATE_ISSUES, base, extract_issues_page)
            .await
    }

    async fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<Issue>, TrackerError> {
        let base = serde_json::json!({
            "states": states,
            "first": self.config.page_size,
        });
        self.paginate(queries::ISSUES_BY_STATES, base, extract_issues_page)
            .await
    }

    async fn fetch_issue_states_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<(String, String)>, TrackerError> {
        let payload = self
            .transport
            .execute(
                queries::ISSUE_STATES_BY_IDS,
                serde_json::json!({ "ids": ids }),
            )
            .await?;
        extract_state_pairs(payload)
    }
}

#[derive(Debug, Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor", default)]
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IssueNode {
    id: String,
    identifier: String,
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    priority: Option<i64>,
    #[serde(default)]
    state: IssueState,
    #[serde(rename = "branchName", default)]
    branch_name: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    labels: LabelConnection,
    #[serde(rename = "createdAt", default)]
    created_at: Option<String>,
    #[serde(rename = "updatedAt", default)]
    updated_at: Option<String>,
    #[serde(rename = "inverseRelations", default)]
    inverse_relations: BlockerConnection,
}

#[derive(Debug, Default, Deserialize)]
struct IssueState {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct LabelConnection {
    #[serde(default)]
    nodes: Vec<LabelNode>,
}

#[derive(Debug, Deserialize)]
struct LabelNode {
    name: String,
}

#[derive(Debug, Default, Deserialize)]
struct BlockerConnection {
    #[serde(default)]
    nodes: Vec<BlockerNode>,
}

#[derive(Debug, Deserialize)]
struct BlockerNode {
    issue: Option<BlockerIssue>,
}

#[derive(Debug, Deserialize)]
struct BlockerIssue {
    id: Option<String>,
    identifier: Option<String>,
    state: Option<IssueState>,
}

#[derive(Debug, Deserialize)]
struct PageEnvelope {
    nodes: Vec<IssueNode>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

fn extract_issues_page(payload: serde_json::Value) -> Result<(Vec<Issue>, PageInfo), TrackerError> {
    let connection = payload
        .pointer("/issues")
        .ok_or_else(|| TrackerError::InvalidResponse("missing data.issues".into()))?;
    let envelope: PageEnvelope = serde_json::from_value(connection.clone())
        .map_err(|e| TrackerError::InvalidResponse(format!("issues page parse: {e}")))?;
    let issues = envelope.nodes.into_iter().map(normalize).collect();
    Ok((issues, envelope.page_info))
}

fn extract_state_pairs(payload: serde_json::Value) -> Result<Vec<(String, String)>, TrackerError> {
    let arr = payload
        .pointer("/issues/nodes")
        .ok_or_else(|| TrackerError::InvalidResponse("missing data.issues.nodes".into()))?
        .as_array()
        .ok_or_else(|| TrackerError::InvalidResponse("data.issues.nodes is not an array".into()))?
        .clone();
    let mut out = Vec::with_capacity(arr.len());
    for node in arr {
        let id = node
            .pointer("/id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TrackerError::InvalidResponse("issue node missing id".into()))?
            .to_string();
        let state = node
            .pointer("/state/name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TrackerError::InvalidResponse("issue node missing state.name".into()))?
            .to_string();
        out.push((id, state));
    }
    Ok(out)
}

fn normalize(node: IssueNode) -> Issue {
    Issue {
        id: node.id,
        identifier: node.identifier,
        title: node.title,
        description: node.description,
        priority: node.priority,
        state: node.state.name.unwrap_or_else(|| "unknown".to_string()),
        branch_name: node.branch_name,
        url: node.url,
        labels: node.labels.nodes.into_iter().map(|l| l.name).collect(),
        blocked_by: node
            .inverse_relations
            .nodes
            .into_iter()
            .filter_map(|rel| rel.issue)
            .map(|i| BlockerRef {
                id: i.id,
                identifier: i.identifier,
                state: i.state.and_then(|s| s.name),
            })
            .collect(),
        created_at: node.created_at,
        updated_at: node.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// In-memory transport that returns canned responses keyed by call
    /// order. Lets tests assert pagination, error mapping, and missing
    /// optional fields without standing up an HTTP server.
    struct MockTransport {
        responses: Mutex<Vec<Result<serde_json::Value, TrackerError>>>,
        seen: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl MockTransport {
        fn new(responses: Vec<Result<serde_json::Value, TrackerError>>) -> Self {
            Self {
                responses: Mutex::new(responses),
                seen: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(String, serde_json::Value)> {
            self.seen.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LinearTransport for MockTransport {
        async fn execute(
            &self,
            query: &str,
            variables: serde_json::Value,
        ) -> Result<serde_json::Value, TrackerError> {
            self.seen
                .lock()
                .unwrap()
                .push((query.to_string(), variables));
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                return Err(TrackerError::Transport("mock ran out of responses".into()));
            }
            responses.remove(0)
        }
    }

    fn issues_page(ids: &[&str], has_next: bool, end_cursor: Option<&str>) -> serde_json::Value {
        let nodes: Vec<_> = ids
            .iter()
            .map(|i| {
                serde_json::json!({
                    "id": format!("issue-{i}"),
                    "identifier": format!("CAD-{i}"),
                    "title": format!("Issue {i}"),
                    "description": null,
                    "priority": 1,
                    "state": { "name": "todo" },
                    "branchName": format!("feat/cad-{i}"),
                    "url": format!("https://linear.app/issue/{i}"),
                    "labels": { "nodes": [{ "name": "priority:P0" }] },
                    "createdAt": "2026-05-22T00:00:00Z",
                    "updatedAt": "2026-05-22T00:00:00Z",
                    "inverseRelations": { "nodes": [] }
                })
            })
            .collect();
        serde_json::json!({
            "issues": {
                "nodes": nodes,
                "pageInfo": {
                    "hasNextPage": has_next,
                    "endCursor": end_cursor.map(|s| s.to_string())
                }
            }
        })
    }

    fn cfg() -> LinearClientConfig {
        LinearClientConfig {
            project_slug_id: Some("CAD".into()),
            page_size: 2,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn fetches_single_page_when_no_next() {
        let transport = MockTransport::new(vec![Ok(issues_page(&["1", "2"], false, None))]);
        let client = LinearClient::new(cfg(), transport);
        let issues = client.fetch_candidate_issues().await.unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].identifier, "CAD-1");
        assert_eq!(issues[1].labels, vec!["priority:P0".to_string()]);
    }

    #[tokio::test]
    async fn paginates_until_has_next_is_false() {
        let transport = MockTransport::new(vec![
            Ok(issues_page(&["1", "2"], true, Some("cursor-1"))),
            Ok(issues_page(&["3"], false, None)),
        ]);
        let client = LinearClient::new(cfg(), transport);
        let issues = client.fetch_candidate_issues().await.unwrap();
        assert_eq!(issues.len(), 3);
    }

    #[tokio::test]
    async fn pagination_carries_end_cursor_into_after_var() {
        let transport_arc = Arc::new(MockTransport::new(vec![
            Ok(issues_page(&["1"], true, Some("cursor-A"))),
            Ok(issues_page(&["2"], false, None)),
        ]));
        let client = LinearClient {
            config: cfg(),
            transport: transport_arc.clone(),
        };
        client.fetch_candidate_issues().await.unwrap();
        let calls = transport_arc.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].1.get("after").is_none());
        assert_eq!(
            calls[1].1.get("after").and_then(|v| v.as_str()),
            Some("cursor-A"),
        );
    }

    #[tokio::test]
    async fn missing_optional_fields_become_default() {
        let payload = serde_json::json!({
            "issues": {
                "nodes": [{
                    "id": "issue-x",
                    "identifier": "CAD-X",
                    "title": "Bare",
                    "state": { "name": "todo" }
                }],
                "pageInfo": { "hasNextPage": false, "endCursor": null }
            }
        });
        let transport = MockTransport::new(vec![Ok(payload)]);
        let client = LinearClient::new(cfg(), transport);
        let issues = client.fetch_candidate_issues().await.unwrap();
        assert_eq!(issues.len(), 1);
        let issue = &issues[0];
        assert_eq!(issue.identifier, "CAD-X");
        assert!(issue.description.is_none());
        assert!(issue.url.is_none());
        assert!(issue.branch_name.is_none());
        assert!(issue.labels.is_empty());
        assert!(issue.blocked_by.is_empty());
        assert!(issue.created_at.is_none());
    }

    #[tokio::test]
    async fn upstream_error_propagates_as_typed_variant() {
        let transport = MockTransport::new(vec![Err(TrackerError::Upstream(
            "INTERNAL_SERVER_ERROR".into(),
        ))]);
        let client = LinearClient::new(cfg(), transport);
        let err = client.fetch_candidate_issues().await.unwrap_err();
        assert!(matches!(err, TrackerError::Upstream(ref m) if m.contains("INTERNAL")));
    }

    #[tokio::test]
    async fn rate_limit_propagates_as_typed_variant() {
        let transport = MockTransport::new(vec![Err(TrackerError::RateLimited(
            "retry-after 30s".into(),
        ))]);
        let client = LinearClient::new(cfg(), transport);
        let err = client.fetch_candidate_issues().await.unwrap_err();
        assert!(matches!(err, TrackerError::RateLimited(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn fetch_issues_by_states_passes_state_list() {
        let transport_arc = Arc::new(MockTransport::new(vec![Ok(issues_page(
            &["1"],
            false,
            None,
        ))]));
        let client = LinearClient {
            config: cfg(),
            transport: transport_arc.clone(),
        };
        let states = vec!["todo".to_string(), "in progress".to_string()];
        let issues = client.fetch_issues_by_states(&states).await.unwrap();
        assert_eq!(issues.len(), 1);
        let calls = transport_arc.calls();
        assert_eq!(calls[0].0, queries::ISSUES_BY_STATES);
        assert_eq!(
            calls[0]
                .1
                .get("states")
                .and_then(|v| v.as_array())
                .unwrap()
                .len(),
            2,
        );
    }

    #[tokio::test]
    async fn fetch_issue_states_by_ids_returns_pairs() {
        let payload = serde_json::json!({
            "issues": {
                "nodes": [
                    { "id": "a", "state": { "name": "in progress" } },
                    { "id": "b", "state": { "name": "done" } }
                ]
            }
        });
        let transport = MockTransport::new(vec![Ok(payload)]);
        let client = LinearClient::new(cfg(), transport);
        let pairs = client
            .fetch_issue_states_by_ids(&["a".into(), "b".into()])
            .await
            .unwrap();
        assert_eq!(
            pairs,
            vec![
                ("a".into(), "in progress".into()),
                ("b".into(), "done".into())
            ]
        );
    }

    #[tokio::test]
    async fn invalid_envelope_is_typed_error() {
        let payload = serde_json::json!({ "other": {} });
        let transport = MockTransport::new(vec![Ok(payload)]);
        let client = LinearClient::new(cfg(), transport);
        let err = client.fetch_candidate_issues().await.unwrap_err();
        assert!(
            matches!(err, TrackerError::InvalidResponse(_)),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn fetch_candidate_issues_requires_project_slug() {
        let transport = MockTransport::new(vec![]);
        let mut config = cfg();
        config.project_slug_id = None;
        let client = LinearClient::new(config, transport);
        let err = client.fetch_candidate_issues().await.unwrap_err();
        assert!(
            matches!(err, TrackerError::Upstream(ref m) if m.contains("project_slug_id")),
            "got {err:?}",
        );
    }

    #[tokio::test]
    async fn missing_token_error_variant_exists() {
        // Document that MissingToken is reachable from a transport impl
        // that has no token in env. Smoke-check by constructing one.
        let err = TrackerError::MissingToken;
        assert_eq!(err.to_string(), "linear token is missing");
    }
}
