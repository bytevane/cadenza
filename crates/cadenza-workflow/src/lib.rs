//! Parse and validate Cadenza's repository-owned `WORKFLOW.md` contract.
//!
//! Front matter is a strict typed YAML document — unknown keys are rejected and
//! every numeric/string field has a documented minimum. Defaults live in
//! `Default` impls and `#[serde(default = "…")]` constructors so callers never
//! plug in defaults at the consumption site.

pub mod source;
pub use source::{
    ReloadEvent, ReloadOutcome, WatchError, WorkflowSource, WorkflowSourceError, WorkflowWatcher,
};

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
    #[serde(default)]
    pub before_run: Option<HookCommand>,
    #[serde(default)]
    pub after_run: Option<HookCommand>,
    #[serde(default)]
    pub before_remove: Option<HookCommand>,
}

/// The four hook phases that a workflow can opt into. The order in
/// `Self::ALL` is the order operators read them as a lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPhase {
    AfterCreate,
    BeforeRun,
    AfterRun,
    BeforeRemove,
}

impl HookPhase {
    pub const ALL: [HookPhase; 4] = [
        HookPhase::AfterCreate,
        HookPhase::BeforeRun,
        HookPhase::AfterRun,
        HookPhase::BeforeRemove,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            HookPhase::AfterCreate => "after_create",
            HookPhase::BeforeRun => "before_run",
            HookPhase::AfterRun => "after_run",
            HookPhase::BeforeRemove => "before_remove",
        }
    }

    /// Fatal phases must abort dispatch on hook failure; warn phases log
    /// the failure and continue. The orchestrator enforces this in #18;
    /// the workflow crate only documents it so the policy lives next to
    /// the phase enum.
    pub fn is_fatal_by_default(self) -> bool {
        matches!(self, HookPhase::AfterCreate | HookPhase::BeforeRun)
    }
}

impl HooksConfig {
    pub fn get(&self, phase: HookPhase) -> Option<&HookCommand> {
        match phase {
            HookPhase::AfterCreate => self.after_create.as_ref(),
            HookPhase::BeforeRun => self.before_run.as_ref(),
            HookPhase::AfterRun => self.after_run.as_ref(),
            HookPhase::BeforeRemove => self.before_remove.as_ref(),
        }
    }
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
    for phase in HookPhase::ALL {
        if let Some(hook) = c.hooks.get(phase) {
            if hook.command.trim().is_empty() {
                return Err(WorkflowError::invalid(
                    format!("hooks.{}.command", phase.as_str()),
                    "must not be empty",
                ));
            }
            require_positive_u64(
                &format!("hooks.{}.timeout_ms", phase.as_str()),
                hook.timeout_ms,
            )?;
        }
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

/// Per-render context: every prompt sees at least `issue` and `attempt`.
/// Extra context is intentionally not threaded through here yet — adding it is
/// a contract change to the prompt template surface and requires a new field.
#[derive(Debug, Clone, Serialize)]
pub struct PromptInput<'a> {
    pub issue: &'a cadenza_core::Issue,
    pub attempt: u32,
}

/// Failure modes from `render_prompt`. Distinct from `WorkflowError` so the
/// orchestrator can route compile/undefined/unknown-item issues to operators
/// without conflating them with workflow parsing.
#[derive(Debug, thiserror::Error)]
pub enum PromptRenderError {
    #[error("prompt template failed to compile: {message}")]
    Compile { message: String },
    #[error("prompt references undefined variable: {message}")]
    UndefinedVariable { message: String },
    #[error("prompt references unknown filter, test, or function: {message}")]
    UnknownItem { message: String },
    #[error("prompt render error: {message}")]
    Other { message: String },
}

impl PromptRenderError {
    fn classify(err: minijinja::Error, stage: RenderStage) -> Self {
        use minijinja::ErrorKind;
        let message = err.to_string();
        match err.kind() {
            ErrorKind::UndefinedError => Self::UndefinedVariable { message },
            ErrorKind::UnknownFilter
            | ErrorKind::UnknownTest
            | ErrorKind::UnknownFunction
            | ErrorKind::UnknownMethod => Self::UnknownItem { message },
            ErrorKind::SyntaxError
            | ErrorKind::TemplateNotFound
            | ErrorKind::BadEscape
            | ErrorKind::InvalidOperation
                if matches!(stage, RenderStage::Compile) =>
            {
                Self::Compile { message }
            }
            ErrorKind::SyntaxError => Self::Compile { message },
            _ => Self::Other { message },
        }
    }
}

enum RenderStage {
    Compile,
    Render,
}

/// Render `template` against `input` in strict mode: undefined variables and
/// unknown filters/tests/functions all fail closed. The strict behaviour is
/// not negotiable — `PromptConfig::strict_undefined` exists as a documented
/// contract field and is enforced at parse time
/// (`prompt.strict_undefined: false` is rejected by `parse_workflow`).
/// Pushing the toggle into the renderer would create a dead consumer-side
/// branch, so it lives at the boundary that actually validates it.
pub fn render_prompt(template: &str, input: &PromptInput<'_>) -> Result<String, PromptRenderError> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.add_template("prompt", template)
        .map_err(|e| PromptRenderError::classify(e, RenderStage::Compile))?;
    let tpl = env
        .get_template("prompt")
        .map_err(|e| PromptRenderError::classify(e, RenderStage::Compile))?;
    tpl.render(input)
        .map_err(|e| PromptRenderError::classify(e, RenderStage::Render))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn each_hook_phase_round_trips_through_config() {
        let body = MINIMAL_FRONT_MATTER
            .trim_end_matches("---\nprompt body\n")
            .to_string()
            + "hooks:\n  after_create:\n    command: \"echo create\"\n  before_run:\n    command: \"echo before-run\"\n  after_run:\n    command: \"echo after-run\"\n  before_remove:\n    command: \"echo before-remove\"\n---\nprompt body\n";
        let w = parse_workflow(&body).expect("parse");
        let names: Vec<_> = HookPhase::ALL
            .iter()
            .map(|p| w.config.hooks.get(*p).map(|h| h.command.as_str()))
            .collect();
        assert_eq!(
            names,
            vec![
                Some("echo create"),
                Some("echo before-run"),
                Some("echo after-run"),
                Some("echo before-remove"),
            ],
        );
    }

    #[test]
    fn before_run_hook_with_zero_timeout_is_invalid_boundary() {
        let body = MINIMAL_FRONT_MATTER
            .trim_end_matches("---\nprompt body\n")
            .to_string()
            + "hooks:\n  before_run:\n    command: \"git clean -xdf\"\n    timeout_ms: 0\n---\nprompt body\n";
        let err = parse_workflow(&body).unwrap_err();
        let WorkflowError::Invalid { field, .. } = err else {
            panic!("unexpected: {err:?}");
        };
        assert_eq!(field, "hooks.before_run.timeout_ms");
    }

    #[test]
    fn fatal_phases_match_documentation() {
        assert!(HookPhase::AfterCreate.is_fatal_by_default());
        assert!(HookPhase::BeforeRun.is_fatal_by_default());
        assert!(!HookPhase::AfterRun.is_fatal_by_default());
        assert!(!HookPhase::BeforeRemove.is_fatal_by_default());
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

    fn sample_issue() -> cadenza_core::Issue {
        cadenza_core::Issue {
            id: "issue-123".into(),
            identifier: "CAD-42".into(),
            title: "Wire orchestrator state".into(),
            description: Some("Tracks orchestrator skeleton work.".into()),
            priority: Some(1),
            state: "in progress".into(),
            branch_name: Some("feat/orch".into()),
            url: Some("https://linear.app/cad/issue/CAD-42".into()),
            labels: vec!["priority:P0".into(), "component:orchestrator".into()],
            blocked_by: vec![],
            created_at: Some("2026-05-22T00:00:00Z".into()),
            updated_at: Some("2026-05-22T08:00:00Z".into()),
        }
    }

    fn input<'a>(issue: &'a cadenza_core::Issue, attempt: u32) -> PromptInput<'a> {
        PromptInput { issue, attempt }
    }

    #[test]
    fn renders_issue_and_attempt_into_prompt() {
        let issue = sample_issue();
        let template =
            "Run #{{ attempt }} for {{ issue.identifier }}: {{ issue.title }} ({{ issue.state }})";
        let rendered = render_prompt(template, &input(&issue, 2)).unwrap();
        assert_eq!(
            rendered,
            "Run #2 for CAD-42: Wire orchestrator state (in progress)",
        );
    }

    #[test]
    fn snapshot_matches_full_workflow_example_template() {
        // Snapshot-style assertion: render the prompt body shipped in
        // WORKFLOW.example.md and assert the deterministic output. If the
        // template or the issue shape changes, this test fails and the new
        // string must be reviewed.
        let issue = sample_issue();
        let workflow_text = include_str!("../../../WORKFLOW.example.md");
        let workflow = parse_workflow(workflow_text).expect("example parses");
        let rendered = render_prompt(&workflow.prompt_template, &input(&issue, 1)).unwrap();
        let expected = "You are working on Linear issue CAD-42: Wire orchestrator state.\n\n\
                        Rules:\n\
                        - Work only inside the assigned workspace.\n\
                        - Use available tools for tracker writes; do not assume the orchestrator writes tickets for you.\n\
                        - Summarize any handoff state clearly.\n\n\
                        Issue description:\n\
                        Tracks orchestrator skeleton work.";
        assert_eq!(rendered, expected);
    }

    #[test]
    fn snapshot_handles_missing_optional_description() {
        // `issue.description` is Option<String>; the example template applies
        // `| default("", true)`, so a None description should render as empty
        // without producing an UndefinedVariable error.
        let mut issue = sample_issue();
        issue.description = None;
        let workflow_text = include_str!("../../../WORKFLOW.example.md");
        let workflow = parse_workflow(workflow_text).expect("example parses");
        let rendered = render_prompt(&workflow.prompt_template, &input(&issue, 1)).unwrap();
        // `default("", true)` produces an empty string, so the rendered body
        // ends with the "Issue description:" header followed by whitespace.
        assert!(
            rendered.trim_end().ends_with("Issue description:"),
            "got: {rendered}",
        );
        assert!(!rendered.contains("None"), "leaked Option debug repr");
    }

    #[test]
    fn undefined_variable_classifies_as_undefined_variable() {
        let issue = sample_issue();
        let err = render_prompt("hello {{ missing }}", &input(&issue, 0)).unwrap_err();
        assert!(
            matches!(err, PromptRenderError::UndefinedVariable { .. }),
            "got: {err:?}",
        );
    }

    #[test]
    fn unknown_filter_classifies_as_unknown_item() {
        let issue = sample_issue();
        let err =
            render_prompt("{{ issue.title | nonexistent_filter }}", &input(&issue, 0)).unwrap_err();
        assert!(
            matches!(err, PromptRenderError::UnknownItem { .. }),
            "got: {err:?}",
        );
    }

    #[test]
    fn unknown_function_classifies_as_unknown_item() {
        let issue = sample_issue();
        let err = render_prompt("{{ nonexistent_function() }}", &input(&issue, 0)).unwrap_err();
        assert!(
            matches!(err, PromptRenderError::UnknownItem { .. }),
            "got: {err:?}",
        );
    }

    #[test]
    fn unknown_test_classifies_as_unknown_item() {
        let issue = sample_issue();
        let err = render_prompt(
            "{% if issue.title is nonexistent_test %}x{% endif %}",
            &input(&issue, 0),
        )
        .unwrap_err();
        assert!(
            matches!(err, PromptRenderError::UnknownItem { .. }),
            "got: {err:?}",
        );
    }

    #[test]
    fn template_syntax_error_classifies_as_compile() {
        let issue = sample_issue();
        let err = render_prompt("{{ unterminated", &input(&issue, 0)).unwrap_err();
        assert!(
            matches!(err, PromptRenderError::Compile { .. }),
            "got: {err:?}",
        );
    }

    // Boundary: attempt is a plain u32 — both 0 and u32::MAX must render
    // without error so we never reject a valid run attempt number.
    #[test]
    fn attempt_zero_boundary_renders() {
        let issue = sample_issue();
        let rendered = render_prompt("attempt {{ attempt }}", &input(&issue, 0)).unwrap();
        assert_eq!(rendered, "attempt 0");
    }

    #[test]
    fn attempt_u32_max_boundary_renders() {
        let issue = sample_issue();
        let rendered = render_prompt("attempt {{ attempt }}", &input(&issue, u32::MAX)).unwrap();
        assert_eq!(rendered, format!("attempt {}", u32::MAX));
    }
}
