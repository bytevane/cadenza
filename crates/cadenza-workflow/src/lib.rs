use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub config: serde_yaml::Value,
    pub prompt_template: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("WORKFLOW.md must start with YAML front matter delimiter `---`")]
    MissingFrontMatter,
    #[error("WORKFLOW.md front matter is not closed with `---`")]
    UnterminatedFrontMatter,
    #[error("invalid YAML front matter: {0}")]
    InvalidYaml(#[from] serde_yaml::Error),
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
    let config = serde_yaml::from_str(front_matter)?;
    Ok(WorkflowDefinition {
        config,
        prompt_template,
    })
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

    #[test]
    fn parses_workflow_frontmatter_and_body() {
        let workflow =
            parse_workflow("---\ntracker:\n  kind: linear\n---\nHello {{ issue.identifier }}")
                .unwrap();
        assert_eq!(workflow.prompt_template, "Hello {{ issue.identifier }}");
    }

    #[test]
    fn strict_prompt_fails_on_unknown_variable() {
        let err = render_prompt_strict("{{ missing }}", json!({})).unwrap_err();
        assert_eq!(err.kind(), minijinja::ErrorKind::UndefinedError);
    }
}
