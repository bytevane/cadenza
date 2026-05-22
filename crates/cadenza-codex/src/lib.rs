use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexTransport {
    Stdio,
    UnixSocket { path: Option<String> },
    WebSocket { url: String },
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexAppServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub transport: CodexTransport,
    pub schema_sha256: Option<String>,
}

impl Default for CodexAppServerConfig {
    fn default() -> Self {
        Self {
            command: "codex".to_string(),
            args: vec!["app-server".to_string(), "--listen".to_string(), "stdio://".to_string()],
            transport: CodexTransport::Stdio,
            schema_sha256: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CodexError {
    #[error("unsupported transport for MVP: {0:?}")]
    UnsupportedTransport(CodexTransport),
    #[error("schema hash is not pinned")]
    MissingSchemaHash,
}

pub fn validate_mvp_config(config: &CodexAppServerConfig) -> Result<(), CodexError> {
    match config.transport {
        CodexTransport::Stdio => {}
        ref other => return Err(CodexError::UnsupportedTransport(other.clone())),
    }
    if config.schema_sha256.as_deref().unwrap_or_default().is_empty() {
        return Err(CodexError::MissingSchemaHash);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_uses_stdio_transport() {
        let cfg = CodexAppServerConfig::default();
        assert!(matches!(cfg.transport, CodexTransport::Stdio));
        assert_eq!(cfg.args, ["app-server", "--listen", "stdio://"]);
    }
}
