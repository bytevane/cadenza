pub const FIELD_ISSUE_ID: &str = "issue_id";
pub const FIELD_ISSUE_IDENTIFIER: &str = "issue_identifier";
pub const FIELD_SESSION_ID: &str = "session_id";
pub const FIELD_THREAD_ID: &str = "thread_id";
pub const FIELD_TURN_ID: &str = "turn_id";
pub const FIELD_PLUGIN_NAME: &str = "plugin_name";
pub const FIELD_TOOL_NAME: &str = "tool_name";
pub const FIELD_SCHEMA_SHA256: &str = "codex_schema_sha256";
pub const FIELD_WIT_PACKAGE: &str = "wit_package";
pub const FIELD_WIT_WORLD: &str = "wit_world";

pub fn redact_value(key: &str, value: &str) -> String {
    let key = key.to_ascii_lowercase();
    if key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("authorization")
        || key.ends_with("_key")
    {
        "[REDACTED]".to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_like_keys() {
        assert_eq!(redact_value("LINEAR_API_KEY", "abc"), "[REDACTED]");
        assert_eq!(redact_value("title", "abc"), "abc");
    }
}
