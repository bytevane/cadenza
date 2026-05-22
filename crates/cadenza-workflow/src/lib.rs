//! Parse and validate Cadenza's repository-owned `WORKFLOW.md` contract.
//!
//! Front matter is a strict typed YAML document — unknown keys are rejected and
//! every numeric/string field has a documented minimum. Defaults live in
//! `Default` impls and `#[serde(default = "…")]` constructors so callers never
//! plug in defaults at the consumption site.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub config: WorkflowConfig,
    pub prompt_template: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowConfig {
    pub tracker: TrackerConfig,
    #[serde(default)]
    pub poll: PollConfig,
    pub workspace: WorkspaceConfig,
    pub codex: CodexConfig,
    pub orchestrator: OrchestratorConfig,
    #[serde(default)]
    pub hooks: HooksConfig,
    #[serde(default)]
    pub prompt: PromptConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackerConfig {
    pub kind: TrackerKind,
    #[serde(default)]
    pub project_slug_id: Option<String>,
    pub token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackerKind {
    Linear,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollConfig {
    #[serde(default = "PollConfig::default_interval_ms")]
    pub interval_ms: u64,
}

impl PollConfig {
    const fn default_interval_ms() -> u64 {
        5_000
    }
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            interval_ms: Self::default_interval_ms(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceConfig {
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexConfig {
    pub command: String,
    #[serde(default)]
    pub schema_sha256: Option<String>,
    #[serde(default = "CodexConfig::default_turn_timeout_ms")]
    pub turn_timeout_ms: u64,
}

impl CodexConfig {
    const fn default_turn_timeout_ms() -> u64 {
        600_000
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestratorConfig {
    #[serde(default = "OrchestratorConfig::default_max_concurrent_agents")]
    pub max_concurrent_agents: u32,
    pub active_states: Vec<String>,
    pub terminal_states: Vec<String>,
}

impl OrchestratorConfig {
    const fn default_max_concurrent_agents() -> u32 {
        1
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HooksConfig {
    #[serde(default)]
    pub after_create: Option<HookCommand>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookCommand {
    pub command: String,
    #[serde(default = "HookCommand::default_timeout_ms")]
    pub timeout_ms: u64,
}

impl HookCommand {
    const fn default_timeout_ms() -> u64 {
        30_000
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptConfig {
    /// Reject `{{ undefined }}` template variables. Always true for now; the
    /// field exists so future workflows can opt out without changing the
    /// schema shape.
    #[serde(default = "PromptConfig::default_strict_undefined")]
    pub strict_undefined: bool,
}

impl PromptConfig {
    const fn default_strict_undefined() -> bool {
        true
    }
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            strict_undefined: Self::default_strict_undefined(),
        }
    }
}

#[derive(Debug, Error)]
pub enum WorkflowError {
    #[error("WORKFLOW.md must start with YAML front matter delimiter `---`")]
    MissingFrontMatter,
    #[error("WORKFLOW.md front matter is not closed with `---`")]
    UnterminatedFrontMatter,
    #[error("front matter YAML parse error: {0}")]
    InvalidYaml(#[from] serde_yaml::Error),
    #[error("validation error: `{field}` {message}")]
    Invalid { field: String, message: String },
}

impl WorkflowError {
    fn invalid(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Invalid {
            field: field.into(),
            message: message.into(),
        }
    }
}

pub fn parse_workflow(input: &str) -> Result<WorkflowDefinition, WorkflowError> {
    let rest = input
        .strip_prefix("---\n")
        .ok_or(WorkflowError::MissingFrontMatter)?;
    let end = rest
        .find("\n---")
        .ok_or(WorkflowError::UnterminatedFrontMatter)?;
    let (front_matter, remainder) = rest.split_at(end);
    let prompt_template = remainder
        .trim_start_matches("\n---")
        .trim_start_matches('\n')
        .trim()
        .to_string();
    let config: WorkflowConfig = serde_yaml::from_str(front_matter)?;
    validate(&config)?;
    Ok(WorkflowDefinition {
        config,
        prompt_template,
    })
}

fn validate(c: &WorkflowConfig) -> Result<(), WorkflowError> {
    if c.tracker.token.trim().is_empty() {
        return Err(WorkflowError::invalid("tracker.token", "must not be empty"));
    }
    require_absolute_path("workspace.root", &c.workspace.root)?;
    if c.codex.command.trim().is_empty() {
        return Err(WorkflowError::invalid("codex.command", "must not be empty"));
    }
    require_positive_u64("codex.turn_timeout_ms", c.codex.turn_timeout_ms)?;
    require_positive_u64("poll.interval_ms", c.poll.interval_ms)?;
    require_positive_u32(
        "orchestrator.max_concurrent_agents",
        c.orchestrator.max_concurrent_agents,
    )?;
    if c.orchestrator.active_states.is_empty() {
        return Err(WorkflowError::invalid(
            "orchestrator.active_states",
            "must contain at least one state",
        ));
    }
    if c.orchestrator.terminal_states.is_empty() {
        return Err(WorkflowError::invalid(
            "orchestrator.terminal_states",
            "must contain at least one state",
        ));
    }
    for state in &c.orchestrator.active_states {
        if c.orchestrator.terminal_states.contains(state) {
            return Err(WorkflowError::invalid(
                "orchestrator",
                format!("state `{state}` cannot be both active and terminal"),
            ));
        }
    }
    if let Some(hook) = &c.hooks.after_create {
        if hook.command.trim().is_empty() {
            return Err(WorkflowError::invalid(
                "hooks.after_create.command",
                "must not be empty",
            ));
        }
        require_positive_u64("hooks.after_create.timeout_ms", hook.timeout_ms)?;
    }
    if !c.prompt.strict_undefined {
        // The renderer in this crate hard-codes `UndefinedBehavior::Strict`.
        // Accepting `strict_undefined: false` would silently ignore the
        // operator's intent — reject until #8 wires it through.
        return Err(WorkflowError::invalid(
            "prompt.strict_undefined",
            "non-strict prompt rendering is not implemented yet (see issue #8)",
        ));
    }
    Ok(())
}

fn require_absolute_path(field: &str, value: &Path) -> Result<(), WorkflowError> {
    if value.as_os_str().is_empty() {
        return Err(WorkflowError::invalid(field, "must not be empty"));
    }
    if !value.is_absolute() {
        return Err(WorkflowError::invalid(field, "must be an absolute path"));
    }
    Ok(())
}

fn require_positive_u64(field: &str, value: u64) -> Result<(), WorkflowError> {
    if value == 0 {
        return Err(WorkflowError::invalid(field, "must be > 0"));
    }
    Ok(())
}

fn require_positive_u32(field: &str, value: u32) -> Result<(), WorkflowError> {
    if value == 0 {
        return Err(WorkflowError::invalid(field, "must be > 0"));
    }
    Ok(())
}

pub fn render_prompt_strict(
    template: &str,
    context: serde_json::Value,
) -> Result<String, minijinja::Error> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.add_template("prompt", template)?;
    env.get_template("prompt")?.render(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MINIMAL_FRONT_MATTER: &str = r#"---
tracker:
  kind: linear
  token: "secret"
workspace:
  root: "/tmp/cadenza/workspaces"
codex:
  command: "codex app-server --listen stdio://"
orchestrator:
  active_states: ["todo"]
  terminal_states: ["done"]
---
prompt body
"#;

    fn minimal() -> WorkflowDefinition {
        parse_workflow(MINIMAL_FRONT_MATTER).expect("minimal workflow parses")
    }

    #[test]
    fn parses_minimal_workflow_with_defaults_applied() {
        let w = minimal();
        assert_eq!(w.config.tracker.kind, TrackerKind::Linear);
        assert_eq!(w.config.tracker.token, "secret");
        assert_eq!(w.config.poll.interval_ms, 5_000);
        assert_eq!(w.config.codex.turn_timeout_ms, 600_000);
        assert!(w.config.codex.schema_sha256.is_none());
        assert_eq!(w.config.orchestrator.max_concurrent_agents, 1);
        assert!(w.config.hooks.after_create.is_none());
        assert!(w.config.prompt.strict_undefined);
        assert_eq!(w.prompt_template, "prompt body");
    }

    #[test]
    fn parses_workflow_example_md() {
        let example = include_str!("../../../WORKFLOW.example.md");
        let w = parse_workflow(example).expect("example workflow parses");
        assert_eq!(w.config.tracker.kind, TrackerKind::Linear);
        assert_eq!(w.config.tracker.project_slug_id.as_deref(), Some("CAD"));
        assert_eq!(w.config.orchestrator.max_concurrent_agents, 2);
        assert_eq!(
            w.config.orchestrator.active_states,
            vec!["todo", "in progress"],
        );
        assert_eq!(w.config.poll.interval_ms, 5_000);
        let hook = w
            .config
            .hooks
            .after_create
            .as_ref()
            .expect("example defines after_create hook");
        assert_eq!(hook.command, "git init");
        assert_eq!(hook.timeout_ms, 30_000);
    }

    #[test]
    fn missing_front_matter_delimiter() {
        let err = parse_workflow("no front matter here").unwrap_err();
        assert!(matches!(err, WorkflowError::MissingFrontMatter));
    }

    #[test]
    fn unterminated_front_matter() {
        let err = parse_workflow("---\nkey: value\nbody without closing delimiter").unwrap_err();
        assert!(matches!(err, WorkflowError::UnterminatedFrontMatter));
    }

    #[test]
    fn invalid_yaml() {
        let err = parse_workflow("---\n: : invalid\n---\nbody").unwrap_err();
        assert!(matches!(err, WorkflowError::InvalidYaml(_)));
    }

    #[test]
    fn unknown_top_level_field_is_rejected() {
        let body =
            MINIMAL_FRONT_MATTER.replace("orchestrator:", "extra_key: surprise\norchestrator:");
        let err = parse_workflow(&body).unwrap_err();
        assert!(matches!(err, WorkflowError::InvalidYaml(_)), "got: {err:?}");
    }

    #[test]
    fn empty_tracker_token_is_invalid() {
        let body = MINIMAL_FRONT_MATTER.replace(r#"token: "secret""#, r#"token: """#);
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, .. } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "tracker.token");
    }

    #[test]
    fn relative_workspace_root_is_invalid() {
        let body = MINIMAL_FRONT_MATTER.replace(
            r#"root: "/tmp/cadenza/workspaces""#,
            r#"root: "relative/path""#,
        );
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, message } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "workspace.root");
        assert!(message.contains("absolute"));
    }

    #[test]
    fn empty_workspace_root_is_invalid() {
        let body =
            MINIMAL_FRONT_MATTER.replace(r#"root: "/tmp/cadenza/workspaces""#, r#"root: """#);
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, .. } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "workspace.root");
    }

    #[test]
    fn empty_codex_command_is_invalid() {
        let body = MINIMAL_FRONT_MATTER.replace(
            r#"command: "codex app-server --listen stdio://""#,
            r#"command: """#,
        );
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, .. } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "codex.command");
    }

    // Boundary law: =N (smallest valid) and =N+1 (just invalid) for every
    // numeric "must be > 0" field. Paired-edges across all four numerics.

    #[test]
    fn zero_turn_timeout_is_invalid_boundary() {
        let body = MINIMAL_FRONT_MATTER.replace(
            r#"command: "codex app-server --listen stdio://""#,
            "command: \"codex app-server --listen stdio://\"\n  turn_timeout_ms: 0",
        );
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, .. } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "codex.turn_timeout_ms");
    }

    #[test]
    fn one_turn_timeout_is_valid_boundary() {
        let body = MINIMAL_FRONT_MATTER.replace(
            r#"command: "codex app-server --listen stdio://""#,
            "command: \"codex app-server --listen stdio://\"\n  turn_timeout_ms: 1",
        );
        let w = parse_workflow(&body).unwrap();
        assert_eq!(w.config.codex.turn_timeout_ms, 1);
    }

    #[test]
    fn zero_poll_interval_is_invalid_boundary() {
        let body = MINIMAL_FRONT_MATTER.replace("codex:", "poll:\n  interval_ms: 0\ncodex:");
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, .. } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "poll.interval_ms");
    }

    #[test]
    fn one_poll_interval_is_valid_boundary() {
        let body = MINIMAL_FRONT_MATTER.replace("codex:", "poll:\n  interval_ms: 1\ncodex:");
        let w = parse_workflow(&body).unwrap();
        assert_eq!(w.config.poll.interval_ms, 1);
    }

    #[test]
    fn zero_max_concurrent_agents_is_invalid_boundary() {
        let body = MINIMAL_FRONT_MATTER.replace(
            "active_states:",
            "max_concurrent_agents: 0\n  active_states:",
        );
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, .. } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "orchestrator.max_concurrent_agents");
    }

    #[test]
    fn one_max_concurrent_agents_is_valid_boundary() {
        let body = MINIMAL_FRONT_MATTER.replace(
            "active_states:",
            "max_concurrent_agents: 1\n  active_states:",
        );
        let w = parse_workflow(&body).unwrap();
        assert_eq!(w.config.orchestrator.max_concurrent_agents, 1);
    }

    #[test]
    fn empty_active_states_is_invalid_boundary() {
        let body = MINIMAL_FRONT_MATTER.replace(r#"active_states: ["todo"]"#, "active_states: []");
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, .. } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "orchestrator.active_states");
    }

    #[test]
    fn single_active_state_is_valid_boundary() {
        // Already the minimal fixture: active_states: ["todo"]
        let w = minimal();
        assert_eq!(w.config.orchestrator.active_states.len(), 1);
    }

    #[test]
    fn empty_terminal_states_is_invalid_boundary() {
        let body =
            MINIMAL_FRONT_MATTER.replace(r#"terminal_states: ["done"]"#, "terminal_states: []");
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, .. } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "orchestrator.terminal_states");
    }

    #[test]
    fn overlapping_active_and_terminal_state_is_invalid() {
        let body = MINIMAL_FRONT_MATTER.replace(
            r#"terminal_states: ["done"]"#,
            r#"terminal_states: ["done", "todo"]"#,
        );
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, message } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "orchestrator");
        assert!(message.contains("todo"));
    }

    #[test]
    fn after_create_hook_with_empty_command_is_invalid() {
        let body = MINIMAL_FRONT_MATTER
            .trim_end_matches("---\nprompt body\n")
            .to_string()
            + "hooks:\n  after_create:\n    command: \"\"\n---\nprompt body\n";
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, .. } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "hooks.after_create.command");
    }

    #[test]
    fn after_create_hook_with_zero_timeout_is_invalid_boundary() {
        let body = MINIMAL_FRONT_MATTER
            .trim_end_matches("---\nprompt body\n")
            .to_string()
            + "hooks:\n  after_create:\n    command: \"git init\"\n    timeout_ms: 0\n---\nprompt body\n";
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, .. } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "hooks.after_create.timeout_ms");
    }

    #[test]
    fn after_create_hook_with_one_timeout_is_valid_boundary() {
        let body = MINIMAL_FRONT_MATTER
            .trim_end_matches("---\nprompt body\n")
            .to_string()
            + "hooks:\n  after_create:\n    command: \"git init\"\n    timeout_ms: 1\n---\nprompt body\n";
        let w = parse_workflow(&body).unwrap();
        let hook = w.config.hooks.after_create.as_ref().unwrap();
        assert_eq!(hook.timeout_ms, 1);
    }

    #[test]
    fn prompt_strict_undefined_false_is_rejected() {
        let body = MINIMAL_FRONT_MATTER
            .trim_end_matches("---\nprompt body\n")
            .to_string()
            + "prompt:\n  strict_undefined: false\n---\nprompt body\n";
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, message } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "prompt.strict_undefined");
        assert!(message.contains("not implemented"));
    }

    #[test]
    fn prompt_strict_undefined_true_is_accepted_explicitly() {
        let body = MINIMAL_FRONT_MATTER
            .trim_end_matches("---\nprompt body\n")
            .to_string()
            + "prompt:\n  strict_undefined: true\n---\nprompt body\n";
        let w = parse_workflow(&body).unwrap();
        assert!(w.config.prompt.strict_undefined);
    }

    #[test]
    fn strict_prompt_fails_on_unknown_variable() {
        let err = render_prompt_strict("{{ missing }}", json!({})).unwrap_err();
        assert_eq!(err.kind(), minijinja::ErrorKind::UndefinedError);
    }
}
