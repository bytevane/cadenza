use cadenza_core::Issue;

#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("linear token is missing")]
    MissingToken,
    #[error("upstream tracker error: {0}")]
    Upstream(String),
    #[error("invalid tracker response: {0}")]
    InvalidResponse(String),
}

/// Minimal tracker contract required by the Symphony-style orchestrator.
/// Implementations should normalize Linear payloads into stable `Issue` records.
pub trait IssueTrackerClient {
    fn fetch_candidate_issues(&self) -> Result<Vec<Issue>, TrackerError>;
    fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<Issue>, TrackerError>;
    fn fetch_issue_states_by_ids(&self, ids: &[String]) -> Result<Vec<(String, String)>, TrackerError>;
}

#[derive(Debug, Clone)]
pub struct LinearClientConfig {
    pub endpoint: String,
    pub project_slug_id: Option<String>,
    pub token_env: String,
}

impl Default for LinearClientConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://api.linear.app/graphql".to_string(),
            project_slug_id: None,
            token_env: "LINEAR_API_KEY".to_string(),
        }
    }
}
