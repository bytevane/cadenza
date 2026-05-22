//! Defensive WORKFLOW.md hot reload primitives.
//!
//! `WorkflowSource` owns the active workflow definition behind an
//! atomic Arc swap and preserves the last-known-good config on any
//! reload failure. `WorkflowWatcher` wraps a `notify` watcher that
//! drives `try_reload` whenever the file changes on disk. The
//! orchestrator never gets a mutable handle to the source — it pulls
//! `Arc<WorkflowDefinition>` via `current()` and reloads happen on
//! their own thread.

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use thiserror::Error;

use crate::{WorkflowDefinition, WorkflowError, parse_workflow};

/// How many past reload events to keep for operator visibility.
const HISTORY_CAPACITY: usize = 16;

#[derive(Debug, Error)]
pub enum WorkflowSourceError {
    #[error("failed to read workflow file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("workflow rejected: {0}")]
    Workflow(#[from] WorkflowError),
}

#[derive(Debug, Clone)]
pub struct ReloadEvent {
    pub at: SystemTime,
    pub outcome: ReloadOutcome,
}

#[derive(Debug, Clone)]
pub enum ReloadOutcome {
    /// Reload succeeded; the active config moved to this version.
    Loaded { version: u64 },
    /// Reload was rejected. `reason` is the rendered error string so
    /// operators can see it in a state snapshot without re-running
    /// the parser.
    Rejected { reason: String },
}

#[derive(Debug, Error)]
pub enum WatchError {
    #[error("notify failed to create watcher: {0}")]
    Notify(#[from] notify::Error),
}

/// Atomic-swap workflow holder. Cheap to clone via `Arc<WorkflowSource>`.
#[derive(Debug)]
pub struct WorkflowSource {
    path: PathBuf,
    state: RwLock<State>,
}

#[derive(Debug)]
struct State {
    current: Arc<WorkflowDefinition>,
    version: u64,
    last_event: ReloadEvent,
    history: VecDeque<ReloadEvent>,
}

impl WorkflowSource {
    /// Read and validate the file at `path`. On success the source is
    /// at version 1 with one `Loaded` event in history. On failure no
    /// `WorkflowSource` is constructed (we have nothing to fall back to).
    pub fn load_initial(path: impl Into<PathBuf>) -> Result<Self, WorkflowSourceError> {
        let path = path.into();
        let bytes = fs::read_to_string(&path).map_err(|e| WorkflowSourceError::Io {
            path: path.clone(),
            source: e,
        })?;
        let definition = parse_workflow(&bytes)?;
        let event = ReloadEvent {
            at: SystemTime::now(),
            outcome: ReloadOutcome::Loaded { version: 1 },
        };
        let mut history = VecDeque::with_capacity(HISTORY_CAPACITY);
        history.push_back(event.clone());
        Ok(Self {
            path,
            state: RwLock::new(State {
                current: Arc::new(definition),
                version: 1,
                last_event: event,
                history,
            }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns a cheap `Arc` clone of the last-known-good definition. The
    /// orchestrator stores this and re-pulls when its own tick logic decides
    /// it is safe — it never mutates through this handle.
    pub fn current(&self) -> Arc<WorkflowDefinition> {
        Arc::clone(
            &self
                .state
                .read()
                .expect("source state lock poisoned")
                .current,
        )
    }

    pub fn version(&self) -> u64 {
        self.state
            .read()
            .expect("source state lock poisoned")
            .version
    }

    pub fn last_event(&self) -> ReloadEvent {
        self.state
            .read()
            .expect("source state lock poisoned")
            .last_event
            .clone()
    }

    pub fn history(&self) -> Vec<ReloadEvent> {
        self.state
            .read()
            .expect("source state lock poisoned")
            .history
            .iter()
            .cloned()
            .collect()
    }

    /// Re-read the file, re-parse, and atomically swap in the new
    /// definition on success. On any failure the existing
    /// `current()` is preserved (last-known-good) and a `Rejected`
    /// event is recorded so the operator can see what happened.
    pub fn try_reload(&self) -> ReloadOutcome {
        let outcome = match self.read_and_parse() {
            Ok(definition) => self.commit_new(definition),
            Err(err) => ReloadOutcome::Rejected {
                reason: err.to_string(),
            },
        };
        self.record(outcome.clone());
        if let ReloadOutcome::Rejected { reason } = &outcome {
            tracing::warn!(
                target: "cadenza.workflow.reload",
                path = %self.path.display(),
                error = reason.as_str(),
                "workflow reload rejected, keeping last-known-good",
            );
        } else if let ReloadOutcome::Loaded { version } = &outcome {
            tracing::info!(
                target: "cadenza.workflow.reload",
                path = %self.path.display(),
                version = version,
                "workflow reload succeeded",
            );
        }
        outcome
    }

    fn read_and_parse(&self) -> Result<WorkflowDefinition, WorkflowSourceError> {
        let bytes = fs::read_to_string(&self.path).map_err(|e| WorkflowSourceError::Io {
            path: self.path.clone(),
            source: e,
        })?;
        parse_workflow(&bytes).map_err(WorkflowSourceError::from)
    }

    fn commit_new(&self, definition: WorkflowDefinition) -> ReloadOutcome {
        let mut state = self.state.write().expect("source state lock poisoned");
        state.version = state.version.saturating_add(1);
        state.current = Arc::new(definition);
        ReloadOutcome::Loaded {
            version: state.version,
        }
    }

    fn record(&self, outcome: ReloadOutcome) {
        let mut state = self.state.write().expect("source state lock poisoned");
        let event = ReloadEvent {
            at: SystemTime::now(),
            outcome,
        };
        state.last_event = event.clone();
        if state.history.len() == HISTORY_CAPACITY {
            state.history.pop_front();
        }
        state.history.push_back(event);
    }
}

/// `notify`-based watcher. Constructs a recommended watcher that calls
/// `source.try_reload()` whenever the watched file changes. The watcher
/// is held by-value; dropping it stops watching.
pub struct WorkflowWatcher {
    _watcher: notify::RecommendedWatcher,
    source: Arc<WorkflowSource>,
}

impl WorkflowWatcher {
    pub fn spawn(source: Arc<WorkflowSource>) -> Result<Self, WatchError> {
        use notify::{EventKind, RecursiveMode, Watcher};

        let path = source.path().to_path_buf();
        let source_for_cb = Arc::clone(&source);
        let mut watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                let event = match res {
                    Ok(ev) => ev,
                    Err(e) => {
                        tracing::warn!(
                            target: "cadenza.workflow.reload",
                            error = %e,
                            "notify reported an error event, ignoring",
                        );
                        return;
                    }
                };
                if matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                ) {
                    source_for_cb.try_reload();
                }
            })?;
        watcher.watch(&path, RecursiveMode::NonRecursive)?;
        Ok(Self {
            _watcher: watcher,
            source,
        })
    }

    pub fn source(&self) -> &Arc<WorkflowSource> {
        &self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const VALID_BODY: &str = r#"---
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

    fn make_tempfile(body: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("create tempfile");
        f.write_all(body.as_bytes()).expect("write");
        f.flush().expect("flush");
        f
    }

    fn rewrite(file: &tempfile::NamedTempFile, body: &str) {
        fs::write(file.path(), body).expect("rewrite");
    }

    #[test]
    fn load_initial_succeeds_on_valid_file_with_version_one() {
        let f = make_tempfile(VALID_BODY);
        let src = WorkflowSource::load_initial(f.path()).expect("load_initial");
        assert_eq!(src.version(), 1);
        assert!(matches!(
            src.last_event().outcome,
            ReloadOutcome::Loaded { version: 1 }
        ));
    }

    #[test]
    fn load_initial_fails_on_missing_file() {
        let path = PathBuf::from("/nonexistent/cadenza-test-workflow.md");
        let err = WorkflowSource::load_initial(path).unwrap_err();
        assert!(matches!(err, WorkflowSourceError::Io { .. }), "got {err:?}");
    }

    #[test]
    fn load_initial_fails_on_invalid_workflow() {
        let f = make_tempfile("not even front matter");
        let err = WorkflowSource::load_initial(f.path()).unwrap_err();
        assert!(
            matches!(err, WorkflowSourceError::Workflow(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn reload_with_valid_change_bumps_version_and_swaps() {
        let f = make_tempfile(VALID_BODY);
        let src = WorkflowSource::load_initial(f.path()).unwrap();
        assert_eq!(src.version(), 1);
        let before = src.current();

        let bumped = VALID_BODY.replace(r#"token: "secret""#, r#"token: "rotated""#);
        rewrite(&f, &bumped);
        let outcome = src.try_reload();

        assert!(matches!(outcome, ReloadOutcome::Loaded { version: 2 }));
        assert_eq!(src.version(), 2);
        let after = src.current();
        assert_eq!(before.config.tracker.token, "secret");
        assert_eq!(after.config.tracker.token, "rotated");
    }

    #[test]
    fn reload_with_invalid_yaml_preserves_last_known_good() {
        let f = make_tempfile(VALID_BODY);
        let src = WorkflowSource::load_initial(f.path()).unwrap();
        let before_arc = src.current();
        let before_version = src.version();

        rewrite(&f, "---\n: : bogus yaml\n---\nbody");
        let outcome = src.try_reload();

        assert!(
            matches!(outcome, ReloadOutcome::Rejected { .. }),
            "got {outcome:?}"
        );
        assert_eq!(
            src.version(),
            before_version,
            "version must not move on reject"
        );
        assert!(Arc::ptr_eq(&before_arc, &src.current()));
        match src.last_event().outcome {
            ReloadOutcome::Rejected { reason } => assert!(reason.contains("YAML")),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn reload_with_invalid_validation_preserves_last_known_good() {
        let f = make_tempfile(VALID_BODY);
        let src = WorkflowSource::load_initial(f.path()).unwrap();
        let before_version = src.version();

        // Empty token is a validation failure, not a YAML parse failure.
        let bumped = VALID_BODY.replace(r#"token: "secret""#, r#"token: """#);
        rewrite(&f, &bumped);
        let outcome = src.try_reload();

        assert!(
            matches!(outcome, ReloadOutcome::Rejected { .. }),
            "got {outcome:?}"
        );
        assert_eq!(src.version(), before_version);
        match src.last_event().outcome {
            ReloadOutcome::Rejected { reason } => assert!(reason.contains("tracker.token")),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn reload_after_file_removed_preserves_last_known_good() {
        let f = make_tempfile(VALID_BODY);
        let src = WorkflowSource::load_initial(f.path()).unwrap();
        let before_version = src.version();
        let before_arc = src.current();
        let path = f.path().to_path_buf();
        drop(f); // removes the tempfile
        assert!(!path.exists());

        let outcome = src.try_reload();
        assert!(
            matches!(outcome, ReloadOutcome::Rejected { .. }),
            "got {outcome:?}"
        );
        assert_eq!(src.version(), before_version);
        assert!(Arc::ptr_eq(&before_arc, &src.current()));
    }

    // Boundary law: history is bounded at HISTORY_CAPACITY. Verify =N
    // (exact-fit) and =N+1 (drop-oldest).
    #[test]
    fn history_at_capacity_keeps_all_events() {
        let f = make_tempfile(VALID_BODY);
        let src = WorkflowSource::load_initial(f.path()).unwrap();
        // load_initial already pushed one event, so push HISTORY_CAPACITY - 1 more.
        for _ in 1..HISTORY_CAPACITY {
            src.try_reload(); // no-op edits still count as Loaded events
        }
        assert_eq!(src.history().len(), HISTORY_CAPACITY);
    }

    #[test]
    fn history_beyond_capacity_drops_oldest() {
        let f = make_tempfile(VALID_BODY);
        let src = WorkflowSource::load_initial(f.path()).unwrap();
        for _ in 0..HISTORY_CAPACITY {
            src.try_reload();
        }
        let history = src.history();
        assert_eq!(history.len(), HISTORY_CAPACITY);
        // The earliest event would have been version=1; after overflow the
        // oldest retained version must be > 1.
        match &history[0].outcome {
            ReloadOutcome::Loaded { version } => assert!(
                *version > 1,
                "expected oldest event version > 1 after overflow, got {version}",
            ),
            other => panic!("expected Loaded, got {other:?}"),
        }
    }

    #[test]
    fn watcher_picks_up_valid_change() {
        let f = make_tempfile(VALID_BODY);
        let src = Arc::new(WorkflowSource::load_initial(f.path()).unwrap());
        let _watcher = WorkflowWatcher::spawn(Arc::clone(&src)).expect("spawn watcher");

        // Rewrite the file with a valid edit and poll until the watcher
        // observes it. Hard cap so the test cannot hang indefinitely.
        let bumped = VALID_BODY.replace(r#"token: "secret""#, r#"token: "watched""#);
        rewrite(&f, &bumped);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if src.version() >= 2 && src.current().config.tracker.token == "watched" {
                break;
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "watcher did not pick up the change within 5s; last_event = {:?}, version = {}",
                    src.last_event(),
                    src.version(),
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
