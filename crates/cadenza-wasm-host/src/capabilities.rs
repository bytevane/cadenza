//! Host capability implementations for the `cadenza:runtime@0.2.0`
//! `tool-runtime` world (issues #16, #17).
//!
//! Five host imports are implemented and linked: `host-log`, `host-time`,
//! `host-workspace`, `host-secrets` (ADR 0005) and `host-linear` (ADR 0006).
//! `host-http` and `host-tools` are deliberately *not* linked — they remain
//! deferred to their own issues; a guest importing them would trap.
//!
//! Security posture:
//! - `workspace-read` resolves the guest path through the `cadenza-workspace`
//!   containment APIs (lexical `safe_join` + symlink-aware `resolve_inside`,
//!   whose resolved path is the one opened); escapes surface as
//!   `host-error::outside-root`.
//! - `secret-exists` answers from a presence-only name set; no value is ever
//!   reachable through the WIT.
//! - `linear-graphql` injects the operator's Linear credential behind a
//!   host-side transport; the raw token never reaches guest memory, and
//!   upstream error strings are scrubbed before they cross back as a
//!   `host-error`. The WIT gives the guest no header channel, so a
//!   plugin-supplied `Authorization` header is structurally impossible.
//! - `log` redacts the message and fields with the shared `cadenza-obs`
//!   `Scrubber` before anything is recorded.
//! - Every host call records issue/plugin context via the captured log sink
//!   and a `tracing` event keyed by the `cadenza-obs` field-name constants.
//! - Host errors never echo absolute host paths back to the guest.

use std::io::{Read, Seek, SeekFrom};

use camino::Utf8Path;
use wasmtime::Store;
use wasmtime::component::{HasSelf, Linker, types::ComponentItem};

use crate::{
    ComponentRuntime, DeferredCapability, HostLogRecord, LinearCall, LinearMode,
    LinearTransportError, LoadedComponent, StoreState, WasmHostError,
};

wasmtime::component::bindgen!({
    path: "../../wit",
    world: "tool-runtime",
});

pub use cadenza::runtime::types::{
    GraphqlMode, GraphqlResponse, HostError, LogLevel, ToolInput, ToolOutput, WorkspaceReadResult,
};

/// Stub every import of `component` so the guest is granted nothing by
/// default: each *function* import becomes a host function that returns a
/// typed [`DeferredCapability`] error, while resources and nested instances
/// get the structural stubs wasmtime's instantiation type-check requires.
/// This mirrors wasmtime's own `Linker::define_unknown_imports_as_traps`, but
/// the error payload is downcastable in [`classify_trap`] so a guest call into
/// a deferred capability (`host-http`, `host-tools`, or any incidental WASI
/// import the rustc-emitted Rust runtime drags in) surfaces as
/// `WasmHostError::CapabilityDenied` rather than the less-precise
/// `LimitBreached` a stringly-typed trap would land on (issue #75 case 2).
///
/// We do the full recursion ourselves rather than calling
/// `define_unknown_imports_as_traps` and overriding afterwards: re-entering an
/// instance namespace with `Linker::instance` inserts a *fresh* empty
/// namespace (it does not append), which would discard the resource stubs
/// wasmtime had installed for instances like `wasi:io/poll` and break the
/// instantiation type-check.
///
/// Pre-condition: `linker.allow_shadowing(true)` must be in effect (set by the
/// caller in `run_tool`); [`add_host_capabilities`] then shadows the in-scope
/// interfaces with their real implementations.
fn define_imports_as_capability_denied(
    linker: &mut Linker<StoreState>,
    component: &wasmtime::component::Component,
    engine: &wasmtime::Engine,
) -> Result<(), WasmHostError> {
    let mut root = linker.root();
    for (import_name, import_item) in component.component_type().imports(engine) {
        stub_import_item(&mut root, import_name, &import_item, "", engine)?;
    }
    Ok(())
}

/// Recursively stub a single import item into `linker_instance`. `interface`
/// is the dotted path of the *parent* instance (empty at the root) so a
/// function stub can carry the precise `(interface, item)` pair in its
/// [`DeferredCapability`] payload.
fn stub_import_item(
    linker_instance: &mut wasmtime::component::LinkerInstance<'_, StoreState>,
    item_name: &str,
    item: &ComponentItem,
    interface: &str,
    engine: &wasmtime::Engine,
) -> Result<(), WasmHostError> {
    let link_err = |e: wasmtime::Error| WasmHostError::Link(e.to_string());
    match item {
        // A function — the only guest-callable item. Stub it with the typed
        // capability-denied payload so `classify_trap` can label it precisely.
        ComponentItem::ComponentFunc(_) => {
            let denial = DeferredCapability {
                interface: interface.to_string(),
                item: item_name.to_string(),
            };
            linker_instance
                .func_new(item_name, move |_store, _ty, _params, _results| {
                    Err(wasmtime::Error::new(denial.clone()))
                })
                .map_err(link_err)?;
        }
        // A nested WIT interface — recurse so its functions/resources are
        // stubbed too. `Linker::instance` inserts the namespace fresh, so this
        // is the *only* pass that touches it.
        ComponentItem::ComponentInstance(instance) => {
            let nested_interface = if interface.is_empty() {
                item_name.to_string()
            } else {
                format!("{interface}/{item_name}")
            };
            let mut nested = linker_instance.instance(item_name).map_err(link_err)?;
            for (export_name, export_item) in instance.exports(engine) {
                stub_import_item(
                    &mut nested,
                    export_name,
                    &export_item,
                    &nested_interface,
                    engine,
                )?;
            }
        }
        // A resource type — not callable, but the instantiation type-check
        // requires a matching host resource definition (e.g. `wasi:io/poll`'s
        // `pollable`). The destructor is a no-op: the guest never obtains a
        // live handle because every function that would mint one is stubbed.
        ComponentItem::Resource(_) => {
            linker_instance
                .resource(
                    item_name,
                    wasmtime::component::ResourceType::host::<()>(),
                    |_store, _rep| Ok(()),
                )
                .map_err(link_err)?;
        }
        // Core modules and sub-components cannot be stubbed; cadenza's world
        // never imports them, so reaching here means a malformed component.
        ComponentItem::Module(_) | ComponentItem::Component(_) => {
            return Err(WasmHostError::Link(format!(
                "cannot stub import `{item_name}`: core module / sub-component imports are unsupported"
            )));
        }
        // Core functions and bare interface types are not guest call sites and
        // need no linker definition.
        ComponentItem::CoreFunc(_) | ComponentItem::Type(_) => {}
    }
    Ok(())
}

/// Wire the in-scope host interfaces into `linker` (`host-log`, `host-time`,
/// `host-workspace`, `host-secrets`, `host-linear`). The guest's incidental
/// WASI imports are stubbed as traps in [`ComponentRuntime::run_tool`] rather
/// than granted.
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
    cadenza::runtime::host_linear::add_to_linker::<_, HasSelf<StoreState>>(linker, |s| s)
        .map_err(link_err)?;
    Ok(())
}

impl ComponentRuntime {
    /// Instantiate `loaded` against a fresh linker carrying the linked host
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
        // Re-arm the epoch deadline at execution time. The deadline set in
        // `new_store` is relative to the engine epoch *then*, but the runtime's
        // ticker advances the epoch continuously — so any gap between
        // `new_store` and this call (or reuse of the store for another call)
        // could leave the budget already spent and trap immediately. Re-arming
        // here makes the budget measure this invocation's execution, not the
        // store's age (issue #62 review follow-up).
        store.set_epoch_deadline(self.epoch_budget_ticks());
        let mut linker = Linker::<StoreState>::new(self.engine());
        // Stub *every* import first, then shadow the in-scope host interfaces
        // with their real implementations. The guest's incidental WASI
        // imports (from the Rust std runtime) therefore grant nothing — no
        // preopens, env, clocks, random, sockets, or filesystem reach the
        // guest, and a deferred host interface (`host-http` / `host-tools`)
        // is not callable either. The only live capabilities are the linked
        // host functions, satisfying the issue's "minimal capability"
        // requirement.
        //
        // `define_imports_as_capability_denied` does the stubbing: it walks
        // the component's full import tree and gives each *function* import a
        // typed `DeferredCapability` closure, so a guest call into a deferred
        // capability surfaces in `classify_trap` as
        // `WasmHostError::CapabilityDenied` rather than the less-precise
        // `LimitBreached` a stringly-typed trap would land on (issue #75 case
        // 2). It deliberately does NOT use wasmtime's own
        // `define_unknown_imports_as_traps` — see that function's doc for why.
        linker.allow_shadowing(true);
        define_imports_as_capability_denied(&mut linker, &loaded.component, self.engine())?;
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

impl cadenza::runtime::host_linear::Host for StoreState {
    fn linear_graphql(
        &mut self,
        operation_name: Option<String>,
        query: String,
        variables_json: String,
        mode: GraphqlMode,
    ) -> Result<GraphqlResponse, HostError> {
        let host_mode = match mode {
            GraphqlMode::Read => LinearMode::Read,
            GraphqlMode::Write => LinearMode::Write,
        };
        // Fingerprint up front so even a denied/errored call is audited
        // without ever logging the raw query text.
        let fingerprint = query_fingerprint(&query);
        let started = std::time::Instant::now();
        let outcome = self.dispatch_linear(
            operation_name.as_deref(),
            &query,
            &variables_json,
            host_mode,
        );
        let duration_ms = started.elapsed().as_millis() as u64;
        // The guest receives only the typed variant + a generic message; the
        // detailed (scrubbed, capped) failure text stays host-side in the audit.
        let (guest_result, audit_error) = match outcome {
            Ok(resp) => (Ok(resp), None),
            Err(failure) => (Err(failure.guest), Some(failure.audit_detail)),
        };
        self.record_linear_audit(
            operation_name.as_deref(),
            &fingerprint,
            duration_ms,
            host_mode,
            audit_error.as_deref(),
        );
        guest_result
    }
}

/// Max bytes of failure detail retained in the audit log for a single
/// `linear-graphql` call. The transport may include an arbitrarily large
/// upstream body in its error string; capping before storage keeps a chatty
/// failure from bloating the bounded log sink host-side.
const MAX_AUDIT_ERROR_BYTES: usize = 512;

/// A `linear-graphql` failure split into what the guest may see (`guest`: a
/// typed variant with a *generic* message, never carrying upstream text or a
/// token) and what is retained host-side for the audit (`audit_detail`:
/// scrubbed + length-capped). Keeping these separate means the
/// "token never reaches guest memory" guarantee does not depend on the caller
/// having seeded the scrubber with the transport credential.
struct LinearFailure {
    guest: HostError,
    audit_detail: String,
}

impl LinearFailure {
    fn denied(message: &str) -> Self {
        Self {
            guest: HostError::Denied(message.to_string()),
            audit_detail: format!("denied: {message}"),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            audit_detail: format!("invalid-argument: {message}"),
            guest: HostError::InvalidArgument(message),
        }
    }
}

impl StoreState {
    /// Validate the call against host policy and hand it to the configured
    /// transport. The error half is a [`LinearFailure`] so the caller can route
    /// a generic message to the guest while auditing the detail host-side.
    fn dispatch_linear(
        &self,
        operation_name: Option<&str>,
        query: &str,
        variables_json: &str,
        mode: LinearMode,
    ) -> Result<GraphqlResponse, LinearFailure> {
        let cap = self
            .caps
            .linear
            .as_ref()
            .ok_or_else(|| LinearFailure::denied("linear capability not configured"))?;
        // Endpoint allowlist: the endpoint is host-configured (the guest never
        // supplies a URL); a misconfiguration fails closed.
        if !cap.endpoint_allowed() {
            return Err(LinearFailure::denied(
                "linear endpoint is not on the allowlist",
            ));
        }
        if query.trim().is_empty() {
            return Err(LinearFailure::invalid("query must not be empty"));
        }
        // Normalise/validate variables: empty means `{}`; anything present
        // must parse as a JSON *object* — GraphQL request variables are a
        // name→value map, so a scalar/array is rejected before the request.
        let trimmed = variables_json.trim();
        let normalised_vars = if trimmed.is_empty() {
            "{}".to_string()
        } else {
            let parsed: serde_json::Value = serde_json::from_str(trimmed).map_err(|e| {
                LinearFailure::invalid(format!("variables-json is not valid JSON: {e}"))
            })?;
            if !parsed.is_object() {
                return Err(LinearFailure::invalid(
                    "variables-json must be a JSON object",
                ));
            }
            trimmed.to_string()
        };

        let call = LinearCall {
            operation_name: operation_name.map(str::to_string),
            query: query.to_string(),
            variables_json: normalised_vars,
            mode,
            endpoint: cap.endpoint().to_string(),
            // Hand the limit to the transport so it can bound its own read and
            // never materialise an oversized body host-side.
            max_response_bytes: self.http_body_limit,
        };
        match cap.transport().execute(call) {
            Ok(res) => {
                // Backstop: even if the transport over-read, refuse to hand a
                // body larger than `max_http_body_bytes` across to guest
                // memory. Bounding the host-side allocation itself is the
                // transport's responsibility (see `LinearCall::max_response_bytes`).
                if res.body_json.len() > self.http_body_limit {
                    return Err(LinearFailure {
                        guest: HostError::Upstream("linear response body too large".to_string()),
                        audit_detail: format!(
                            "response body exceeds limit of {} bytes",
                            self.http_body_limit
                        ),
                    });
                }
                Ok(GraphqlResponse {
                    status: res.status,
                    body_json: res.body_json,
                })
            }
            Err(err) => Err(self.map_transport_failure(err)),
        }
    }

    /// Map a transport failure into a [`LinearFailure`]. The guest-facing error
    /// carries a fixed, generic message — the upstream/IO text is **never**
    /// forwarded to the guest, so an upstream that echoes the Authorization
    /// token cannot leak it regardless of whether the scrubber was seeded. The
    /// detail (scrubbed + capped) is kept only for the host-side audit.
    fn map_transport_failure(&self, err: LinearTransportError) -> LinearFailure {
        let detail = |kind: &str, msg: String| {
            let scrubbed = self.caps.scrubber.scrub_text(&msg);
            format!("{kind}: {}", truncate_for_audit(&scrubbed))
        };
        match err {
            LinearTransportError::RateLimited(hint) => LinearFailure {
                guest: HostError::RateLimited(hint),
                audit_detail: match hint {
                    Some(secs) => format!("rate-limited: retry after {secs}s"),
                    None => "rate-limited".to_string(),
                },
            },
            LinearTransportError::Upstream(msg) => LinearFailure {
                guest: HostError::Upstream("linear upstream error".to_string()),
                audit_detail: detail("upstream", msg),
            },
            LinearTransportError::Io(msg) => LinearFailure {
                guest: HostError::Io("linear transport io error".to_string()),
                audit_detail: detail("io", msg),
            },
        }
    }

    /// Record one audit entry for a `linear-graphql` call. The raw query is
    /// never logged — only the fingerprint, operation name (scrubbed),
    /// duration, mode, and (on failure) the scrubbed + capped failure detail.
    fn record_linear_audit(
        &self,
        operation_name: Option<&str>,
        fingerprint: &str,
        duration_ms: u64,
        mode: LinearMode,
        error_detail: Option<&str>,
    ) {
        use cadenza_obs::fields;
        let scrub = |s: &str| self.caps.scrubber.scrub_text(s);
        let mut map = serde_json::Map::new();
        map.insert(
            fields::FIELD_OPERATION_NAME.to_string(),
            match operation_name {
                Some(name) => serde_json::Value::String(scrub(name)),
                None => serde_json::Value::Null,
            },
        );
        map.insert(
            fields::FIELD_QUERY_FINGERPRINT.to_string(),
            serde_json::Value::String(fingerprint.to_string()),
        );
        map.insert(
            fields::FIELD_DURATION_MS.to_string(),
            serde_json::Value::Number(duration_ms.into()),
        );
        map.insert(
            fields::FIELD_GRAPHQL_MODE.to_string(),
            serde_json::Value::String(mode.label().to_string()),
        );
        if let Some(detail) = error_detail {
            // Defensive re-scrub: transport detail is already scrubbed, but
            // locally-built denial/invalid messages pass through here too.
            map.insert(
                fields::FIELD_ERROR.to_string(),
                serde_json::Value::String(scrub(detail)),
            );
        }
        let fields_json = serde_json::Value::Object(map).to_string();
        self.record_call(
            "host-linear.linear-graphql",
            None,
            None,
            Some("linear graphql operation".to_string()),
            Some(fields_json),
        );
    }
}

/// Truncate failure detail to [`MAX_AUDIT_ERROR_BYTES`] on a UTF-8 char
/// boundary, appending an ellipsis marker when clipped, so a transport that
/// stuffs a large upstream body into its error string cannot bloat the audit.
fn truncate_for_audit(text: &str) -> String {
    if text.len() <= MAX_AUDIT_ERROR_BYTES {
        return text.to_string();
    }
    let mut end = MAX_AUDIT_ERROR_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[truncated]", &text[..end])
}

/// Non-cryptographic FNV-1a-64 fingerprint of a GraphQL query, hex-encoded.
/// Used so the audit log can correlate identical operations without recording
/// the raw query text. Not a security primitive.
fn query_fingerprint(query: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    for byte in query.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("fnv1a64:{hash:016x}")
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
/// Instantiation can fail four different ways: (a) the linker was missing
/// wiring (a `Link` problem), (b) guest init code traps with an epoch
/// interruption (`Timeout`), (c) the host limiter denied an allocation via
/// the typed `ResourceLimitBreached` signal (`LimitBreached` — issue #75 case
/// 1), or (d) a deferred-capability stub was invoked during initialization
/// (`CapabilityDenied`). Downcasting the typed payloads first means an over-cap
/// declared memory/table no longer mislabels as `Link`, and a deferred host
/// import called during init no longer mislabels as `LimitBreached`.
///
/// One resource-limit sub-case is deliberately left as `Link`: a *count*-cap
/// breach (too many instances/tables/memories, enforced by wasmtime via
/// `Store::bump_resource_counts`) bails with a plain string and carries no
/// typed payload or `Trap` in wasmtime 45, so there is no non-fragile signal
/// to downcast. Fixing that precisely needs upstream support and is out of
/// scope for #75 (whose example is the size-growth path handled above).
pub(crate) fn classify_instantiate(err: wasmtime::Error) -> WasmHostError {
    if let Some(breach) = err.downcast_ref::<crate::ResourceLimitBreached>() {
        return WasmHostError::LimitBreached(breach.to_string());
    }
    if err.downcast_ref::<crate::DeferredCapability>().is_some() {
        return crate::classify_trap(err);
    }
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

    // End-to-end proof for issue #75 case 2: a guest that calls an import the
    // host did not link must surface as `CapabilityDenied`, not `LimitBreached`.
    // This drives a minimal hand-written component (no plugin build needed)
    // whose exported `go` calls an unlinked root import; `define_imports_as_
    // capability_denied` stubs that import with the typed payload, and a real
    // guest→host call must propagate it so `classify_trap` maps it precisely.
    #[test]
    fn deferred_import_call_surfaces_as_capability_denied() {
        use wasmtime::component::{Component, Linker};

        // A component that imports a single root-level function `run-deferred`
        // and exports `go`, which calls it. `run-deferred` is never linked by
        // a real host capability, so our stub is what answers the call.
        const WAT: &str = r#"
            (component
              (import "run-deferred" (func $deferred))
              (core func $deferred-core (canon lower (func $deferred)))
              (core module $m
                (import "host" "deferred" (func $d))
                (func (export "go") call $d)
              )
              (core instance $i (instantiate $m
                (with "host" (instance (export "deferred" (func $deferred-core))))
              ))
              (func (export "go") (canon lift (core func $i "go")))
            )
        "#;

        let runtime = crate::ComponentRuntime::new(crate::WasmRuntimeLimits::default()).unwrap();
        let component = Component::new(runtime.engine(), WAT).expect("compile wat component");

        let mut linker = Linker::<StoreState>::new(runtime.engine());
        linker.allow_shadowing(true);
        define_imports_as_capability_denied(&mut linker, &component, runtime.engine())
            .expect("stub imports");

        let mut store = runtime.new_store(crate::RequestContext::default());
        let instance = linker
            .instantiate(&mut store, &component)
            .expect("instantiate stubbed component");
        let go = instance
            .get_typed_func::<(), ()>(&mut store, "go")
            .expect("export go");

        let err = go
            .call(&mut store, ())
            .expect_err("calling an unlinked import must fail");
        // The typed payload must survive the guest→host boundary so the host
        // can classify it precisely rather than as a generic limit breach.
        let classified = crate::classify_trap(err);
        assert!(
            matches!(classified, WasmHostError::CapabilityDenied(_)),
            "deferred import call must classify as CapabilityDenied, got {classified:?}",
        );
    }

    // Guards the symlink half of containment specifically: lexical `safe_join`
    // passes a symlink that lives inside the root, so only `resolve_inside`'s
    // canonicalize catches the escape. Removing that call makes this test read
    // the outside target and fail.
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

    // --- host-linear (#17) ---

    use super::cadenza::runtime::host_linear::Host as _;
    use crate::{
        ComponentRuntime, HostCapabilities, HostClock, LinearCall, LinearCapability,
        LinearHttpResult, LinearTransport, LinearTransportError, LogSink, RequestContext,
        WasmRuntimeLimits,
    };
    use std::sync::{Arc, Mutex};

    /// Mock transport that is the *host-side* injector of auth. It records the
    /// calls it received (so a test can assert the guest never influenced
    /// headers) and returns a canned result/error. The `injected_token` models
    /// the credential that lives host-side and must never reach the guest.
    #[derive(Debug)]
    struct MockTransport {
        injected_token: String,
        result: Mutex<Option<Result<LinearHttpResult, LinearTransportError>>>,
        seen: Mutex<Vec<LinearCall>>,
    }

    impl MockTransport {
        fn ok(body: &str) -> Arc<Self> {
            Arc::new(Self {
                injected_token: "lr_live_HOSTONLY".to_string(),
                result: Mutex::new(Some(Ok(LinearHttpResult {
                    status: 200,
                    body_json: body.to_string(),
                }))),
                seen: Mutex::new(Vec::new()),
            })
        }

        fn erroring(err: LinearTransportError) -> Arc<Self> {
            Arc::new(Self {
                injected_token: "lr_live_HOSTONLY".to_string(),
                result: Mutex::new(Some(Err(err))),
                seen: Mutex::new(Vec::new()),
            })
        }

        fn calls(&self) -> Vec<LinearCall> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl LinearTransport for MockTransport {
        fn execute(&self, call: LinearCall) -> Result<LinearHttpResult, LinearTransportError> {
            // The transport — not the guest — is where auth is injected.
            let _auth_header = format!("Authorization: Bearer {}", self.injected_token);
            self.seen.lock().unwrap().push(call);
            self.result
                .lock()
                .unwrap()
                .take()
                .unwrap_or(Err(LinearTransportError::Io("mock exhausted".to_string())))
        }
    }

    fn linear_store(
        cap: Option<LinearCapability>,
        scrubber: Scrubber,
    ) -> wasmtime::Store<StoreState> {
        linear_store_with(WasmRuntimeLimits::default(), cap, scrubber)
    }

    fn linear_store_with(
        limits: WasmRuntimeLimits,
        cap: Option<LinearCapability>,
        scrubber: Scrubber,
    ) -> wasmtime::Store<StoreState> {
        let rt = ComponentRuntime::new(limits).unwrap();
        let log_sink = LogSink::new();
        rt.new_store_with(
            RequestContext {
                issue_id: Some("CAD-17".to_string()),
                plugin_name: Some("linear-example".to_string()),
                ..Default::default()
            },
            HostCapabilities {
                scrubber,
                clock: HostClock::Fixed(1),
                log_sink,
                linear: cap,
                ..Default::default()
            },
        )
    }

    fn last_audit(store: &wasmtime::Store<StoreState>) -> HostLogRecord {
        store
            .data()
            .log_sink()
            .records()
            .into_iter()
            .find(|r| r.op == "host-linear.linear-graphql")
            .expect("a host-linear audit record")
    }

    #[test]
    fn linear_unconfigured_capability_is_denied() {
        let mut store = linear_store(None, Scrubber::empty());
        let err = store
            .data_mut()
            .linear_graphql(
                Some("Q".to_string()),
                "query { viewer { id } }".to_string(),
                String::new(),
                GraphqlMode::Read,
            )
            .expect_err("missing capability must deny");
        assert!(matches!(err, HostError::Denied(_)), "got {err:?}");
        // Even a denied call is audited.
        let audit = last_audit(&store);
        let fields: serde_json::Value =
            serde_json::from_str(audit.fields_json.as_deref().unwrap()).unwrap();
        assert!(fields[cadenza_obs::fields::FIELD_QUERY_FINGERPRINT].is_string());
    }

    #[test]
    fn linear_endpoint_off_allowlist_is_denied() {
        let transport = MockTransport::ok("{}");
        // Endpoint not present in the allowlist set.
        let cap = LinearCapability::new(
            "https://evil.example/graphql",
            ["https://api.linear.app/graphql".to_string()],
            transport.clone(),
        );
        let mut store = linear_store(Some(cap), Scrubber::empty());
        let err = store
            .data_mut()
            .linear_graphql(
                None,
                "query { viewer { id } }".to_string(),
                String::new(),
                GraphqlMode::Read,
            )
            .expect_err("off-allowlist endpoint must deny");
        assert!(matches!(err, HostError::Denied(_)), "got {err:?}");
        // The transport must never have been reached.
        assert!(
            transport.calls().is_empty(),
            "denied call hit the transport"
        );
    }

    #[test]
    fn linear_empty_query_is_invalid_argument() {
        let transport = MockTransport::ok("{}");
        let cap = LinearCapability::with_default_allowlist(transport.clone());
        let mut store = linear_store(Some(cap), Scrubber::empty());
        let err = store
            .data_mut()
            .linear_graphql(None, "   ".to_string(), String::new(), GraphqlMode::Read)
            .expect_err("empty query must be rejected");
        assert!(matches!(err, HostError::InvalidArgument(_)), "got {err:?}");
        // Validation happens before any request — a malformed call must never
        // reach the upstream.
        assert!(
            transport.calls().is_empty(),
            "rejected call hit the transport"
        );
    }

    #[test]
    fn linear_invalid_variables_json_is_invalid_argument() {
        let transport = MockTransport::ok("{}");
        let cap = LinearCapability::with_default_allowlist(transport.clone());
        let mut store = linear_store(Some(cap), Scrubber::empty());
        let err = store
            .data_mut()
            .linear_graphql(
                None,
                "query { viewer { id } }".to_string(),
                "{not json".to_string(),
                GraphqlMode::Read,
            )
            .expect_err("malformed variables must be rejected");
        assert!(matches!(err, HostError::InvalidArgument(_)), "got {err:?}");
        assert!(
            transport.calls().is_empty(),
            "rejected call hit the transport"
        );
    }

    #[test]
    fn linear_non_object_variables_are_rejected() {
        // Valid JSON but not an object — GraphQL variables must be a map.
        let transport = MockTransport::ok("{}");
        let cap = LinearCapability::with_default_allowlist(transport.clone());
        let mut store = linear_store(Some(cap), Scrubber::empty());
        let err = store
            .data_mut()
            .linear_graphql(
                None,
                "query { viewer { id } }".to_string(),
                "[1,2,3]".to_string(),
                GraphqlMode::Read,
            )
            .expect_err("array variables must be rejected");
        assert!(matches!(err, HostError::InvalidArgument(_)), "got {err:?}");
        assert!(
            transport.calls().is_empty(),
            "rejected call hit the transport"
        );
    }

    #[test]
    fn linear_oversized_response_body_is_rejected() {
        // A response body larger than the runtime's max_http_body_bytes must
        // not cross into guest memory; it fails with a typed error instead.
        let limits = WasmRuntimeLimits {
            max_http_body_bytes: 16,
            ..Default::default()
        };
        let big = "x".repeat(64);
        let transport = MockTransport::ok(&big);
        let cap = LinearCapability::with_default_allowlist(transport);
        let mut store = linear_store_with(limits, Some(cap), Scrubber::empty());
        let err = store
            .data_mut()
            .linear_graphql(
                None,
                "query { viewer { id } }".to_string(),
                String::new(),
                GraphqlMode::Read,
            )
            .expect_err("oversized body must be rejected");
        assert!(
            matches!(err, HostError::Upstream(ref m) if m.contains("too large")),
            "got {err:?}",
        );
    }

    #[test]
    fn linear_allowed_operation_returns_transport_body_and_audits() {
        let transport = MockTransport::ok(r#"{"data":{"viewer":{"id":"u_1"}}}"#);
        let cap = LinearCapability::with_default_allowlist(transport.clone());
        let mut store = linear_store(Some(cap), Scrubber::empty());

        let resp = store
            .data_mut()
            .linear_graphql(
                Some("Viewer".to_string()),
                "query Viewer { viewer { id } }".to_string(),
                r#"{"first":1}"#.to_string(),
                GraphqlMode::Read,
            )
            .expect("allowed operation succeeds");
        assert_eq!(resp.status, 200);
        assert!(resp.body_json.contains("u_1"));

        // The transport saw exactly the host-normalised call; the guest never
        // supplied a header (the WIT has no header channel).
        let calls = transport.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].operation_name.as_deref(), Some("Viewer"));
        assert_eq!(calls[0].endpoint, LinearCapability::DEFAULT_ENDPOINT);
        assert_eq!(calls[0].variables_json, r#"{"first":1}"#);
        // The runtime's response-size limit is handed to the transport so it
        // can bound its own read.
        assert_eq!(
            calls[0].max_response_bytes,
            WasmRuntimeLimits::default().max_http_body_bytes
        );

        // Audit carries operation name, fingerprint, duration, and mode; no
        // error on the success path.
        let audit = last_audit(&store);
        let fields: serde_json::Value =
            serde_json::from_str(audit.fields_json.as_deref().unwrap()).unwrap();
        assert_eq!(fields[cadenza_obs::fields::FIELD_OPERATION_NAME], "Viewer");
        assert!(
            fields[cadenza_obs::fields::FIELD_QUERY_FINGERPRINT]
                .as_str()
                .unwrap()
                .starts_with("fnv1a64:")
        );
        assert!(fields[cadenza_obs::fields::FIELD_DURATION_MS].is_number());
        assert_eq!(fields[cadenza_obs::fields::FIELD_GRAPHQL_MODE], "read");
        assert!(fields.get(cadenza_obs::fields::FIELD_ERROR).is_none());
    }

    #[test]
    fn linear_empty_variables_normalise_to_empty_object() {
        let transport = MockTransport::ok("{}");
        let cap = LinearCapability::with_default_allowlist(transport.clone());
        let mut store = linear_store(Some(cap), Scrubber::empty());
        store
            .data_mut()
            .linear_graphql(
                None,
                "query { viewer { id } }".to_string(),
                String::new(),
                GraphqlMode::Write,
            )
            .expect("succeeds");
        assert_eq!(transport.calls()[0].variables_json, "{}");
        // Write mode is logged distinctly from read.
        let audit = last_audit(&store);
        let fields: serde_json::Value =
            serde_json::from_str(audit.fields_json.as_deref().unwrap()).unwrap();
        assert_eq!(fields[cadenza_obs::fields::FIELD_GRAPHQL_MODE], "write");
    }

    #[test]
    fn linear_upstream_error_never_forwards_text_to_guest() {
        // The upstream echoes the host-only token in its error body AND the
        // scrubber is EMPTY (caller forgot to seed it). The guest must still
        // never see the token: the guest-facing error is a generic typed
        // variant, so safety does not depend on scrubber seeding.
        let token = "lr_live_HOSTONLY";
        let transport = MockTransport::erroring(LinearTransportError::Upstream(format!(
            "unauthorized: token {token} rejected"
        )));
        let cap = LinearCapability::with_default_allowlist(transport);
        let mut store = linear_store(Some(cap), Scrubber::empty());

        let err = store
            .data_mut()
            .linear_graphql(
                None,
                "query { viewer { id } }".to_string(),
                String::new(),
                GraphqlMode::Read,
            )
            .expect_err("upstream failure must surface as host-error");
        let HostError::Upstream(msg) = &err else {
            panic!("expected upstream, got {err:?}");
        };
        assert!(!msg.contains(token), "token leaked to guest: {msg}");
        // The guest message is generic — it carries no upstream text at all.
        assert!(
            !msg.contains("unauthorized"),
            "upstream text leaked to guest: {msg}",
        );
    }

    #[test]
    fn linear_upstream_error_detail_is_scrubbed_and_capped_in_audit() {
        // A seeded scrubber redacts the token from the host-side audit detail,
        // and an oversized upstream error string is capped so it cannot bloat
        // the bounded log sink.
        let token = "lr_live_HOSTONLY";
        let big = format!("{token} {}", "A".repeat(4096));
        let transport = MockTransport::erroring(LinearTransportError::Upstream(big));
        let cap = LinearCapability::with_default_allowlist(transport);
        let scrubber = Scrubber::with_secrets(vec![token.to_string()]);
        let mut store = linear_store(Some(cap), scrubber);

        store
            .data_mut()
            .linear_graphql(
                None,
                "query { viewer { id } }".to_string(),
                String::new(),
                GraphqlMode::Read,
            )
            .expect_err("upstream failure must surface");

        let audit = last_audit(&store);
        let fields = audit.fields_json.expect("audit fields");
        assert!(!fields.contains(token), "token leaked into audit: {fields}");
        // The detail is capped well below the 4 KiB upstream string.
        let parsed: serde_json::Value = serde_json::from_str(&fields).unwrap();
        let detail = parsed[cadenza_obs::fields::FIELD_ERROR].as_str().unwrap();
        assert!(
            detail.len() < 700,
            "audit error detail not capped ({} bytes): {detail}",
            detail.len(),
        );
        assert!(detail.contains("truncated"), "expected truncation marker");
    }

    #[test]
    fn linear_rate_limit_carries_retry_hint() {
        let transport = MockTransport::erroring(LinearTransportError::RateLimited(Some(30)));
        let cap = LinearCapability::with_default_allowlist(transport);
        let mut store = linear_store(Some(cap), Scrubber::empty());
        let err = store
            .data_mut()
            .linear_graphql(
                None,
                "query { viewer { id } }".to_string(),
                String::new(),
                GraphqlMode::Read,
            )
            .expect_err("rate limit must surface");
        assert!(
            matches!(err, HostError::RateLimited(Some(30))),
            "got {err:?}"
        );
    }

    #[test]
    fn query_fingerprint_is_deterministic_and_distinguishes_queries() {
        let a = query_fingerprint("query { viewer { id } }");
        let b = query_fingerprint("query { viewer { id } }");
        let c = query_fingerprint("mutation { x }");
        assert_eq!(a, b, "fingerprint must be deterministic");
        assert_ne!(a, c, "different queries must differ");
        assert!(a.starts_with("fnv1a64:"));
    }

    #[test]
    fn linear_audit_field_constants_match() {
        // Pin the audit field names so a rename of the cadenza-obs contract is
        // caught here rather than silently drifting.
        assert_eq!(cadenza_obs::fields::FIELD_OPERATION_NAME, "operation_name");
        assert_eq!(
            cadenza_obs::fields::FIELD_QUERY_FINGERPRINT,
            "query_fingerprint"
        );
        assert_eq!(cadenza_obs::fields::FIELD_DURATION_MS, "duration_ms");
        assert_eq!(cadenza_obs::fields::FIELD_GRAPHQL_MODE, "graphql_mode");
        assert_eq!(cadenza_obs::fields::FIELD_ERROR, "error");
    }
}
