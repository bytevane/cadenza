//! Host capability implementations for the `cadenza:runtime@0.2.0`
//! `tool-runtime` world (issue #16).
//!
//! Only the four in-scope imports are implemented and linked:
//! `host-log`, `host-time`, `host-workspace`, `host-secrets`. `host-http`,
//! `host-linear`, and `host-tools` are deliberately *not* linked — the
//! example guest does not import them, so instantiation succeeds without
//! them, and they remain deferred to their own issues (see ADR 0005).
//!
//! Security posture:
//! - `workspace-read` resolves the guest path through the `cadenza-workspace`
//!   containment APIs (lexical `safe_join` + symlink-aware
//!   `canonicalize_inside`); escapes surface as `host-error::outside-root`.
//! - `secret-exists` answers from a presence-only name set; no value is ever
//!   reachable through the WIT.
//! - `log` redacts the message and fields with the shared `cadenza-obs`
//!   `Scrubber` before anything is recorded.
//! - Every host call records issue/plugin context via the captured log sink
//!   and a `tracing` event keyed by the `cadenza-obs` field-name constants.
//! - Host errors never echo absolute host paths back to the guest.

use std::io::{Read, Seek, SeekFrom};

use camino::Utf8Path;
use wasmtime::Store;
use wasmtime::component::{HasSelf, Linker};

use crate::{ComponentRuntime, HostLogRecord, LoadedComponent, StoreState, WasmHostError};

wasmtime::component::bindgen!({
    path: "../../wit",
    world: "tool-runtime",
});

pub use cadenza::runtime::types::{
    HostError, LogLevel, ToolInput, ToolOutput, WorkspaceReadResult,
};

/// Wire the four in-scope host interfaces into `linker`. The guest's
/// incidental WASI imports are stubbed as traps in
/// [`ComponentRuntime::run_tool`] rather than granted.
pub(crate) fn add_host_capabilities(linker: &mut Linker<StoreState>) -> Result<(), WasmHostError> {
    let link_err = |e: wasmtime::Error| WasmHostError::Link(e.to_string());
    cadenza::runtime::host_log::add_to_linker::<_, HasSelf<StoreState>>(linker, |s| s)
        .map_err(link_err)?;
    cadenza::runtime::host_time::add_to_linker::<_, HasSelf<StoreState>>(linker, |s| s)
        .map_err(link_err)?;
    cadenza::runtime::host_workspace::add_to_linker::<_, HasSelf<StoreState>>(linker, |s| s)
        .map_err(link_err)?;
    cadenza::runtime::host_secrets::add_to_linker::<_, HasSelf<StoreState>>(linker, |s| s)
        .map_err(link_err)?;
    Ok(())
}

impl ComponentRuntime {
    /// Instantiate `loaded` against a fresh linker carrying the four host
    /// capabilities, then call the guest's exported `tool.run`.
    ///
    /// The outer `Result` carries host/trap failures; the inner `Result` is
    /// the guest's own `result<tool-output, host-error>` return, so callers
    /// observe the shared WIT error model end to end.
    pub fn run_tool(
        &self,
        store: &mut Store<StoreState>,
        loaded: &LoadedComponent,
        input: ToolInput,
    ) -> Result<Result<ToolOutput, HostError>, WasmHostError> {
        let mut linker = Linker::<StoreState>::new(self.engine());
        // Stub *every* import as a trap first, then shadow the four in-scope
        // host interfaces with their real implementations. The guest's
        // incidental WASI imports (from the Rust std runtime) therefore grant
        // nothing — no preopens, env, clocks, random, sockets, or filesystem
        // reach the guest; a guest that calls a WASI function traps (surfaced
        // via `classify_trap`). The only live capabilities are the four host
        // functions, satisfying the issue's "minimal capability" requirement.
        linker.allow_shadowing(true);
        linker
            .define_unknown_imports_as_traps(&loaded.component)
            .map_err(|e| WasmHostError::Link(e.to_string()))?;
        add_host_capabilities(&mut linker)?;
        linker.allow_shadowing(false);
        let bindings = ToolRuntime::instantiate(&mut *store, &loaded.component, &linker)
            .map_err(classify_instantiate)?;
        let result = bindings
            .cadenza_runtime_tool()
            .call_run(&mut *store, &input)
            .map_err(crate::classify_trap)?;
        Ok(result)
    }
}

impl StoreState {
    /// Append a host-call record to the log sink and emit a `tracing` event
    /// stamped with issue/plugin context. The field idents below mirror the
    /// `cadenza-obs` field-name constants (pinned by a unit test).
    fn record_call(
        &self,
        op: &str,
        level: Option<&str>,
        guest_target: Option<&str>,
        message: Option<String>,
        fields_json: Option<String>,
    ) {
        tracing::debug!(
            target: "cadenza.wasm_host.capability",
            op = op,
            issue_id = self.request.issue_id.as_deref().unwrap_or(""),
            plugin_name = self.request.plugin_name.as_deref().unwrap_or(""),
            component = "cadenza-wasm-host",
            guest_target = guest_target.unwrap_or(""),
        );
        self.caps.log_sink.push(HostLogRecord {
            op: op.to_string(),
            issue_id: self.request.issue_id.clone(),
            plugin_name: self.request.plugin_name.clone(),
            level: level.map(str::to_string),
            message,
            fields_json,
        });
    }
}

impl cadenza::runtime::host_time::Host for StoreState {
    fn now_millis(&mut self) -> u64 {
        let now = self.caps.clock.now_millis();
        self.record_call("host-time.now-millis", None, None, None, None);
        now
    }
}

impl cadenza::runtime::host_log::Host for StoreState {
    fn log(
        &mut self,
        level: LogLevel,
        target: Option<String>,
        message: String,
        fields_json: Option<String>,
    ) -> Result<(), HostError> {
        let scrubbed_message = self.caps.scrubber.scrub_text(&message);
        let scrubbed_fields = match fields_json {
            Some(raw) => Some(scrub_fields(&self.caps.scrubber, &raw)?),
            None => None,
        };
        let scrubbed_target = target.map(|t| self.caps.scrubber.scrub_text(&t));
        self.record_call(
            "host-log.log",
            Some(level_label(level)),
            scrubbed_target.as_deref(),
            Some(scrubbed_message),
            scrubbed_fields,
        );
        Ok(())
    }
}

impl cadenza::runtime::host_workspace::Host for StoreState {
    fn workspace_read(
        &mut self,
        path: String,
        offset: Option<u64>,
        limit: Option<u64>,
        as_text: bool,
    ) -> Result<WorkspaceReadResult, HostError> {
        let result = read_workspace(self, path, offset, limit, as_text);
        self.record_call("host-workspace.workspace-read", None, None, None, None);
        result
    }
}

impl cadenza::runtime::host_secrets::Host for StoreState {
    fn secret_exists(&mut self, name: String) -> Result<bool, HostError> {
        self.record_call("host-secrets.secret-exists", None, None, None, None);
        if name.is_empty() {
            return Err(HostError::InvalidArgument(
                "secret name must not be empty".to_string(),
            ));
        }
        // Presence only — the value is never stored or returned.
        Ok(self.caps.secret_names.contains(&name))
    }
}

fn level_label(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Trace => "trace",
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    }
}

/// Resolve and read a workspace-relative path with containment + symlink
/// safety, then apply the `offset`/`limit` window.
fn read_workspace(
    state: &StoreState,
    path: String,
    offset: Option<u64>,
    limit: Option<u64>,
    as_text: bool,
) -> Result<WorkspaceReadResult, HostError> {
    let root =
        state.request.workspace_path.as_ref().ok_or_else(|| {
            HostError::Denied("no workspace configured for this request".to_string())
        })?;
    let root_utf8 = Utf8Path::from_path(root)
        .ok_or_else(|| HostError::Internal("workspace root path is not valid UTF-8".to_string()))?;

    // 1. Lexical containment: rejects absolute segments and `..` escapes.
    let candidate = cadenza_workspace::safe_join(root_utf8, &path).map_err(map_workspace_error)?;
    // 2. Symlink-aware containment that returns the *resolved* path; a missing
    //    file surfaces here as not-found. Opening this exact path (rather than
    //    re-canonicalising `candidate`) keeps the validated path and the opened
    //    path consistent under a concurrent symlink swap. A residual race on
    //    the final component is a known std limitation; a write-capable API
    //    will need O_NOFOLLOW/openat2 (SECURITY.md) and is out of scope here.
    let resolved = cadenza_workspace::resolve_inside(root.as_path(), candidate.as_std_path())
        .map_err(map_workspace_error)?;
    // 3. Read only the requested window (seek + bounded read), never the whole
    //    file: a guest asking for a tiny slice of a huge in-root file must not
    //    force the host to allocate the entire file. An unbounded request
    //    (`limit == None`) is hard-capped at `MAX_WORKSPACE_READ_BYTES`.
    let mut file = std::fs::File::open(&resolved).map_err(map_io_error)?;
    let total_len = file.metadata().map_err(map_io_error)?.len();
    let (bytes, truncated) = read_window(
        &mut file,
        total_len,
        offset,
        limit,
        MAX_WORKSPACE_READ_BYTES,
    )
    .map_err(map_io_error)?;

    if as_text && std::str::from_utf8(&bytes).is_err() {
        return Err(HostError::InvalidArgument(
            "requested as-text but bytes are not valid UTF-8".to_string(),
        ));
    }

    Ok(WorkspaceReadResult {
        path,
        bytes,
        truncated,
    })
}

/// Hard cap on a single `workspace-read` when the guest gives no explicit
/// `limit`, so an unbounded request cannot force the host to allocate an
/// arbitrarily large file. A guest wanting more must page with `offset`.
const MAX_WORKSPACE_READ_BYTES: u64 = 4 * 1024 * 1024;

/// Read the `offset`/`limit` window from `src` (a seekable reader of known
/// `total_len`) without loading the whole file. The window size is
/// `limit`, hard-capped at `cap`; `truncated` is true when bytes remain past
/// the returned window. An `offset` beyond EOF yields an empty, non-truncated
/// read rather than an error.
fn read_window<R: Read + Seek>(
    src: &mut R,
    total_len: u64,
    offset: Option<u64>,
    limit: Option<u64>,
    cap: u64,
) -> std::io::Result<(Vec<u8>, bool)> {
    let start = offset.unwrap_or(0).min(total_len);
    src.seek(SeekFrom::Start(start))?;
    let window = limit.unwrap_or(cap).min(cap);
    let mut buf = Vec::new();
    src.take(window).read_to_end(&mut buf)?;
    let remaining = total_len - start;
    let truncated = remaining > buf.len() as u64;
    Ok((buf, truncated))
}

/// Map a `ToolRuntime::instantiate` failure onto a typed host error.
/// Instantiation can fail either because the linker is missing wiring (a
/// `Link` problem) or because guest initialization code traps / breaches a
/// resource limit; the latter must surface as `Timeout`/`LimitBreached` so
/// callers can branch on the cause.
fn classify_instantiate(err: wasmtime::Error) -> WasmHostError {
    if err.downcast_ref::<wasmtime::Trap>().is_some() {
        crate::classify_trap(err)
    } else {
        WasmHostError::Link(format!("instantiate: {err}"))
    }
}

/// Map a `cadenza-workspace` error onto the shared WIT `host-error`. Escapes
/// collapse to `outside-root`; errors never echo the absolute host path.
fn map_workspace_error(err: cadenza_workspace::WorkspaceError) -> HostError {
    use cadenza_workspace::WorkspaceError as W;
    use std::io::ErrorKind;
    match err {
        W::OutsideRoot { .. } | W::Traversal { .. } => HostError::OutsideRoot,
        W::AbsoluteSegment { .. } => HostError::InvalidArgument(
            "absolute paths are not permitted; use a workspace-relative path".to_string(),
        ),
        W::RootNotAbsolute(_) => HostError::Internal("workspace root is not absolute".to_string()),
        W::Canonicalize { source, .. } => match source.kind() {
            ErrorKind::NotFound => HostError::NotFound("path not found in workspace".to_string()),
            ErrorKind::PermissionDenied => HostError::Denied("permission denied".to_string()),
            _ => HostError::Io(source.to_string()),
        },
    }
}

fn map_io_error(err: std::io::Error) -> HostError {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::NotFound => HostError::NotFound("path not found in workspace".to_string()),
        ErrorKind::PermissionDenied => HostError::Denied("permission denied".to_string()),
        _ => HostError::Io(err.to_string()),
    }
}

/// Redact a guest-supplied `fields-json` string. Valid JSON is walked and
/// each value redacted by key shape / value substring via the shared
/// `Scrubber`; non-JSON input is rejected as `invalid-argument`.
fn scrub_fields(scrubber: &cadenza_obs::Scrubber, raw: &str) -> Result<String, HostError> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| HostError::InvalidArgument(format!("fields-json is not valid JSON: {e}")))?;
    Ok(scrub_json(scrubber, None, value).to_string())
}

fn scrub_json(
    scrubber: &cadenza_obs::Scrubber,
    key: Option<&str>,
    value: serde_json::Value,
) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(s) => match key {
            Some(k) => Value::String(scrubber.redact_key_value(k, &s)),
            None => Value::String(scrubber.scrub_text(&s)),
        },
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                // A secret-shaped key with a non-string value (number, bool,
                // nested object) is still redacted wholesale. Route through
                // the scrubber so the redaction marker stays owned by
                // cadenza-obs rather than duplicated here.
                let new_v = if cadenza_obs::looks_secret(&k) && !v.is_string() {
                    Value::String(scrubber.redact_key_value(&k, ""))
                } else {
                    scrub_json(scrubber, Some(&k), v)
                };
                out.insert(k, new_v);
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(|v| scrub_json(scrubber, key, v))
                .collect(),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadenza_obs::Scrubber;
    use std::io::{Error, ErrorKind};

    /// Drive `read_window` over an in-memory `Cursor` (a `Read + Seek`).
    fn win(data: &[u8], offset: Option<u64>, limit: Option<u64>, cap: u64) -> (Vec<u8>, bool) {
        read_window(
            &mut std::io::Cursor::new(data.to_vec()),
            data.len() as u64,
            offset,
            limit,
            cap,
        )
        .unwrap()
    }

    const BIG_CAP: u64 = u64::MAX;

    #[test]
    fn read_window_no_limit_returns_all_within_cap() {
        let (bytes, truncated) = win(b"hello", None, None, BIG_CAP);
        assert_eq!(bytes, b"hello");
        assert!(!truncated);
    }

    #[test]
    fn read_window_limit_below_length_truncates() {
        let (bytes, truncated) = win(b"hello", None, Some(3), BIG_CAP);
        assert_eq!(bytes, b"hel");
        assert!(truncated);
    }

    #[test]
    fn read_window_limit_equal_length_is_not_truncated() {
        // Paired-edge with the case above: limit == len is the boundary where
        // truncation flips off.
        let (bytes, truncated) = win(b"hello", None, Some(5), BIG_CAP);
        assert_eq!(bytes, b"hello");
        assert!(!truncated);
    }

    #[test]
    fn read_window_offset_then_limit() {
        let (bytes, truncated) = win(b"hello world", Some(6), Some(3), BIG_CAP);
        assert_eq!(bytes, b"wor");
        assert!(truncated);
    }

    #[test]
    fn read_window_offset_at_eof_is_empty_not_truncated() {
        let (bytes, truncated) = win(b"hello", Some(5), None, BIG_CAP);
        assert!(bytes.is_empty());
        assert!(!truncated);
    }

    #[test]
    fn read_window_offset_past_eof_clamps() {
        let (bytes, truncated) = win(b"hello", Some(99), Some(4), BIG_CAP);
        assert!(bytes.is_empty());
        assert!(!truncated);
    }

    #[test]
    fn read_window_caps_unbounded_request() {
        // No explicit limit, but the cap bounds the read and flags truncation
        // so a huge in-root file cannot force a full-file host allocation.
        let (bytes, truncated) = win(b"0123456789", None, None, 4);
        assert_eq!(bytes, b"0123");
        assert!(truncated);
    }

    #[test]
    fn read_window_limit_above_cap_is_capped() {
        let (bytes, truncated) = win(b"0123456789", None, Some(999), 4);
        assert_eq!(bytes, b"0123");
        assert!(truncated);
    }

    #[test]
    fn workspace_error_escape_maps_to_outside_root() {
        let err = cadenza_workspace::WorkspaceError::OutsideRoot {
            root: "/r".into(),
            candidate: "/etc".into(),
        };
        assert!(matches!(map_workspace_error(err), HostError::OutsideRoot));
        let traversal = cadenza_workspace::WorkspaceError::Traversal {
            candidate: "/..".into(),
        };
        assert!(matches!(
            map_workspace_error(traversal),
            HostError::OutsideRoot
        ));
    }

    #[test]
    fn workspace_error_absolute_segment_is_invalid_argument() {
        let err = cadenza_workspace::WorkspaceError::AbsoluteSegment {
            segment: "/etc/passwd".into(),
        };
        assert!(matches!(
            map_workspace_error(err),
            HostError::InvalidArgument(_)
        ));
    }

    #[test]
    fn workspace_error_missing_canonicalize_target_is_not_found() {
        let err = cadenza_workspace::WorkspaceError::Canonicalize {
            path: "x".into(),
            source: Error::from(ErrorKind::NotFound),
        };
        assert!(matches!(map_workspace_error(err), HostError::NotFound(_)));
    }

    #[test]
    fn scrub_fields_redacts_secret_shaped_keys_and_values() {
        let scrubber = Scrubber::with_secrets(vec!["lr_tok_secret".to_string()]);
        let out = scrub_fields(
            &scrubber,
            r#"{"LINEAR_API_KEY":"abc","note":"uses lr_tok_secret","count":3}"#,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["LINEAR_API_KEY"], "[REDACTED]");
        assert!(
            !out.contains("lr_tok_secret"),
            "registered value leaked: {out}"
        );
        assert_eq!(parsed["count"], 3);
    }

    #[test]
    fn scrub_fields_redacts_secret_key_with_non_string_value() {
        let scrubber = Scrubber::empty();
        let out = scrub_fields(&scrubber, r#"{"api_key":12345,"ok":true}"#).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["api_key"], "[REDACTED]");
        assert_eq!(parsed["ok"], true);
    }

    #[test]
    fn scrub_fields_rejects_non_json() {
        let scrubber = Scrubber::empty();
        assert!(matches!(
            scrub_fields(&scrubber, "not json"),
            Err(HostError::InvalidArgument(_))
        ));
    }

    #[test]
    fn obs_field_constants_match_tracing_idents() {
        // `record_call` emits literal `tracing` field idents; pin them to the
        // canonical `cadenza-obs` constants so a rename of the contract is
        // caught here instead of silently drifting.
        assert_eq!(cadenza_obs::fields::FIELD_ISSUE_ID, "issue_id");
        assert_eq!(cadenza_obs::fields::FIELD_PLUGIN_NAME, "plugin_name");
        assert_eq!(cadenza_obs::fields::FIELD_COMPONENT, "component");
    }

    #[test]
    fn classify_instantiate_maps_non_trap_to_link() {
        let err = wasmtime::Error::msg("unknown import: missing");
        assert!(matches!(classify_instantiate(err), WasmHostError::Link(_)));
    }

    #[test]
    fn classify_instantiate_maps_interrupt_trap_to_timeout() {
        // An epoch interruption during guest init must surface as Timeout, not
        // as a linker wiring failure.
        let err = wasmtime::Error::from(wasmtime::Trap::Interrupt);
        assert!(matches!(classify_instantiate(err), WasmHostError::Timeout));
    }

    #[test]
    fn classify_instantiate_maps_other_trap_to_limit_breached() {
        let err = wasmtime::Error::from(wasmtime::Trap::UnreachableCodeReached);
        assert!(matches!(
            classify_instantiate(err),
            WasmHostError::LimitBreached(_)
        ));
    }

    // Guards the symlink half of containment specifically: lexical `safe_join`
    // passes a symlink that lives inside the root, so only `canonicalize_inside`
    // catches the escape. Removing that call makes this test read the outside
    // target and fail.
    #[cfg(unix)]
    #[test]
    fn read_workspace_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("ws");
        let outside = outer.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"sensitive").unwrap();
        // A symlink that *lexically* lives inside the root but resolves out.
        symlink(outside.join("secret.txt"), root.join("link.txt")).unwrap();

        let rt = crate::ComponentRuntime::new(crate::WasmRuntimeLimits::default()).unwrap();
        let store = rt.new_store_with(
            crate::RequestContext {
                workspace_path: Some(root.clone()),
                ..Default::default()
            },
            crate::HostCapabilities::default(),
        );

        let err = read_workspace(store.data(), "link.txt".to_string(), None, None, true)
            .expect_err("symlink escape must be denied");
        assert!(
            matches!(err, HostError::OutsideRoot),
            "symlink escape must map to outside-root, got {err:?}",
        );
    }
}
