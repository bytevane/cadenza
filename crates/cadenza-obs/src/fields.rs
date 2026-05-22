//! Stable log/field name constants. Every log site in cadenza
//! references these so operators can filter on canonical names.

pub const FIELD_ISSUE_ID: &str = "issue_id";
pub const FIELD_ISSUE_IDENTIFIER: &str = "issue_identifier";
pub const FIELD_SESSION_ID: &str = "session_id";
pub const FIELD_THREAD_ID: &str = "thread_id";
pub const FIELD_TURN_ID: &str = "turn_id";
pub const FIELD_ATTEMPT: &str = "attempt";
pub const FIELD_COMPONENT: &str = "component";
pub const FIELD_PLUGIN_NAME: &str = "plugin_name";
pub const FIELD_TOOL_NAME: &str = "tool_name";
pub const FIELD_SCHEMA_SHA256: &str = "codex_schema_sha256";
pub const FIELD_WIT_PACKAGE: &str = "wit_package";
pub const FIELD_WIT_WORLD: &str = "wit_world";
pub const FIELD_SKIP_REASON: &str = "skip_reason";
pub const FIELD_WORKFLOW_VERSION: &str = "workflow_version";
pub const FIELD_RETRY_DUE_AT_MS: &str = "retry_due_at_ms";
