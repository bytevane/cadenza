use cadenza_core::workspace_key;
use camino::{Utf8Path, Utf8PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("workspace path escaped root: root={root}, candidate={candidate}")]
    OutsideRoot { root: String, candidate: String },
    #[error("workspace root must be absolute: {0}")]
    RootNotAbsolute(String),
}

pub fn workspace_path(
    root: impl AsRef<Utf8Path>,
    issue_identifier: &str,
) -> Result<Utf8PathBuf, WorkspaceError> {
    let root = root.as_ref();
    if !root.is_absolute() {
        return Err(WorkspaceError::RootNotAbsolute(root.to_string()));
    }

    let key = workspace_key(issue_identifier);
    let candidate = root.join(key);
    ensure_inside(root, &candidate)?;
    Ok(candidate)
}

pub fn ensure_inside(root: &Utf8Path, candidate: &Utf8Path) -> Result<(), WorkspaceError> {
    let root_norm = trim_trailing_slashes(root.as_str());
    let candidate_norm = trim_trailing_slashes(candidate.as_str());

    if candidate_norm == root_norm || candidate_norm.starts_with(&format!("{root_norm}/")) {
        Ok(())
    } else {
        Err(WorkspaceError::OutsideRoot {
            root: root.to_string(),
            candidate: candidate.to_string(),
        })
    }
}

fn trim_trailing_slashes(value: &str) -> &str {
    let trimmed = value.trim_end_matches('/');
    if trimmed.is_empty() { "/" } else { trimmed }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_issue_identifier_under_root() {
        let path = workspace_path("/var/lib/cadenza/workspaces", "ABC-123/foo").unwrap();
        assert_eq!(path.as_str(), "/var/lib/cadenza/workspaces/ABC-123_foo");
    }

    #[test]
    fn rejects_paths_outside_root() {
        let err = ensure_inside(
            Utf8Path::new("/var/lib/cadenza/workspaces"),
            Utf8Path::new("/var/lib/cadenza-other/ABC-123"),
        )
        .unwrap_err();
        assert!(matches!(err, WorkspaceError::OutsideRoot { .. }));
    }
}
