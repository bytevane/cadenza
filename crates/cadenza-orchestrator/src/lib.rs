use std::collections::{BTreeMap, BTreeSet};

use cadenza_core::{RunAttempt, RunStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeState {
    pub poll_interval_ms: u64,
    pub max_concurrent_agents: usize,
    pub running: BTreeMap<String, RunningEntry>,
    pub claimed: BTreeSet<String>,
    pub retry_attempts: BTreeMap<String, RetryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningEntry {
    pub attempt: RunAttempt,
    pub session_id: Option<String>,
    pub last_event: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryEntry {
    pub issue_id: String,
    pub identifier: String,
    pub attempt: u32,
    pub due_at_ms: u64,
    pub error: Option<String>,
}

impl RuntimeState {
    pub fn new(poll_interval_ms: u64, max_concurrent_agents: usize) -> Self {
        Self {
            poll_interval_ms,
            max_concurrent_agents,
            ..Self::default()
        }
    }

    pub fn has_capacity(&self) -> bool {
        self.running.len() < self.max_concurrent_agents
    }

    pub fn is_claimed(&self, issue_id: &str) -> bool {
        self.claimed.contains(issue_id)
    }

    pub fn claim(&mut self, issue_id: impl Into<String>) -> bool {
        self.claimed.insert(issue_id.into())
    }

    pub fn start_run(&mut self, issue_id: String, attempt: RunAttempt) {
        self.claimed.insert(issue_id.clone());
        self.running.insert(
            issue_id,
            RunningEntry {
                attempt: RunAttempt {
                    status: RunStatus::Running,
                    ..attempt
                },
                session_id: None,
                last_event: None,
            },
        );
    }
}
