//! Wasmtime component loader and resource-limit boundary for Cadenza.
//!
//! The host configures Wasmtime with explicit memory/table/instance
//! caps and an epoch-based timeout; `ComponentRuntime` owns a background
//! [`EpochTicker`] that advances the engine epoch so that timeout actually
//! fires (issue #62). Components are loaded from disk
//! via `ComponentRuntime::load`; the WIT package/world declared by
//! the caller must match cadenza's frozen baseline (`WIT_PACKAGE` /
//! `WIT_WORLD`) otherwise the loader fails closed.
//!
//! Host capability functions (`host-log`, `host-time`, `host-workspace`,
//! `host-secrets`, `host-linear`) are implemented in [`capabilities`] and
//! linked into the Wasmtime `Linker` by [`ComponentRuntime::run_tool`]. The
//! store carries a [`RequestContext`] (issue/plugin identity + workspace
//! root), a [`HostCapabilities`] bundle (configured secret names, redaction
//! scrubber, clock, captured log sink, Linear capability) and the
//! `RuntimeLimiter`. The guest's incidental WASI imports are stubbed as traps
//! (not granted) during linking, so the only live capabilities are those
//! linked host functions — see ADR 0005 and ADR 0006.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cadenza_obs::Scrubber;
use serde::{Deserialize, Serialize};
use wasmtime::component::Component;
use wasmtime::{Config, Engine, ResourceLimiter, Store};

mod capabilities;

pub use capabilities::{HostError, LogLevel, ToolInput, ToolOutput, WorkspaceReadResult};

/// Frozen WIT identity of the cadenza host. Plugins must declare the
/// same package and world in their `WasmComponentRef`. The ABI gate
/// (#5) enforces that the actual binary surface matches what the
/// snapshot pins.
pub const WIT_PACKAGE: &str = "cadenza:runtime@0.2.0";
pub const WIT_WORLD: &str = "tool-runtime";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmRuntimeLimits {
    pub max_memory_bytes: usize,
    pub max_tables: usize,
    pub max_instances: usize,
    pub epoch_timeout_ms: u64,
    pub max_http_body_bytes: usize,
}

impl Default for WasmRuntimeLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024,
            max_tables: 64,
            max_instances: 16,
            epoch_timeout_ms: 5_000,
            max_http_body_bytes: 2 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmComponentRef {
    pub name: String,
    pub path: PathBuf,
    pub wit_package: String,
    pub wit_world: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WasmHostError {
    #[error("component WIT package mismatch: expected {expected}, actual {actual}")]
    WitPackageMismatch { expected: String, actual: String },
    #[error("component WIT world mismatch: expected {expected}, actual {actual}")]
    WitWorldMismatch { expected: String, actual: String },
    #[error("component denied by capability policy: {0}")]
    CapabilityDenied(String),
    #[error("failed to wire host capabilities into the linker: {0}")]
    Link(String),
    #[error("component file not found: {path}")]
    NotFound { path: PathBuf },
    #[error("component compile error: {0}")]
    Compile(String),
    #[error("guest exceeded resource limit: {0}")]
    LimitBreached(String),
    #[error("guest hit the wall-clock timeout (epoch interruption)")]
    Timeout,
    #[error("wasmtime engine init failed: {0}")]
    Engine(String),
    #[error("io error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Per-instance store payload read by the host capability functions in
/// [`capabilities`]. Holds the resource limiter, the per-request identity and
/// workspace root ([`RequestContext`]), and the host-side capability config
/// ([`HostCapabilities`]). Workspace access is exclusively via
/// `host-workspace`; the guest's incidental WASI imports are trapped, not
/// granted (see [`ComponentRuntime::run_tool`]).
pub struct StoreState {
    pub limiter: RuntimeLimiter,
    pub request: RequestContext,
    caps: HostCapabilities,
    /// Max bytes a host capability may hand back to the guest in a single
    /// response body (the runtime's `max_http_body_bytes`). Enforced by
    /// `host-linear` on the GraphQL response so an oversized upstream body
    /// cannot force a large host allocation or breach guest memory.
    http_body_limit: usize,
}

impl StoreState {
    /// Read-only view of the configured host capabilities.
    pub fn caps(&self) -> &HostCapabilities {
        &self.caps
    }

    /// The captured structured-log sink. Each host call appends a
    /// [`HostLogRecord`] carrying issue/plugin context; the `log` capability
    /// additionally records the (redacted) level/message/fields.
    pub fn log_sink(&self) -> &LogSink {
        &self.caps.log_sink
    }
}

/// Caller-supplied identity the host functions stamp onto every log record.
/// No raw secret material is allowed here — credentials live in the host and
/// `host-secrets` discloses only presence (see SECURITY.md). `workspace_path`
/// is the containment root for `host-workspace.workspace-read`.
#[derive(Debug, Default, Clone)]
pub struct RequestContext {
    pub issue_id: Option<String>,
    pub plugin_name: Option<String>,
    pub workspace_path: Option<PathBuf>,
}

/// Host-side capability configuration for a single guest invocation. None of
/// these expose secret *values* to the guest: `secret_names` is presence-only
/// metadata, and `scrubber` redacts secret-shaped material out of logs.
#[derive(Debug, Default, Clone)]
pub struct HostCapabilities {
    /// Names of secrets the host considers present. `secret-exists` answers
    /// from this set; the value is never stored here or disclosed.
    pub secret_names: BTreeSet<String>,
    /// Redaction applied to `host-log` messages and fields.
    pub scrubber: Scrubber,
    /// Clock backing `now-millis`; injectable so tests are deterministic.
    pub clock: HostClock,
    /// Structured-log capture for host calls.
    pub log_sink: LogSink,
    /// Host-mediated Linear GraphQL capability (#17, ADR 0006). `None` fails
    /// `host-linear.linear-graphql` closed with `host-error::denied`. The raw
    /// Linear token lives inside the transport, never here and never in guest
    /// memory.
    pub linear: Option<LinearCapability>,
}

/// Direction of a Linear GraphQL operation, mirroring the WIT `graphql-mode`
/// enum without leaking the generated bindgen type into the host API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinearMode {
    Read,
    Write,
}

impl LinearMode {
    /// Canonical lower-case label used in the audit log (`graphql_mode`).
    pub fn label(self) -> &'static str {
        match self {
            LinearMode::Read => "read",
            LinearMode::Write => "write",
        }
    }
}

/// A single host-validated Linear GraphQL call handed to the transport. The
/// transport is the *sole* injector of the `Authorization` header; nothing
/// here is guest-supplied auth (the WIT gives the guest no header channel).
#[derive(Debug, Clone)]
pub struct LinearCall {
    pub operation_name: Option<String>,
    pub query: String,
    /// Always valid JSON (the capability validates and normalises empty input
    /// to `{}` before constructing the call).
    pub variables_json: String,
    pub mode: LinearMode,
    /// The host-configured endpoint, already checked against the allowlist.
    pub endpoint: String,
    /// Max bytes the transport should read for the response body. A correct
    /// transport MUST bound its read to this (e.g. a capped/streaming read
    /// that aborts once exceeded) so an oversized upstream response cannot
    /// exhaust host memory before it is even returned. The capability also
    /// re-checks the returned body length as a backstop for the guest-memory
    /// boundary, but only the transport can bound its own allocation.
    pub max_response_bytes: usize,
}

/// Raw transport result for a Linear GraphQL call. A completed HTTP exchange
/// — including a 200 carrying a GraphQL `errors` array — is an `Ok` here;
/// only HTTP/transport-level failures are [`LinearTransportError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearHttpResult {
    pub status: u16,
    pub body_json: String,
}

/// Transport-level failure for a Linear GraphQL call. Messages are scrubbed by
/// the capability layer before they cross into guest memory as a `host-error`.
#[derive(Debug, thiserror::Error)]
pub enum LinearTransportError {
    #[error("rate limited")]
    RateLimited(Option<u32>),
    #[error("upstream error: {0}")]
    Upstream(String),
    #[error("io error: {0}")]
    Io(String),
}

/// Host-side Linear GraphQL transport. Implementations inject the operator's
/// credentials and perform the HTTP request; the credential is never exposed
/// to the guest. Injectable so tests drive a mock without a live server
/// (mirrors [`HostClock`] and `cadenza_tracker_linear::LinearTransport`).
pub trait LinearTransport: Send + Sync + std::fmt::Debug {
    fn execute(&self, call: LinearCall) -> Result<LinearHttpResult, LinearTransportError>;
}

/// Host-mediated Linear GraphQL capability: a host-configured endpoint, an
/// endpoint allowlist, and the transport that injects auth. The raw token
/// lives behind `transport`; this struct holds no credential.
#[derive(Debug, Clone)]
pub struct LinearCapability {
    endpoint: String,
    allowed_endpoints: BTreeSet<String>,
    transport: Arc<dyn LinearTransport>,
}

impl LinearCapability {
    /// The Linear production GraphQL endpoint — the default allowlist entry.
    pub const DEFAULT_ENDPOINT: &'static str = "https://api.linear.app/graphql";

    /// Build a capability with an explicit endpoint and allowlist.
    pub fn new(
        endpoint: impl Into<String>,
        allowed_endpoints: impl IntoIterator<Item = String>,
        transport: Arc<dyn LinearTransport>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            allowed_endpoints: allowed_endpoints.into_iter().collect(),
            transport,
        }
    }

    /// Build a capability targeting [`Self::DEFAULT_ENDPOINT`] with an
    /// allowlist that contains only that endpoint.
    pub fn with_default_allowlist(transport: Arc<dyn LinearTransport>) -> Self {
        Self::new(
            Self::DEFAULT_ENDPOINT,
            [Self::DEFAULT_ENDPOINT.to_string()],
            transport,
        )
    }

    /// The configured endpoint host-linear will call.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Whether the configured endpoint is a member of the allowlist.
    pub fn endpoint_allowed(&self) -> bool {
        self.allowed_endpoints.contains(&self.endpoint)
    }

    pub(crate) fn transport(&self) -> &Arc<dyn LinearTransport> {
        &self.transport
    }
}

/// Clock source for `host-time.now-millis`. Defaults to the system clock;
/// tests inject a fixed value for determinism.
#[derive(Debug, Clone, Default)]
pub enum HostClock {
    #[default]
    System,
    Fixed(u64),
}

impl HostClock {
    /// Milliseconds since the Unix epoch. A pre-epoch system clock clamps to
    /// 0 rather than panicking.
    pub fn now_millis(&self) -> u64 {
        match self {
            HostClock::System => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            HostClock::Fixed(v) => *v,
        }
    }
}

/// A single captured host-call log entry. Every host call records at least
/// `op` + issue/plugin context; `host-log.log` also records the redacted
/// level/message/fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLogRecord {
    pub op: String,
    pub issue_id: Option<String>,
    pub plugin_name: Option<String>,
    pub level: Option<String>,
    pub message: Option<String>,
    pub fields_json: Option<String>,
}

/// Default cap on captured [`HostLogRecord`]s. A guest can call host imports
/// in a tight loop (e.g. `now-millis`) before its epoch deadline; without a
/// cap the sink would grow unbounded host-side, bypassing guest memory limits
/// and threatening host availability. Once the cap is hit, further records are
/// counted in [`LogSink::dropped`] instead of stored.
pub const DEFAULT_LOG_CAPACITY: usize = 4096;

/// Cloneable, shareable, **bounded** capture of [`HostLogRecord`]s. Cheap to
/// clone (the buffer is behind an `Arc<Mutex<…>>`), so a caller can hold a
/// handle and inspect what the guest logged after `run_tool` returns. Bounded
/// to `capacity` records to keep a chatty guest from exhausting host memory.
#[derive(Debug, Clone)]
pub struct LogSink {
    inner: Arc<Mutex<LogSinkInner>>,
}

#[derive(Debug)]
struct LogSinkInner {
    records: Vec<HostLogRecord>,
    dropped: u64,
    capacity: usize,
}

impl Default for LogSink {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_LOG_CAPACITY)
    }
}

impl LogSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sink that retains at most `capacity` records; further pushes are
    /// counted in [`LogSink::dropped`].
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LogSinkInner {
                records: Vec::new(),
                dropped: 0,
                capacity,
            })),
        }
    }

    /// Snapshot of the records captured so far (up to `capacity`).
    pub fn records(&self) -> Vec<HostLogRecord> {
        self.inner
            .lock()
            .expect("log sink mutex poisoned")
            .records
            .clone()
    }

    /// Count of records dropped after the capacity was reached.
    pub fn dropped(&self) -> u64 {
        self.inner.lock().expect("log sink mutex poisoned").dropped
    }

    pub(crate) fn push(&self, record: HostLogRecord) {
        let mut inner = self.inner.lock().expect("log sink mutex poisoned");
        if inner.records.len() >= inner.capacity {
            inner.dropped = inner.dropped.saturating_add(1);
            return;
        }
        inner.records.push(record);
    }
}

/// Resource limiter Wasmtime consults during memory/table growth AND
/// at instance/memory/table allocation time. Tracks configured caps
/// plus a counter of denied growth attempts so tests can assert
/// breach behaviour.
#[derive(Debug, Clone)]
pub struct RuntimeLimiter {
    max_memory_bytes: usize,
    max_tables: usize,
    max_instances: usize,
    denied_growth: usize,
}

impl RuntimeLimiter {
    pub fn new(limits: &WasmRuntimeLimits) -> Self {
        Self {
            max_memory_bytes: limits.max_memory_bytes,
            max_tables: limits.max_tables,
            max_instances: limits.max_instances,
            denied_growth: 0,
        }
    }

    pub fn denied_growth(&self) -> usize {
        self.denied_growth
    }

    pub fn max_memory_bytes(&self) -> usize {
        self.max_memory_bytes
    }

    pub fn max_tables(&self) -> usize {
        self.max_tables
    }

    pub fn max_instances(&self) -> usize {
        self.max_instances
    }
}

impl ResourceLimiter for RuntimeLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.max_memory_bytes {
            self.denied_growth = self.denied_growth.saturating_add(1);
            return Ok(false);
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.max_tables {
            self.denied_growth = self.denied_growth.saturating_add(1);
            return Ok(false);
        }
        Ok(true)
    }

    /// Cap on the number of component instances per store. Wasmtime's
    /// default is 10_000; without this override the configured
    /// `max_instances` is silently ignored once we switched away from
    /// `StoreLimits` (PR #52 codex P1).
    fn instances(&self) -> usize {
        self.max_instances
    }

    /// Cap on the number of tables a component can allocate. Same story
    /// as `instances()` — wasmtime defaults to 10_000 here too, so
    /// without this override `max_tables` only constrains the growth of
    /// each individual table, not how many tables exist (PR #52 codex P2).
    fn tables(&self) -> usize {
        self.max_tables
    }
}

/// Target interval between engine-epoch increments. Wasmtime epoch deadlines
/// count engine-epoch increments, not wall-clock time, so a deadline of N ticks
/// only approximates N milliseconds when ticks land ~1ms apart.
///
/// Cadence-vs-resolution tradeoff: a 1ms `thread::sleep` actually wakes every
/// ~1-15ms on Linux/macOS, so each tick may represent more than 1ms of
/// wall-clock. `epoch_timeout_ms` is therefore an approximate *floor* — a guest
/// is guaranteed to trap, but the real elapsed time may exceed the configured
/// budget by the scheduler's slack. That is acceptable: the invariant we need
/// is "a runaway guest terminates", not "it terminates at exactly N ms". A
/// higher-resolution timer is deliberately out of scope (issue #62).
const EPOCH_TICK_INTERVAL: Duration = Duration::from_millis(1);

/// Background thread that advances a single [`Engine`]'s epoch counter on a
/// fixed cadence so [`Store::set_epoch_deadline`] actually fires. Owned by
/// [`ComponentRuntime`]; [`Drop`] signals the thread to stop and joins it, so
/// the invariant "while a `ComponentRuntime` is alive, its engine's epoch is
/// being advanced" holds for the runtime's whole lifetime (issue #62).
///
/// A guest can only execute via [`ComponentRuntime::run_tool`], which borrows
/// `&self`, so the runtime — and therefore this ticker — is necessarily alive
/// for the duration of any guest call. A store's epoch deadline is thus always
/// backed by a live ticker while the guest is running.
struct EpochTicker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl EpochTicker {
    /// Spawn a ticker advancing `engine`'s epoch every `interval`. The thread
    /// owns its own `Engine` clone (engines are `Arc`-backed, so this is the
    /// *same* epoch counter the runtime's stores observe).
    fn spawn(engine: Engine, interval: Duration) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_signal = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("cadenza-epoch-ticker".to_string())
            .spawn(move || {
                while !stop_signal.load(Ordering::Relaxed) {
                    std::thread::sleep(interval);
                    engine.increment_epoch();
                }
            })
            .expect("spawn cadenza epoch ticker thread");
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // The thread checks `stop` after at most one `interval` sleep, so
            // this join blocks for at most that long.
            let _ = handle.join();
        }
    }
}

/// Wasmtime engine + reusable config. Cheap to clone; one per host
/// process is typical, with per-load `Store` instances on top. Owns the
/// [`EpochTicker`] that advances the engine epoch (issue #62) — dropping the
/// runtime stops the ticker.
pub struct ComponentRuntime {
    engine: Engine,
    limits: WasmRuntimeLimits,
    // Dropped after `engine` by field order; its own `Engine` clone keeps the
    // counter alive regardless. Held only for its `Drop` (stop + join), hence
    // the leading underscore.
    _ticker: EpochTicker,
}

impl ComponentRuntime {
    pub fn new(limits: WasmRuntimeLimits) -> Result<Self, WasmHostError> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.epoch_interruption(true);
        config.consume_fuel(false);
        let engine = Engine::new(&config).map_err(|e| WasmHostError::Engine(e.to_string()))?;
        // Start advancing the epoch immediately so any store's deadline can
        // fire. Without this, `set_epoch_deadline` never trips and a CPU-bound
        // guest runs forever (issue #62).
        let ticker = EpochTicker::spawn(engine.clone(), EPOCH_TICK_INTERVAL);
        Ok(Self {
            engine,
            limits,
            _ticker: ticker,
        })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn limits(&self) -> &WasmRuntimeLimits {
        &self.limits
    }

    /// Validate the caller's declared WIT identity against the frozen
    /// baseline, read the .wasm bytes, and deserialize a Wasmtime
    /// `Component`. The component is returned untyped — host capability
    /// linking and instantiation live in #16. WIT mismatches are
    /// surfaced before any FS read so a misconfigured workflow fails
    /// without touching the disk.
    pub fn load(&self, component: &WasmComponentRef) -> Result<LoadedComponent, WasmHostError> {
        if component.wit_package != WIT_PACKAGE {
            return Err(WasmHostError::WitPackageMismatch {
                expected: WIT_PACKAGE.to_string(),
                actual: component.wit_package.clone(),
            });
        }
        if component.wit_world != WIT_WORLD {
            return Err(WasmHostError::WitWorldMismatch {
                expected: WIT_WORLD.to_string(),
                actual: component.wit_world.clone(),
            });
        }
        if !component.path.is_file() {
            return Err(WasmHostError::NotFound {
                path: component.path.clone(),
            });
        }
        let bytes = std::fs::read(&component.path).map_err(|e| WasmHostError::Io {
            path: component.path.clone(),
            source: e,
        })?;
        let component_handle = Component::new(&self.engine, &bytes)
            .map_err(|e| WasmHostError::Compile(e.to_string()))?;
        Ok(LoadedComponent {
            name: component.name.clone(),
            path: component.path.clone(),
            component: component_handle,
        })
    }

    /// Build a fresh per-issue store with the runtime limiter and an
    /// initial epoch deadline. The runtime's [`EpochTicker`] advances the
    /// engine epoch on a fixed cadence (`Engine::increment_epoch`) — when the
    /// deadline elapses, Wasmtime traps the guest and `WasmHostError::Timeout`
    /// is returned by the caller.
    pub fn new_store(&self, request: RequestContext) -> Store<StoreState> {
        self.new_store_with(request, HostCapabilities::default())
    }

    /// Like [`ComponentRuntime::new_store`] but with explicit host
    /// capabilities (configured secret names, redaction scrubber, clock, log
    /// sink). The guest reaches the filesystem only through the contained
    /// `host-workspace.workspace-read` capability; incidental WASI imports are
    /// trapped during linking (see [`ComponentRuntime::run_tool`]).
    pub fn new_store_with(
        &self,
        request: RequestContext,
        caps: HostCapabilities,
    ) -> Store<StoreState> {
        // `RuntimeLimiter` enforces memory/table growth (size) AND
        // the instance- and table-allocation caps (count) via its
        // `instances()` / `tables()` methods. There is no separate
        // `StoreLimits` because the configured caps must reflect in
        // `denied_growth` for observability — see PR #52 codex P1/P2.
        let state = StoreState {
            limiter: RuntimeLimiter::new(&self.limits),
            request,
            caps,
            http_body_limit: self.limits.max_http_body_bytes,
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limiter);
        // Use the configured epoch budget (#48 P1). The runtime's
        // `EpochTicker` advances the engine epoch via `Engine::increment_epoch`
        // at a fixed cadence (`EPOCH_TICK_INTERVAL`, target 1 tick/ms), so this
        // deadline expressed in *ticks* approximates the budget in ms (#62).
        // A zero or unconfigured budget falls back to 1 so an
        // unsupervised store still traps rather than running forever.
        store.set_epoch_deadline(self.epoch_budget_ticks());
        store
    }

    /// Epoch-deadline value (in ticks) derived from
    /// `WasmRuntimeLimits::epoch_timeout_ms`. Factored out so tests can
    /// assert the configured value without instantiating a wasmtime
    /// store. A zero budget is clamped to 1 so an unsupervised store
    /// still traps rather than running forever.
    pub fn epoch_budget_ticks(&self) -> u64 {
        self.limits.epoch_timeout_ms.max(1)
    }
}

/// Successful `load` result — handle to the deserialized component
/// plus its declared identity. The orchestrator owns this and passes
/// it to a future linker step in #16.
pub struct LoadedComponent {
    pub name: String,
    pub path: PathBuf,
    pub component: Component,
}

impl std::fmt::Debug for LoadedComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedComponent")
            .field("name", &self.name)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// Helper for converting a wasmtime trap into our typed timeout when
/// the trap kind was an epoch interruption. Other trap kinds map to
/// `LimitBreached` so the orchestrator can branch on the cause.
pub fn classify_trap(err: wasmtime::Error) -> WasmHostError {
    use wasmtime::Trap;
    if let Some(trap) = err.downcast_ref::<Trap>() {
        if matches!(*trap, Trap::Interrupt) {
            return WasmHostError::Timeout;
        }
        return WasmHostError::LimitBreached(format!("guest trap: {trap}"));
    }
    WasmHostError::LimitBreached(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_wasm_path() -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut f, b"\0asm\0\0\0\0placeholder").unwrap();
        f
    }

    fn good_ref(path: PathBuf) -> WasmComponentRef {
        WasmComponentRef {
            name: "test-plugin".into(),
            path,
            wit_package: WIT_PACKAGE.into(),
            wit_world: WIT_WORLD.into(),
        }
    }

    #[test]
    fn engine_initialises_with_component_model_and_epoch_interruption() {
        let runtime = ComponentRuntime::new(WasmRuntimeLimits::default()).unwrap();
        // Engine handle is usable for store creation — exercise that path.
        let _store = runtime.new_store(RequestContext::default());
        assert_eq!(runtime.limits().max_memory_bytes, 64 * 1024 * 1024);
    }

    #[test]
    fn wit_package_mismatch_fails_before_io() {
        let runtime = ComponentRuntime::new(WasmRuntimeLimits::default()).unwrap();
        let mut r = good_ref(PathBuf::from("/this/path/should/not/be/touched.wasm"));
        r.wit_package = "evil:other@1.0.0".into();
        let err = runtime.load(&r).unwrap_err();
        assert!(
            matches!(err, WasmHostError::WitPackageMismatch { .. }),
            "got {err:?}",
        );
    }

    #[test]
    fn wit_world_mismatch_fails_before_io() {
        let runtime = ComponentRuntime::new(WasmRuntimeLimits::default()).unwrap();
        let mut r = good_ref(PathBuf::from("/this/path/should/not/be/touched.wasm"));
        r.wit_world = "other-world".into();
        let err = runtime.load(&r).unwrap_err();
        assert!(
            matches!(err, WasmHostError::WitWorldMismatch { .. }),
            "got {err:?}",
        );
    }

    #[test]
    fn missing_file_is_not_found_error() {
        let runtime = ComponentRuntime::new(WasmRuntimeLimits::default()).unwrap();
        let r = good_ref(PathBuf::from("/does/not/exist/cadenza.wasm"));
        let err = runtime.load(&r).unwrap_err();
        assert!(matches!(err, WasmHostError::NotFound { .. }), "got {err:?}");
    }

    #[test]
    fn malformed_bytes_become_compile_error() {
        let f = temp_wasm_path();
        let runtime = ComponentRuntime::new(WasmRuntimeLimits::default()).unwrap();
        let r = good_ref(f.path().to_path_buf());
        let err = runtime.load(&r).unwrap_err();
        assert!(matches!(err, WasmHostError::Compile(_)), "got {err:?}");
    }

    #[test]
    fn store_carries_request_context_and_limits() {
        let runtime = ComponentRuntime::new(WasmRuntimeLimits::default()).unwrap();
        let req = RequestContext {
            issue_id: Some("CAD-42".into()),
            plugin_name: Some("example-plugin".into()),
            workspace_path: Some(PathBuf::from("/tmp/ws")),
        };
        let store = runtime.new_store(req.clone());
        let data = store.data();
        assert_eq!(data.request.issue_id.as_deref(), Some("CAD-42"));
        assert_eq!(data.request.plugin_name.as_deref(), Some("example-plugin"));
        assert_eq!(data.request.workspace_path, Some(PathBuf::from("/tmp/ws")));
        assert_eq!(data.limiter.denied_growth(), 0);
        // Per #48 P2: the CUSTOM RuntimeLimiter must be the one wired
        // into wasmtime, so its cap reflects the runtime config.
        assert_eq!(
            data.limiter.max_memory_bytes(),
            runtime.limits().max_memory_bytes
        );
        assert_eq!(data.limiter.max_tables(), runtime.limits().max_tables);
        // PR #52 codex P1: max_instances must flow through the limiter
        // since StoreLimits is gone.
        assert_eq!(data.limiter.max_instances(), runtime.limits().max_instances);
    }

    #[test]
    fn limiter_reports_instances_cap_to_wasmtime() {
        // Direct ResourceLimiter probe: the trait's `instances()`
        // method must return the configured cap so wasmtime enforces
        // it at instantiation. Default trait impl returns 10_000.
        let limits = WasmRuntimeLimits {
            max_instances: 3,
            ..Default::default()
        };
        let limiter = RuntimeLimiter::new(&limits);
        assert_eq!(ResourceLimiter::instances(&limiter), 3);
    }

    #[test]
    fn limiter_reports_tables_cap_to_wasmtime() {
        // PR #52 codex P2: `table_growing` only constrains individual
        // table growth, not the *count* of tables a component can
        // allocate. The trait's `tables()` method must return the
        // configured cap so wasmtime enforces it at allocation time.
        // Default trait impl returns 10_000.
        let limits = WasmRuntimeLimits {
            max_tables: 5,
            ..Default::default()
        };
        let limiter = RuntimeLimiter::new(&limits);
        assert_eq!(ResourceLimiter::tables(&limiter), 5);
    }

    #[test]
    fn epoch_budget_uses_configured_timeout() {
        // Per #48 P1: the deadline MUST derive from
        // `epoch_timeout_ms`, not be hardcoded to 1.
        let limits = WasmRuntimeLimits {
            epoch_timeout_ms: 7_500,
            ..Default::default()
        };
        let runtime = ComponentRuntime::new(limits).unwrap();
        assert_eq!(runtime.epoch_budget_ticks(), 7_500);
    }

    #[test]
    fn epoch_budget_zero_clamps_to_one() {
        // Boundary: a zero budget must still trap rather than letting
        // an unsupervised store run forever.
        let limits = WasmRuntimeLimits {
            epoch_timeout_ms: 0,
            ..Default::default()
        };
        let runtime = ComponentRuntime::new(limits).unwrap();
        assert_eq!(runtime.epoch_budget_ticks(), 1);
    }

    #[test]
    fn limiter_allows_at_cap_and_denies_above_cap() {
        let limits = WasmRuntimeLimits {
            max_memory_bytes: 1_024,
            ..Default::default()
        };
        let mut limiter = RuntimeLimiter::new(&limits);
        assert!(limiter.memory_growing(0, 1_024, None).unwrap());
        assert_eq!(limiter.denied_growth(), 0);
        assert!(!limiter.memory_growing(0, 1_025, None).unwrap());
        assert_eq!(limiter.denied_growth(), 1);
    }

    #[test]
    fn limiter_table_cap_paired_edges() {
        let limits = WasmRuntimeLimits {
            max_tables: 4,
            ..Default::default()
        };
        let mut limiter = RuntimeLimiter::new(&limits);
        assert!(limiter.table_growing(0, 4, None).unwrap());
        assert!(!limiter.table_growing(0, 5, None).unwrap());
        assert_eq!(limiter.denied_growth(), 1);
    }

    #[test]
    fn request_context_has_no_raw_secret_field() {
        // Compile-time documentation. RequestContext exposes only
        // issue_id + workspace_path; secret material is intentionally
        // absent and the orchestrator passes credentials via host
        // functions that never copy raw values into guest memory.
        let _ = RequestContext::default();
    }

    #[test]
    fn log_sink_is_bounded_and_counts_drops() {
        // A chatty guest must not grow the sink without bound; past the
        // capacity, records are dropped and counted instead of stored.
        let sink = LogSink::with_capacity(2);
        for _ in 0..5 {
            sink.push(HostLogRecord {
                op: "host-time.now-millis".to_string(),
                issue_id: None,
                plugin_name: None,
                level: None,
                message: None,
                fields_json: None,
            });
        }
        assert_eq!(sink.records().len(), 2);
        assert_eq!(sink.dropped(), 3);
    }
}
