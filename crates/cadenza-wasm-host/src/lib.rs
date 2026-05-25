//! Wasmtime component loader and resource-limit boundary for Cadenza.
//!
//! The host configures Wasmtime with explicit memory/table/instance
//! caps and an epoch-based timeout. Components are loaded from disk
//! via `ComponentRuntime::load`; the WIT package/world declared by
//! the caller must match cadenza's frozen baseline (`WIT_PACKAGE` /
//! `WIT_WORLD`) otherwise the loader fails closed.
//!
//! Host capability functions (`host-log`, `host-time`, `host-workspace`,
//! `host-secrets`) are implemented in [`capabilities`] and linked into the
//! Wasmtime `Linker` by [`ComponentRuntime::run_tool`]. The store carries a
//! [`RequestContext`] (issue/plugin identity + workspace root), a
//! [`HostCapabilities`] bundle (configured secret names, redaction scrubber,
//! clock, captured log sink), the `RuntimeLimiter`, and a locked-down WASI
//! context that satisfies only the language-runtime imports the guest pulls
//! in (no preopens, no inherited env/stdio) — see ADR 0005.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use cadenza_obs::Scrubber;
use serde::{Deserialize, Serialize};
use wasmtime::component::{Component, ResourceTable};
use wasmtime::{Config, Engine, ResourceLimiter, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

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
/// workspace root ([`RequestContext`]), the host-side capability config
/// ([`HostCapabilities`]), and a locked-down WASI context (no preopens, no
/// inherited env/stdio) that only satisfies the language-runtime imports the
/// guest emits — workspace access is exclusively via `host-workspace`.
pub struct StoreState {
    pub limiter: RuntimeLimiter,
    pub request: RequestContext,
    caps: HostCapabilities,
    wasi: WasiCtx,
    table: ResourceTable,
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

impl WasiView for StoreState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
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

/// Cloneable, shareable capture of [`HostLogRecord`]s. Cheap to clone (the
/// buffer is behind an `Arc<Mutex<…>>`), so a caller can hold a handle and
/// inspect what the guest logged after `run_tool` returns.
#[derive(Debug, Clone, Default)]
pub struct LogSink {
    inner: Arc<Mutex<Vec<HostLogRecord>>>,
}

impl LogSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of the records captured so far.
    pub fn records(&self) -> Vec<HostLogRecord> {
        self.inner.lock().expect("log sink mutex poisoned").clone()
    }

    pub(crate) fn push(&self, record: HostLogRecord) {
        self.inner
            .lock()
            .expect("log sink mutex poisoned")
            .push(record);
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

/// Wasmtime engine + reusable config. Cheap to clone; one per host
/// process is typical, with per-load `Store` instances on top.
pub struct ComponentRuntime {
    engine: Engine,
    limits: WasmRuntimeLimits,
}

impl ComponentRuntime {
    pub fn new(limits: WasmRuntimeLimits) -> Result<Self, WasmHostError> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.epoch_interruption(true);
        config.consume_fuel(false);
        let engine = Engine::new(&config).map_err(|e| WasmHostError::Engine(e.to_string()))?;
        Ok(Self { engine, limits })
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
    /// initial epoch deadline. The orchestrator advances the engine
    /// epoch periodically (`Engine::increment_epoch`) — when the
    /// deadline elapses without progress, Wasmtime traps the guest
    /// and `WasmHostError::Timeout` is returned by the caller.
    pub fn new_store(&self, request: RequestContext) -> Store<StoreState> {
        self.new_store_with(request, HostCapabilities::default())
    }

    /// Like [`ComponentRuntime::new_store`] but with explicit host
    /// capabilities (configured secret names, redaction scrubber, clock, log
    /// sink). The WASI context is locked down: no preopened directories, no
    /// inherited environment or stdio, so the only filesystem reach the guest
    /// has is the contained `host-workspace.workspace-read` capability.
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
            wasi: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limiter);
        // Use the configured epoch budget (#48 P1). The orchestrator
        // advances the engine epoch via `Engine::increment_epoch` at a
        // fixed cadence (target: 1 tick per millisecond), so this
        // deadline expressed in *ticks* approximates the budget in ms.
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
}
