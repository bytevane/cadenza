//! HTTP transport for the Linear GraphQL endpoint. Used by the real
//! integration smoke (#23) and any future production wiring. Tests
//! continue to use the `MockTransport` in `lib::tests`.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use crate::{LinearTransport, TrackerError};

#[derive(Debug, Clone)]
pub struct HttpLinearTransport {
    endpoint: String,
    token: String,
    client: Client,
}

impl HttpLinearTransport {
    /// `token` is the operator's Linear API token. It is never logged
    /// — the orchestrator should register it with the obs scrubber
    /// alongside any other workflow-known secrets.
    pub fn new(
        endpoint: impl Into<String>,
        token: impl Into<String>,
    ) -> Result<Self, TrackerError> {
        let client = Client::builder()
            .build()
            .map_err(|e| TrackerError::Transport(e.to_string()))?;
        Ok(Self {
            endpoint: endpoint.into(),
            token: token.into(),
            client,
        })
    }
}

#[async_trait]
impl LinearTransport for HttpLinearTransport {
    async fn execute(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<serde_json::Value, TrackerError> {
        let body = json!({
            "query": query,
            "variables": variables,
        });
        let resp = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .map_err(|e| TrackerError::Transport(e.to_string()))?;
        let status = resp.status();
        // 429 = rate limited. Surface a typed variant so the
        // orchestrator (#19) can branch on it.
        if status.as_u16() == 429 {
            let retry_hint = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unspecified")
                .to_string();
            return Err(TrackerError::RateLimited(retry_hint));
        }
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(TrackerError::Upstream(format!(
                "HTTP {} from Linear: {}",
                status,
                text.chars().take(300).collect::<String>(),
            )));
        }
        let envelope: GraphqlEnvelope = resp
            .json()
            .await
            .map_err(|e| TrackerError::InvalidResponse(format!("json decode: {e}")))?;
        if let Some(errors) = envelope.errors {
            if !errors.is_empty() {
                let joined = errors
                    .iter()
                    .map(|e| e.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(TrackerError::Upstream(joined));
            }
        }
        envelope
            .data
            .ok_or_else(|| TrackerError::InvalidResponse("missing GraphQL `data` envelope".into()))
    }
}

#[derive(Debug, Deserialize)]
struct GraphqlEnvelope {
    data: Option<serde_json::Value>,
    #[serde(default)]
    errors: Option<Vec<GraphqlError>>,
}

#[derive(Debug, Deserialize)]
struct GraphqlError {
    message: String,
}
