# Spec: Issue #10 — workspace key sanitizer + containment checks

Tracks https://github.com/bytevane/cadenza/issues/10 (Milestone: MVP 1 - Workflow & Workspace).

## Outcome

`cadenza-workspace` ships three lexical guards plus one FS-aware guard
so every per-issue path Cadenza touches provably lives under
`workspace.root`. The old single-purpose `workspace_path` regresses on
`..` segments — the rewrite fixes it.

## Public surface

```rust
pub enum WorkspaceError {
    RootNotAbsolute(String),
    Traversal { candidate: String },
    AbsoluteSegment { segment: String },
    OutsideRoot { root: String, candidate: String },
    Canonicalize { path: String, source: io::Error },
}

pub fn workspace_for_issue(root, issue_identifier) -> Result<Utf8PathBuf, WorkspaceError>;
pub fn workspace_path(root, issue_identifier) -> Result<Utf8PathBuf, WorkspaceError>; // alias
pub fn assert_inside_workspace_root(root: &Utf8Path, candidate: &Utf8Path) -> Result<(), WorkspaceError>;
pub fn safe_join(root: &Utf8Path, segment: &str) -> Result<Utf8PathBuf, WorkspaceError>;
pub fn canonicalize_inside(root: &Path, candidate: &Path) -> Result<(), WorkspaceError>;
pub fn canonicalize_inside_buf(root: PathBuf, candidate: PathBuf) -> Result<(), WorkspaceError>;
```

## Containment semantics

- **Lexical normalisation** (`normalize_components`) walks each path
  component, collapsing `CurDir`, popping a `Normal` for each
  `ParentDir`, and erroring with `Traversal` if a `ParentDir` would
  step above the root component. The pre-existing string-prefix check
  silently accepted `/var/lib/cadenza/workspaces/../other` because the
  candidate string *starts with* the root prefix — fixed here.
- **`safe_join`** rejects absolute segments outright (`AbsoluteSegment`)
  and routes `..`-containing segments through the same normaliser, so
  `foo/../bar` resolves to `root/bar` (allowed) but `foo/../..` is
  rejected.
- **`canonicalize_inside`** resolves symlinks. A workspace symlink
  pointing at `/etc` fails with `OutsideRoot` after canonicalisation.
- All helpers return typed `WorkspaceError`; no `anyhow::Error` at
  this boundary so the orchestrator can branch on the variant.

## Acceptance verification

| Acceptance criterion (from #10) | Verification |
| --- | --- |
| `cargo run -p cadenza-cli -- workspace-key ABC-123/foo` returns a safe deterministic key. | Unchanged from earlier work; `cadenza_core::workspace_key` covers it. |
| Path traversal attempts fail. | `rejects_dot_dot_escape_to_sibling`, `safe_join_rejects_escape_via_dot_dot`, `safe_join_rejects_deep_escape`, `rejects_dot_dot_above_root_dir`. |
| Absolute path attempts fail. | `safe_join_rejects_absolute_segment` + `rejects_outside_root_absolute_path`. |
| Symlink escape attempts fail on platforms where the test is supported. | `canonicalize_inside_rejects_symlink_escape` (`#[cfg(unix)]`). |
| All workspace path functions return typed errors. | `WorkspaceError` enum is the sole error type across the public surface. |

## Boundary tests (per project rule)

- `boundary_one_segment_inside_root_is_ok` (=N: smallest valid descent) paired with `boundary_one_segment_above_root_is_rejected` (=N+1: smallest illegal ascent).
- `safe_join_basic_relative_segment` paired with `safe_join_collapses_internal_dot_dot_inside_root` — inner `..` allowed if result stays inside.
- `canonicalize_inside_accepts_real_subdirectory` paired with `canonicalize_inside_rejects_symlink_escape`.

## Out of scope

- Git checkout / workspace bootstrap (per issue non-goal).
- `noatime`, mount-policy, or container-namespace isolation.
- Hook execution boundary (#11) consumes these helpers but does not extend them.

## References

- `cadenza_core::workspace_key`
- `decisions/0001-rust-host-wasm-extensions.md`
- `SECURITY.md`
