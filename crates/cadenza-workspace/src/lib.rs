//! Per-issue workspace key + containment safety.
//!
//! Every per-issue workspace path produced by Cadenza must live under the
//! operator-configured `workspace.root`. The helpers here enforce that
//! invariant lexically (no FS access required) and, when the workspace
//! exists on disk, by canonicalising both paths so a symlink cannot
//! tunnel out of the root.
//!
//! Hook execution lives in [`hooks`]; see that module for the trusted
//! shell hook boundary.

pub mod hooks;

use std::path::{Path, PathBuf};

use cadenza_core::workspace_key;
use camino::{Utf8Component, Utf8Path, Utf8PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("workspace root must be absolute: {0}")]
    RootNotAbsolute(String),
    #[error("path contains a `..` traversal that escapes root: {candidate}")]
    Traversal { candidate: String },
    #[error("absolute path not allowed as workspace segment: {segment}")]
    AbsoluteSegment { segment: String },
    #[error("workspace path escaped root: root={root}, candidate={candidate}")]
    OutsideRoot { root: String, candidate: String },
    #[error("failed to canonicalize {path}: {source}")]
    Canonicalize {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Compute the per-issue workspace path under `root`. The result is a
/// direct child of `root` whose final component is the sanitised key
/// produced by `cadenza_core::workspace_key`. The workspace itself does
/// not need to exist on disk yet.
pub fn workspace_for_issue(
    root: impl AsRef<Utf8Path>,
    issue_identifier: &str,
) -> Result<Utf8PathBuf, WorkspaceError> {
    let root = root.as_ref();
    if !root.is_absolute() {
        return Err(WorkspaceError::RootNotAbsolute(root.to_string()));
    }
    let key = workspace_key(issue_identifier);
    let candidate = root.join(key);
    assert_inside_workspace_root(root, &candidate)?;
    Ok(candidate)
}

/// Backwards-compatible alias for [`workspace_for_issue`] retained for
/// existing call sites (cadenza-cli, doctor command).
pub fn workspace_path(
    root: impl AsRef<Utf8Path>,
    issue_identifier: &str,
) -> Result<Utf8PathBuf, WorkspaceError> {
    workspace_for_issue(root, issue_identifier)
}

/// Lexical containment check. Both `root` and `candidate` are normalised
/// (collapsing `.` and `..` components); the result must have `root` as
/// a prefix on a component boundary. Does not touch the filesystem.
pub fn assert_inside_workspace_root(
    root: &Utf8Path,
    candidate: &Utf8Path,
) -> Result<(), WorkspaceError> {
    if !root.is_absolute() {
        return Err(WorkspaceError::RootNotAbsolute(root.to_string()));
    }
    let root_components = normalize_components(root)?;
    let candidate_components = normalize_components(candidate)?;
    if candidate_components.len() < root_components.len()
        || candidate_components[..root_components.len()] != root_components[..]
    {
        return Err(WorkspaceError::OutsideRoot {
            root: root.to_string(),
            candidate: candidate.to_string(),
        });
    }
    Ok(())
}

/// Append `segment` to `root` lexically and verify the result is inside
/// `root`. Rejects absolute segments outright; `..` segments are allowed
/// only as long as they do not escape `root` after normalisation.
pub fn safe_join(root: &Utf8Path, segment: &str) -> Result<Utf8PathBuf, WorkspaceError> {
    let segment_path = Utf8Path::new(segment);
    if segment_path.is_absolute() {
        return Err(WorkspaceError::AbsoluteSegment {
            segment: segment.to_string(),
        });
    }
    let joined = root.join(segment_path);
    let components = normalize_components(&joined)?;
    let normalized = reassemble(&components);
    assert_inside_workspace_root(root, &normalized)?;
    Ok(normalized)
}

/// Canonicalised containment check that returns the resolved candidate path.
/// Both `root` and `candidate` must exist on disk; symlinks are resolved
/// before comparison so a symlink inside `root` that points to `/etc` fails
/// closed. Callers that go on to open the file should open the **returned**
/// path, not re-canonicalise `candidate`, so the validated path and the opened
/// path cannot diverge under a concurrent symlink swap (check/use consistency).
pub fn resolve_inside(root: &Path, candidate: &Path) -> Result<PathBuf, WorkspaceError> {
    let root_real = root
        .canonicalize()
        .map_err(|e| WorkspaceError::Canonicalize {
            path: root.display().to_string(),
            source: e,
        })?;
    let candidate_real = candidate
        .canonicalize()
        .map_err(|e| WorkspaceError::Canonicalize {
            path: candidate.display().to_string(),
            source: e,
        })?;
    if !candidate_real.starts_with(&root_real) {
        return Err(WorkspaceError::OutsideRoot {
            root: root_real.display().to_string(),
            candidate: candidate_real.display().to_string(),
        });
    }
    Ok(candidate_real)
}

/// Canonicalised containment check. Both `root` and `candidate` must
/// exist on disk; symlinks are resolved before comparison so a symlink
/// inside `root` that points to `/etc` will fail closed. Use
/// [`resolve_inside`] instead when you will open the file afterwards.
pub fn canonicalize_inside(root: &Path, candidate: &Path) -> Result<(), WorkspaceError> {
    resolve_inside(root, candidate).map(|_| ())
}

/// PathBuf convenience wrapper around [`canonicalize_inside`].
pub fn canonicalize_inside_buf(root: PathBuf, candidate: PathBuf) -> Result<(), WorkspaceError> {
    canonicalize_inside(&root, &candidate)
}

fn normalize_components(path: &Utf8Path) -> Result<Vec<String>, WorkspaceError> {
    let mut out: Vec<String> = Vec::new();
    for comp in path.components() {
        match comp {
            Utf8Component::Prefix(p) => out.push(p.as_str().to_string()),
            Utf8Component::RootDir => out.push("/".to_string()),
            Utf8Component::CurDir => continue,
            Utf8Component::ParentDir => {
                let last = out.last().map(String::as_str);
                let popable = matches!(last, Some(name) if name != "/" && !name.is_empty());
                if popable {
                    out.pop();
                } else {
                    return Err(WorkspaceError::Traversal {
                        candidate: path.to_string(),
                    });
                }
            }
            Utf8Component::Normal(n) => out.push(n.to_string()),
        }
    }
    Ok(out)
}

fn reassemble(components: &[String]) -> Utf8PathBuf {
    let mut out = Utf8PathBuf::new();
    for c in components {
        if c == "/" {
            out.push("/");
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/var/lib/cadenza/workspaces";

    fn root() -> &'static Utf8Path {
        Utf8Path::new(ROOT)
    }

    // ---------- workspace_for_issue ----------

    #[test]
    fn workspace_for_issue_maps_identifier_under_root() {
        let path = workspace_for_issue(ROOT, "ABC-123/foo").unwrap();
        assert_eq!(path.as_str(), "/var/lib/cadenza/workspaces/ABC-123_foo");
    }

    #[test]
    fn workspace_for_issue_rejects_relative_root() {
        let err = workspace_for_issue("var/lib", "ABC-123").unwrap_err();
        assert!(
            matches!(err, WorkspaceError::RootNotAbsolute(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn workspace_path_alias_still_works() {
        let path = workspace_path(ROOT, "ABC-123").unwrap();
        assert_eq!(path.as_str(), "/var/lib/cadenza/workspaces/ABC-123");
    }

    // ---------- assert_inside_workspace_root ----------

    #[test]
    fn inside_root_direct_child_is_ok() {
        assert_inside_workspace_root(root(), Utf8Path::new("/var/lib/cadenza/workspaces/ABC-123"))
            .unwrap();
    }

    #[test]
    fn inside_root_self_is_ok() {
        assert_inside_workspace_root(root(), root()).unwrap();
    }

    #[test]
    fn inside_root_normalised_dot_dot_returns_to_root_is_ok() {
        assert_inside_workspace_root(
            root(),
            Utf8Path::new("/var/lib/cadenza/workspaces/ABC-123/.."),
        )
        .unwrap();
    }

    #[test]
    fn rejects_dot_dot_escape_to_sibling() {
        let err = assert_inside_workspace_root(
            root(),
            Utf8Path::new("/var/lib/cadenza/workspaces/../other"),
        )
        .unwrap_err();
        assert!(
            matches!(err, WorkspaceError::OutsideRoot { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_outside_root_absolute_path() {
        let err = assert_inside_workspace_root(root(), Utf8Path::new("/etc/passwd")).unwrap_err();
        assert!(
            matches!(err, WorkspaceError::OutsideRoot { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_dot_dot_above_root_dir() {
        let err = assert_inside_workspace_root(root(), Utf8Path::new("/..")).unwrap_err();
        assert!(
            matches!(err, WorkspaceError::Traversal { .. }),
            "got {err:?}"
        );
    }

    // Paired-edge boundary on the "..-allowed-only-if-stays-inside" rule:
    // a one-segment child is the smallest valid descent, a one-segment
    // ascent (`/var/lib/cadenza`) is the smallest illegal escape.
    #[test]
    fn boundary_one_segment_inside_root_is_ok() {
        assert_inside_workspace_root(root(), Utf8Path::new("/var/lib/cadenza/workspaces/x"))
            .unwrap();
    }

    #[test]
    fn boundary_one_segment_above_root_is_rejected() {
        let err =
            assert_inside_workspace_root(root(), Utf8Path::new("/var/lib/cadenza")).unwrap_err();
        assert!(
            matches!(err, WorkspaceError::OutsideRoot { .. }),
            "got {err:?}"
        );
    }

    // ---------- safe_join ----------

    #[test]
    fn safe_join_basic_relative_segment() {
        let p = safe_join(root(), "foo/bar").unwrap();
        assert_eq!(p.as_str(), "/var/lib/cadenza/workspaces/foo/bar");
    }

    #[test]
    fn safe_join_collapses_internal_dot_dot_inside_root() {
        let p = safe_join(root(), "foo/../bar").unwrap();
        assert_eq!(p.as_str(), "/var/lib/cadenza/workspaces/bar");
    }

    #[test]
    fn safe_join_rejects_absolute_segment() {
        let err = safe_join(root(), "/etc/passwd").unwrap_err();
        assert!(
            matches!(err, WorkspaceError::AbsoluteSegment { .. }),
            "got {err:?}",
        );
    }

    #[test]
    fn safe_join_rejects_escape_via_dot_dot() {
        let err = safe_join(root(), "../other").unwrap_err();
        assert!(
            matches!(err, WorkspaceError::OutsideRoot { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn safe_join_rejects_deep_escape() {
        let err = safe_join(root(), "foo/../../../etc").unwrap_err();
        assert!(
            matches!(err, WorkspaceError::OutsideRoot { .. }),
            "got {err:?}"
        );
    }

    // ---------- canonicalize_inside ----------

    #[test]
    fn canonicalize_inside_accepts_real_subdirectory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let child = root.join("issue");
        std::fs::create_dir(&child).unwrap();
        canonicalize_inside(root, &child).expect("real child is inside");
    }

    #[test]
    fn resolve_inside_returns_canonical_child_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let child = root.join("issue");
        std::fs::create_dir(&child).unwrap();
        let resolved = resolve_inside(root, &child).expect("real child is inside");
        // The returned path is canonical and inside the canonical root, so a
        // caller opens exactly what was validated.
        assert!(resolved.starts_with(root.canonicalize().unwrap()));
        assert!(resolved.ends_with("issue"));
    }

    #[cfg(unix)]
    #[test]
    fn canonicalize_inside_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let outer = tempfile::tempdir().unwrap();
        let root_dir = outer.path().join("root");
        let evil_target = outer.path().join("escape");
        std::fs::create_dir(&root_dir).unwrap();
        std::fs::create_dir(&evil_target).unwrap();
        let link = root_dir.join("issue");
        symlink(&evil_target, &link).unwrap();
        let err = canonicalize_inside(&root_dir, &link).unwrap_err();
        assert!(
            matches!(err, WorkspaceError::OutsideRoot { .. }),
            "got {err:?}"
        );
    }
}
