//! Codex app-server stdio launcher + initialize handshake.
//!
//! `AppServerLauncher` spawns the pinned Codex CLI under `bash -lc <command>`
//! with the per-issue workspace as cwd, opens piped stdin/stdout/stderr,
//! sends one JSON-RPC `initialize` request, parses the response, and
//! returns an `AppServerClient` holding the live child process. Stderr is
//! collected on a background task into a bounded, redacted buffer; stdout
//! is the protocol channel and is not freely tee'd anywhere.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::protocol::{
    ClientInfo, InitializeCapabilities, InitializeParams, InitializeResponse, JsonRpcRequest,
    JsonRpcResponse,
};

/// Per-stream cap for captured stderr. Codex stderr is logs, not data;
/// 64 KiB is enough for an operator post-mortem without unbounded growth.
pub const DEFAULT_STDERR_CAP_BYTES: usize = 64 * 1024;
const REDACTION_MARKER: &str = "***REDACTED***";

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("workspace does not exist: {0}")]
    WorkspaceMissing(PathBuf),
    #[error("workspace path is not absolute: {0}")]
    WorkspaceNotAbsolute(PathBuf),
    #[error("failed to spawn codex app-server: {0}")]
    Spawn(std::io::Error),
    #[error("failed to attach to codex stdio: {0}")]
    Stdio(std::io::Error),
    #[error("failed to encode initialize request: {0}")]
    Encode(serde_json::Error),
    #[error("failed to write initialize request: {0}")]
    Write(std::io::Error),
    #[error("failed to read initialize response: {0}")]
    Read(std::io::Error),
    #[error("initialize handshake timed out after {0:?}")]
    Timeout(Duration),
    #[error("initialize response was not valid JSON: {0}")]
    Decode(serde_json::Error),
    #[error(
        "codex app-server reported an error: code={code} message={message} stderr_tail={stderr_tail}"
    )]
    Protocol {
        code: i64,
        message: String,
        stderr_tail: String,
    },
    #[error(
        "codex app-server exited before sending initialize response (stderr_tail={stderr_tail})"
    )]
    EarlyExit { stderr_tail: String },
}

/// Builder + entry point for spawning a Codex app-server.
#[derive(Debug, Clone)]
pub struct AppServerLauncher {
    command: String,
    workspace: PathBuf,
    startup_timeout: Duration,
    client_info: ClientInfo,
    capabilities: Option<InitializeCapabilities>,
    secrets: Vec<String>,
    stderr_cap_bytes: usize,
}

impl AppServerLauncher {
    /// `command` is the shell-style string Cadenza's `WorkflowConfig.codex.command`
    /// already validates (e.g. `"codex app-server --listen stdio://"`); it is
    /// run through `bash -lc`.
    pub fn new(command: impl Into<String>, workspace: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
            workspace: workspace.into(),
            startup_timeout: Duration::from_secs(15),
            client_info: ClientInfo {
                name: "cadenza".into(),
                title: Some("Cadenza orchestrator".into()),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            capabilities: None,
            secrets: Vec::new(),
            stderr_cap_bytes: DEFAULT_STDERR_CAP_BYTES,
        }
    }

    pub fn with_startup_timeout(mut self, d: Duration) -> Self {
        self.startup_timeout = d;
        self
    }

    pub fn with_client_info(mut self, info: ClientInfo) -> Self {
        self.client_info = info;
        self
    }

    pub fn with_capabilities(mut self, caps: InitializeCapabilities) -> Self {
        self.capabilities = Some(caps);
        self
    }

    pub fn with_secrets(mut self, secrets: Vec<String>) -> Self {
        // Apply longest secrets first so a shorter prefix cannot leak a
        // longer secret's suffix into the captured stderr.
        let mut secrets: Vec<String> = secrets.into_iter().filter(|s| !s.is_empty()).collect();
        secrets.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
        self.secrets = secrets;
        self
    }

    pub fn with_stderr_cap_bytes(mut self, bytes: usize) -> Self {
        self.stderr_cap_bytes = bytes;
        self
    }

    pub async fn launch(self) -> Result<AppServerClient, LaunchError> {
        if !self.workspace.is_absolute() {
            return Err(LaunchError::WorkspaceNotAbsolute(self.workspace));
        }
        if !self.workspace.is_dir() {
            return Err(LaunchError::WorkspaceMissing(self.workspace));
        }

        let mut cmd = Command::new("bash");
        cmd.arg("-lc")
            .arg(&self.command)
            .current_dir(&self.workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            cmd.process_group(0);
        }
        let mut child = cmd.spawn().map_err(LaunchError::Spawn)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LaunchError::Stdio(std::io::Error::other("child stdin not piped")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LaunchError::Stdio(std::io::Error::other("child stdout not piped")))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| LaunchError::Stdio(std::io::Error::other("child stderr not piped")))?;

        let stderr_buf = Arc::new(Mutex::new(BoundedBuf::new(self.stderr_cap_bytes)));
        let stderr_task = spawn_stderr_capture(stderr, Arc::clone(&stderr_buf));

        let mut stdin = stdin;
        let mut stdout = BufReader::new(stdout);

        let init = InitializeParams {
            client_info: self.client_info.clone(),
            capabilities: self.capabilities.clone(),
        };
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "initialize",
            params: init,
        };
        let payload = match serde_json::to_vec(&request) {
            Ok(bytes) => bytes,
            Err(e) => {
                // Encode fires after spawn — clean up so we never leak the
                // child or the stderr capture task.
                kill_now(&mut child).await;
                let _ = child.wait().await;
                stderr_task.abort();
                let _ = stderr_task.await;
                return Err(LaunchError::Encode(e));
            }
        };
        let write_result = async {
            stdin.write_all(&payload).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await
        };

        let handshake = async {
            write_result.await.map_err(LaunchError::Write)?;
            let mut line = String::new();
            loop {
                line.clear();
                let read = stdout
                    .read_line(&mut line)
                    .await
                    .map_err(LaunchError::Read)?;
                if read == 0 {
                    // The child closed stdout, but the stderr task may
                    // still be draining the last lines that were emitted
                    // right before exit (the most useful diagnostic).
                    // Give it a short bounded chance to settle.
                    settle_stderr(&stderr_buf).await;
                    let tail =
                        redact_text(stderr_buf.lock().await.snapshot_string(), &self.secrets);
                    return Err(LaunchError::EarlyExit { stderr_tail: tail });
                }
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let response: JsonRpcResponse =
                    serde_json::from_str(trimmed).map_err(LaunchError::Decode)?;
                if let Some(err) = response.error {
                    settle_stderr(&stderr_buf).await;
                    let tail =
                        redact_text(stderr_buf.lock().await.snapshot_string(), &self.secrets);
                    return Err(LaunchError::Protocol {
                        code: err.code,
                        message: err.message,
                        stderr_tail: tail,
                    });
                }
                let result = response.result.ok_or_else(|| {
                    LaunchError::Decode(serde::de::Error::custom("missing result"))
                })?;
                let initialize: InitializeResponse =
                    serde_json::from_value(result).map_err(LaunchError::Decode)?;
                return Ok(initialize);
            }
        };

        let initialize_response = match timeout(self.startup_timeout, handshake).await {
            Ok(res) => res,
            Err(_elapsed) => {
                kill_now(&mut child).await;
                let _ = child.wait().await;
                stderr_task.abort();
                let _ = stderr_task.await;
                return Err(LaunchError::Timeout(self.startup_timeout));
            }
        };

        let initialize_response = match initialize_response {
            Ok(r) => r,
            Err(e) => {
                kill_now(&mut child).await;
                let _ = child.wait().await;
                stderr_task.abort();
                let _ = stderr_task.await;
                return Err(e);
            }
        };

        Ok(AppServerClient {
            child,
            stdin,
            stdout,
            initialize_response,
            stderr_buf,
            stderr_task: Some(stderr_task),
            secrets: self.secrets,
        })
    }
}

/// Live Codex app-server connection. After construction the initialize
/// handshake has already succeeded — `initialize_response()` is the
/// authoritative response. Call `shutdown` to gracefully tear down; on
/// drop the kill-on-drop flag terminates the child as a safety net.
#[derive(Debug)]
pub struct AppServerClient {
    child: Child,
    #[allow(dead_code)]
    stdin: ChildStdin,
    #[allow(dead_code)]
    stdout: BufReader<tokio::process::ChildStdout>,
    initialize_response: InitializeResponse,
    stderr_buf: Arc<Mutex<BoundedBuf>>,
    stderr_task: Option<JoinHandle<()>>,
    secrets: Vec<String>,
}

impl AppServerClient {
    pub fn initialize_response(&self) -> &InitializeResponse {
        &self.initialize_response
    }

    /// Bounded, redacted snapshot of everything the child has written to
    /// stderr so far. Useful for surfacing handshake-time log lines on
    /// failure / for inclusion in state snapshots.
    pub async fn stderr_snapshot(&self) -> String {
        let raw = self.stderr_buf.lock().await.snapshot_string();
        redact_text(raw, &self.secrets)
    }

    /// Shut down the child process. Sends SIGKILL to the whole group so
    /// any backgrounded grandchildren die together, drains the stderr
    /// capture task, and waits for the child to exit.
    pub async fn shutdown(mut self) -> Result<(), LaunchError> {
        kill_now(&mut self.child).await;
        if let Some(task) = self.stderr_task.take() {
            task.abort();
            let _ = task.await;
        }
        let _ = self.child.wait().await;
        Ok(())
    }
}

async fn kill_now(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // SAFETY: pid is valid for the lifetime of this `Child` borrow;
        // killpg with SIGKILL has no preconditions.
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGKILL);
        }
    }
    let _ = child.start_kill();
}

/// Wait briefly for the stderr capture task to drain whatever was emitted
/// right before a child exit. Polls the monotonic `total_pushed` counter
/// up to 200 ms; if two consecutive samples agree the writer has stopped.
/// Using a counter (instead of `buf.len()`) is necessary because once the
/// ring fills its length is constant while the writer can still be
/// streaming new bytes that would push older ones out — the diagnostic
/// tail. Bounded best-effort — the cost is paid only on error paths.
async fn settle_stderr(buf: &Arc<Mutex<BoundedBuf>>) {
    let deadline = std::time::Instant::now() + Duration::from_millis(200);
    let mut last_total: Option<u64> = None;
    loop {
        let total = buf.lock().await.total_pushed;
        if Some(total) == last_total && total > 0 {
            return;
        }
        last_total = Some(total);
        if std::time::Instant::now() >= deadline {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn spawn_stderr_capture(
    stderr: tokio::process::ChildStderr,
    buf: Arc<Mutex<BoundedBuf>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut chunk = [0u8; 4096];
        loop {
            match tokio::io::AsyncReadExt::read(&mut reader, &mut chunk).await {
                Ok(0) => break,
                Ok(n) => buf.lock().await.push(&chunk[..n]),
                Err(_) => break,
            }
        }
    })
}

fn redact_text(mut text: String, secrets: &[String]) -> String {
    for secret in secrets {
        if !secret.is_empty() && text.contains(secret) {
            text = text.replace(secret, REDACTION_MARKER);
        }
    }
    text
}

/// Ring buffer that retains the **most recent** `cap` bytes. Codex
/// failures usually emit the diagnostic line right before exit; keeping
/// the head would discard exactly that. The implementation drops the
/// oldest byte for every new byte once the cap is reached.
///
/// `total_pushed` is a monotonic counter (saturating). `settle_stderr`
/// uses it instead of `buf.len()` because once the ring is full the
/// length is constant — using length would mark "settled" while new
/// bytes (and the actual diagnostic tail) are still arriving.
#[derive(Debug)]
struct BoundedBuf {
    cap: usize,
    buf: std::collections::VecDeque<u8>,
    overflowed: bool,
    total_pushed: u64,
}

impl BoundedBuf {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            buf: std::collections::VecDeque::with_capacity(cap.min(8 * 1024)),
            overflowed: false,
            total_pushed: 0,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        if self.cap == 0 {
            if !chunk.is_empty() {
                self.overflowed = true;
                self.total_pushed = self.total_pushed.saturating_add(chunk.len() as u64);
            }
            return;
        }
        for &b in chunk {
            if self.buf.len() == self.cap {
                self.buf.pop_front();
                self.overflowed = true;
            }
            self.buf.push_back(b);
            self.total_pushed = self.total_pushed.saturating_add(1);
        }
    }

    fn snapshot_string(&self) -> String {
        let bytes: Vec<u8> = self.buf.iter().copied().collect();
        let mut out = String::from_utf8_lossy(&bytes).into_owned();
        if self.overflowed {
            out.insert_str(
                0,
                "<truncated; stderr exceeded capture cap; keeping tail>\n",
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_with_zero_cap_does_not_grow_unbounded() {
        let mut b = BoundedBuf::new(0);
        b.push(b"hello");
        b.push(b"world");
        assert_eq!(b.buf.len(), 0);
        assert!(b.overflowed);
        assert_eq!(b.total_pushed, 10);
        // Snapshot still reflects the overflow marker.
        let s = b.snapshot_string();
        assert!(s.contains("truncated"), "{s}");
    }

    // Boundary: =N (under cap → no overflow) paired with =N+1
    // (cap+1 byte → overflow, oldest evicted, tail retained).
    #[test]
    fn ring_buffer_under_cap_no_overflow() {
        let mut b = BoundedBuf::new(3);
        b.push(b"ab");
        assert!(!b.overflowed);
        assert_eq!(b.buf.iter().copied().collect::<Vec<_>>(), b"ab");
        assert_eq!(b.total_pushed, 2);
    }

    #[test]
    fn ring_buffer_keeps_tail_when_overfilled() {
        let mut b = BoundedBuf::new(3);
        b.push(b"abcdef");
        assert!(b.overflowed);
        assert_eq!(b.buf.iter().copied().collect::<Vec<_>>(), b"def");
        assert_eq!(b.total_pushed, 6);
    }

    #[test]
    fn total_pushed_keeps_climbing_after_ring_fills() {
        // settle_stderr depends on total_pushed continuing to move even
        // when buf.len() has plateaued at cap.
        let mut b = BoundedBuf::new(2);
        b.push(b"abcd");
        let after_first = b.total_pushed;
        b.push(b"e");
        assert!(b.total_pushed > after_first);
    }
}
