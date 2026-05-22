pub mod contracts;

use serde::{Deserialize, Serialize};

/// Normalized issue record used by orchestration, prompt rendering, and observability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<i64>,
    pub state: String,
    pub branch_name: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<BlockerRef>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockerRef {
    pub id: Option<String>,
    pub identifier: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunAttempt {
    pub issue_id: String,
    pub issue_identifier: String,
    pub attempt: Option<u32>,
    pub workspace_path: String,
    pub started_at: Option<String>,
    pub status: RunStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pending,
    Starting,
    Running,
    Completed,
    Retrying,
    Stopped,
    Failed,
}

/// Convert a tracker identifier into a deterministic workspace key.
/// Only `[A-Za-z0-9._-]` is preserved; all other characters are replaced by `_`.
pub fn workspace_key(identifier: &str) -> String {
    let key: String = identifier
        .chars()
        .map(|ch| match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-' => ch,
            _ => '_',
        })
        .collect();

    if key.is_empty() { "_".to_string() } else { key }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_key_preserves_allowed_characters() {
        assert_eq!(workspace_key("ABC-123.foo_bar"), "ABC-123.foo_bar");
    }

    #[test]
    fn workspace_key_replaces_unsafe_characters() {
        assert_eq!(workspace_key("../ABC 123/☃"), ".._ABC_123__");
    }
}
