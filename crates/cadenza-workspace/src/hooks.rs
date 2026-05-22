//! Trusted shell hook execution boundary.
//!
//! Hooks run as `sh -c <command>` with `cwd` set to the per-issue
//! workspace, an enforced wall-clock timeout that kills the process on
//! expiry, bounded stdout/stderr capture, and substring redaction for
//! any secret values the orchestrator has registered. The runner is
//! synchronous and returns a `HookOutcome` instead of panicking — the
//! caller (orchestrator) decides whether to propagate a failure as
//! fatal based on the hook phase (see `HookPhase::is_fatal_by_default`).

use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cadenza_workflow::HookCommand;
use thiserror::Error;

/// Per-stream byte cap. Hooks rarely need to print megabytes; if they
/// do, the rest is dropped and replaced with a `<truncated …>` marker.
pub const DEFAULT_CAPTURE_BYTES: usize = 64 * 1024;

const REDACTION_MARKER: &str = "***REDACTED***";

#[derive(Debug, Error)]
pub enum HookLaunchError {
    #[error("hook workspace does not exist: {0}")]
    WorkspaceMissing(PathBuf),
    #[error("failed to spawn hook process: {0}")]
    Spawn(io::Error),
    #[error("failed to capture hook stdio: {0}")]
    Capture(io::Error),
}

#[derive(Debug, Clone)]
pub enum HookOutcome {
    /// Process exited with status 0 within the timeout.
    Success { stdout: String, stderr: String },
    /// Process exited with a non-zero status within the timeout.
    Failed {
        exit: ExitStatus,
        stdout: String,
        stderr: String,
    },
    /// Wall-clock timeout elapsed; the process was killed. Captured
    /// stdout/stderr is whatever the reader threads observed before
    /// the kill.
    TimedOut { stdout: String, stderr: String },
}

#[derive(Debug, Clone)]
pub struct HookRunner {
    workspace: PathBuf,
    secrets: Vec<String>,
    capture_bytes: usize,
}

impl HookRunner {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            workspace: workspace.into(),
            secrets: Vec::new(),
            capture_bytes: DEFAULT_CAPTURE_BYTES,
        }
    }

    pub fn with_secrets(mut self, secrets: Vec<String>) -> Self {
        self.secrets = secrets.into_iter().filter(|s| !s.is_empty()).collect();
        self
    }

    pub fn with_capture_bytes(mut self, bytes: usize) -> Self {
        self.capture_bytes = bytes;
        self
    }

    /// Run `hook` synchronously. Returns `HookOutcome::Success` only when
    /// the process exited cleanly within the timeout; any other case
    /// (non-zero exit, timeout, killed) is reflected in the variant.
    /// Launch-time failures (cwd missing, spawn errors) return the
    /// distinct `HookLaunchError` so the caller does not conflate them
    /// with normal process failures.
    pub fn run(&self, hook: &HookCommand) -> Result<HookOutcome, HookLaunchError> {
        if !self.workspace.is_dir() {
            return Err(HookLaunchError::WorkspaceMissing(self.workspace.clone()));
        }

        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&hook.command)
            .current_dir(&self.workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Run sh as its own process-group leader so a kill on timeout
        // reaches every grandchild (e.g. `sleep` under `sh -c`). Without
        // this the orphaned grandchild keeps the stdio pipes open and
        // the reader threads block until it exits on its own.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn().map_err(HookLaunchError::Spawn)?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| HookLaunchError::Capture(io::Error::other("child stdout not piped")))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| HookLaunchError::Capture(io::Error::other("child stderr not piped")))?;

        let stdout_buf = Arc::new(Mutex::new(BoundedBuf::new(self.capture_bytes)));
        let stderr_buf = Arc::new(Mutex::new(BoundedBuf::new(self.capture_bytes)));

        let stdout_thread = spawn_reader(stdout, Arc::clone(&stdout_buf));
        let stderr_thread = spawn_reader(stderr, Arc::clone(&stderr_buf));

        // `Instant + Duration` panics on overflow, so a maliciously large
        // `timeout_ms` would crash the process. Saturate to a 24h ceiling —
        // workflow validators already require `timeout_ms > 0`; this guards
        // the upper edge.
        let timeout = Duration::from_millis(hook.timeout_ms);
        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(24 * 60 * 60));
        let mut timed_out = false;
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => break,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        kill_process_group(&mut child);
                        let _ = child.wait();
                        timed_out = true;
                        break;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(e) => return Err(HookLaunchError::Spawn(e)),
            }
        }

        let _ = stdout_thread.join();
        let _ = stderr_thread.join();

        let stdout = stdout_buf.lock().expect("stdout buf poisoned").finish();
        let stderr = stderr_buf.lock().expect("stderr buf poisoned").finish();
        let stdout = self.redact(stdout);
        let stderr = self.redact(stderr);

        if timed_out {
            tracing::warn!(
                target: "cadenza.workspace.hook",
                workspace = %self.workspace.display(),
                command = %hook.command,
                timeout_ms = hook.timeout_ms,
                "hook timed out, killed",
            );
            return Ok(HookOutcome::TimedOut { stdout, stderr });
        }

        // try_wait already drained the status; re-wait is safe and returns the
        // cached exit info on most platforms.
        let status = child.wait().map_err(HookLaunchError::Spawn)?;
        if status.success() {
            Ok(HookOutcome::Success { stdout, stderr })
        } else {
            Ok(HookOutcome::Failed {
                exit: status,
                stdout,
                stderr,
            })
        }
    }

    fn redact(&self, mut text: String) -> String {
        for secret in &self.secrets {
            if !secret.is_empty() && text.contains(secret) {
                text = text.replace(secret, REDACTION_MARKER);
            }
        }
        text
    }
}

#[derive(Debug)]
struct BoundedBuf {
    cap: usize,
    buf: Vec<u8>,
    overflowed: bool,
}

impl BoundedBuf {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            buf: Vec::with_capacity(cap.min(8 * 1024)),
            overflowed: false,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        if self.buf.len() >= self.cap {
            self.overflowed = true;
            return;
        }
        let take = (self.cap - self.buf.len()).min(chunk.len());
        self.buf.extend_from_slice(&chunk[..take]);
        if take < chunk.len() {
            self.overflowed = true;
        }
    }

    fn finish(&mut self) -> String {
        let mut out = String::from_utf8_lossy(&self.buf).into_owned();
        if self.overflowed {
            out.push_str("\n<truncated; output exceeded capture cap>");
        }
        out
    }
}

/// Send SIGKILL to the entire process group spawned for the hook so
/// grandchildren like `sleep` (under `sh -c "sleep 5"`) die immediately
/// and release their end of the stdio pipes. On non-unix platforms we
/// fall back to `Child::kill`.
fn kill_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as libc::pid_t;
        // SAFETY: pid is a valid PID owned by us; killpg with SIGKILL has
        // no preconditions beyond a valid pgid.
        unsafe {
            libc::killpg(pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    sink: Arc<Mutex<BoundedBuf>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    sink.lock().expect("buf poisoned").push(&chunk[..n]);
                }
                Err(_) => break,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        (dir, path)
    }

    fn hook(command: &str, timeout_ms: u64) -> HookCommand {
        HookCommand {
            command: command.to_string(),
            timeout_ms,
        }
    }

    #[test]
    fn rejects_missing_workspace() {
        let runner = HookRunner::new(PathBuf::from("/this/does/not/exist/cadenza"));
        let err = runner.run(&hook("true", 1_000)).unwrap_err();
        assert!(
            matches!(err, HookLaunchError::WorkspaceMissing(_)),
            "got {err:?}",
        );
    }

    #[test]
    fn cwd_is_the_issue_workspace() {
        let (_dir, ws) = workspace();
        let runner = HookRunner::new(&ws);
        // Resolve symlinks (macOS /var → /private/var) so the assertion is stable.
        let canonical_ws = std::fs::canonicalize(&ws).unwrap();
        let out = runner.run(&hook("pwd", 5_000)).unwrap();
        match out {
            HookOutcome::Success { stdout, .. } => {
                let printed = std::fs::canonicalize(stdout.trim()).unwrap();
                assert_eq!(printed, canonical_ws);
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[test]
    fn success_captures_stdout_and_stderr() {
        let (_dir, ws) = workspace();
        let runner = HookRunner::new(&ws);
        let out = runner
            .run(&hook("echo hello && echo trouble 1>&2", 5_000))
            .unwrap();
        match out {
            HookOutcome::Success { stdout, stderr } => {
                assert!(stdout.contains("hello"), "stdout: {stdout}");
                assert!(stderr.contains("trouble"), "stderr: {stderr}");
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[test]
    fn non_zero_exit_is_failed_outcome() {
        let (_dir, ws) = workspace();
        let runner = HookRunner::new(&ws);
        let out = runner.run(&hook("exit 7", 5_000)).unwrap();
        match out {
            HookOutcome::Failed { exit, .. } => assert_eq!(exit.code(), Some(7)),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    // Boundary: a maliciously large `timeout_ms` must not cause Instant
    // overflow inside the runner. We do not actually wait that long —
    // `true` exits immediately, so the loop unrolls on the first
    // try_wait. The point is that constructing `deadline` with u64::MAX
    // did not panic.
    #[test]
    fn upper_boundary_u64_max_timeout_does_not_panic() {
        let (_dir, ws) = workspace();
        let runner = HookRunner::new(&ws);
        let out = runner.run(&hook("true", u64::MAX)).unwrap();
        assert!(matches!(out, HookOutcome::Success { .. }), "got {out:?}");
    }

    #[test]
    fn timeout_kills_long_running_process() {
        let (_dir, ws) = workspace();
        let runner = HookRunner::new(&ws);
        let start = Instant::now();
        let out = runner.run(&hook("sleep 5", 200)).unwrap();
        let elapsed = start.elapsed();
        assert!(matches!(out, HookOutcome::TimedOut { .. }), "got {out:?}");
        assert!(
            elapsed < Duration::from_secs(2),
            "expected timeout under 2s, took {elapsed:?}",
        );
    }

    // Boundary law on the output cap: =N (under cap) vs =N+1 (just over).
    #[test]
    fn output_under_cap_is_not_truncated() {
        let (_dir, ws) = workspace();
        let runner = HookRunner::new(&ws).with_capture_bytes(100);
        let out = runner.run(&hook("printf %s 'hello'", 5_000)).unwrap();
        match out {
            HookOutcome::Success { stdout, .. } => {
                assert!(!stdout.contains("truncated"), "stdout: {stdout}");
                assert!(stdout.contains("hello"));
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[test]
    fn output_at_cap_plus_one_is_truncated() {
        let (_dir, ws) = workspace();
        // Cap exactly equals the produced payload size so the next byte
        // overflows. seq 1..20 prints 20 short lines (~50+ bytes).
        let runner = HookRunner::new(&ws).with_capture_bytes(16);
        let out = runner.run(&hook("seq 1 20", 5_000)).unwrap();
        match out {
            HookOutcome::Success { stdout, .. } => {
                assert!(stdout.contains("truncated"), "stdout: {stdout}");
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[test]
    fn secret_value_is_redacted_from_stdout() {
        let (_dir, ws) = workspace();
        let runner = HookRunner::new(&ws).with_secrets(vec!["lr_tok_abcdef".into()]);
        let out = runner
            .run(&hook("echo using token lr_tok_abcdef now", 5_000))
            .unwrap();
        match out {
            HookOutcome::Success { stdout, .. } => {
                assert!(!stdout.contains("lr_tok_abcdef"), "stdout: {stdout}");
                assert!(stdout.contains("***REDACTED***"), "stdout: {stdout}");
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[test]
    fn empty_secret_string_is_ignored() {
        // Without the filter, an empty needle would match between every
        // byte and explode the output. `with_secrets` must drop empties.
        let (_dir, ws) = workspace();
        let runner = HookRunner::new(&ws).with_secrets(vec!["".into()]);
        let out = runner.run(&hook("echo plain", 5_000)).unwrap();
        match out {
            HookOutcome::Success { stdout, .. } => assert!(stdout.contains("plain")),
            other => panic!("expected Success, got {other:?}"),
        }
    }
}
