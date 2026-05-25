//! End-to-end host-capability tests for issues #16 and #17.
//!
//! These build the real example component (`cadenza-linear-graphql-plugin`)
//! for `wasm32-wasip2`, link the host functions, instantiate, and call
//! `tool.run`. They cover the acceptance criteria: the example plugin logs
//! and reads an allowed workspace file, an out-of-root read fails,
//! `secret-exists` reports presence only, a host-mediated `linear-graphql`
//! call runs without leaking the token, and host errors surface via the
//! shared WIT `host-error` model.
//!
//! The guest is built on demand (once) via a nested `cargo build`. The
//! `wasm32-wasip2` target is a hard requirement (CI installs it); a missing
//! target makes these tests fail rather than silently skip.

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

use cadenza_obs::Scrubber;
use cadenza_wasm_host::{
    ComponentRuntime, HostCapabilities, HostClock, HostError, LinearCall, LinearCapability,
    LinearHttpResult, LinearTransport, LinearTransportError, LogSink, RequestContext, ToolInput,
    ToolOutput, WIT_PACKAGE, WIT_WORLD, WasmComponentRef, WasmHostError, WasmRuntimeLimits,
};

/// Host-side mock transport for the `host-linear` integration tests. It is the
/// sole injector of the Linear credential — the guest never supplies one — and
/// records the calls it received so a test can assert the host-normalised
/// request. The `token` models a credential that must stay host-side.
#[derive(Debug)]
struct MockLinearTransport {
    token: String,
    body_json: String,
    seen: Mutex<Vec<LinearCall>>,
}

impl MockLinearTransport {
    fn new(token: &str, body_json: &str) -> Arc<Self> {
        Arc::new(Self {
            token: token.to_string(),
            body_json: body_json.to_string(),
            seen: Mutex::new(Vec::new()),
        })
    }
}

impl LinearTransport for MockLinearTransport {
    fn execute(&self, call: LinearCall) -> Result<LinearHttpResult, LinearTransportError> {
        // Auth is injected here, host-side; the token never crosses to guest.
        let _auth = format!("Authorization: Bearer {}", self.token);
        self.seen.lock().unwrap().push(call);
        Ok(LinearHttpResult {
            status: 200,
            body_json: self.body_json.clone(),
        })
    }
}

/// Build the example plugin once and cache the resulting `.wasm` path.
fn plugin_component() -> &'static PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(build_plugin)
}

fn build_plugin() -> PathBuf {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("resolve workspace root");
    let output = Command::new(env!("CARGO"))
        .current_dir(&workspace_root)
        .args([
            "build",
            "-p",
            "cadenza-linear-graphql-plugin",
            "--target",
            "wasm32-wasip2",
            "--message-format=json",
        ])
        .output()
        .expect("spawn nested cargo build for the example plugin");
    assert!(
        output.status.success(),
        "building the example plugin failed:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    // Parse cargo's JSON artifact records rather than guessing the target
    // directory layout (honours a custom CARGO_TARGET_DIR).
    let stdout = String::from_utf8(output.stdout).expect("cargo json output is utf-8");
    for line in stdout.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value["reason"] != "compiler-artifact" {
            continue;
        }
        let Some(files) = value["filenames"].as_array() else {
            continue;
        };
        for file in files {
            if let Some(path) = file.as_str() {
                if path.ends_with(".wasm") && path.contains("cadenza_linear_graphql_plugin") {
                    return PathBuf::from(path);
                }
            }
        }
    }
    panic!("no .wasm artifact for cadenza-linear-graphql-plugin in cargo output");
}

fn runtime() -> ComponentRuntime {
    // `WasmRuntimeLimits::default()` must instantiate a real component
    // out-of-the-box (issue #74) — no per-test override required.
    ComponentRuntime::new(WasmRuntimeLimits::default()).expect("engine init")
}

fn load(rt: &ComponentRuntime) -> cadenza_wasm_host::LoadedComponent {
    let component_ref = WasmComponentRef {
        name: "example".to_string(),
        path: plugin_component().clone(),
        wit_package: WIT_PACKAGE.to_string(),
        wit_world: WIT_WORLD.to_string(),
    };
    rt.load(&component_ref).expect("load example component")
}

fn run(
    rt: &ComponentRuntime,
    loaded: &cadenza_wasm_host::LoadedComponent,
    request: RequestContext,
    caps: HostCapabilities,
    args: serde_json::Value,
) -> Result<ToolOutput, HostError> {
    let mut store = rt.new_store_with(request, caps);
    let input = ToolInput {
        name: "demo".to_string(),
        args_json: args.to_string(),
    };
    rt.run_tool(&mut store, loaded, input)
        .expect("host side of run_tool succeeds")
}

fn request_in(root: &std::path::Path) -> RequestContext {
    RequestContext {
        issue_id: Some("CAD-16".to_string()),
        plugin_name: Some("example".to_string()),
        workspace_path: Some(root.to_path_buf()),
    }
}

#[test]
fn real_component_instantiates_under_default_limits() {
    // Issue #74: a real `wasm32-wasip2` component must instantiate under
    // `WasmRuntimeLimits::default()` with no per-test override. Previously
    // `max_tables` was conflated with the per-table element cap, so the
    // production default (64) denied a real component's function table —
    // wasmtime reported "table minimum size of 112 elements exceeds table
    // limits" before any tool code ran. Construct a fresh store from the
    // default-limit runtime and call `tool.run` to prove instantiation works
    // end-to-end (a failure here would short-circuit before invocation).
    let rt = ComponentRuntime::new(WasmRuntimeLimits::default()).expect("engine init");
    assert_eq!(rt.limits(), &WasmRuntimeLimits::default());
    let loaded = load(&rt);
    let tmp = tempfile::tempdir().unwrap();
    let out = run(
        &rt,
        &loaded,
        request_in(tmp.path()),
        HostCapabilities {
            clock: HostClock::Fixed(1),
            ..Default::default()
        },
        serde_json::json!({}),
    )
    .expect("default limits must let a real component instantiate and run");
    // The component returns *some* JSON object on the happy path; the exact
    // shape is covered by the capability tests below — here we only assert
    // instantiation reached the tool entry point.
    let _: serde_json::Value =
        serde_json::from_str(&out.result_json).expect("tool returned valid JSON");
}

#[test]
fn example_plugin_logs_and_reads_allowed_file() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("notes.txt"), b"hello workspace").unwrap();

    let rt = runtime();
    let loaded = load(&rt);
    let log_sink = LogSink::new();
    let caps = HostCapabilities {
        secret_names: ["LINEAR_API_KEY".to_string()].into_iter().collect(),
        scrubber: Scrubber::empty(),
        clock: HostClock::Fixed(1_234),
        log_sink: log_sink.clone(),
        linear: None,
    };

    let out = run(
        &rt,
        &loaded,
        request_in(tmp.path()),
        caps,
        serde_json::json!({
            "message": "running demo",
            "read_path": "notes.txt",
            "secret_name": "LINEAR_API_KEY",
        }),
    )
    .expect("guest run succeeds");

    assert!(!out.is_error);
    let summary: serde_json::Value = serde_json::from_str(&out.result_json).unwrap();
    assert_eq!(summary["now_millis"], 1_234);
    assert_eq!(summary["logged"], true);
    assert_eq!(summary["read"]["text"], "hello workspace");
    assert_eq!(summary["read"]["len"], 15);
    assert_eq!(summary["read"]["truncated"], false);
    assert_eq!(summary["secret_present"], true);

    // Every host call carries issue/plugin context, and all four
    // capabilities were exercised.
    let records = log_sink.records();
    assert!(
        records
            .iter()
            .all(|r| r.issue_id.as_deref() == Some("CAD-16")
                && r.plugin_name.as_deref() == Some("example")),
        "a host call was logged without issue/plugin context: {records:?}",
    );
    for op in [
        "host-time.now-millis",
        "host-log.log",
        "host-workspace.workspace-read",
        "host-secrets.secret-exists",
    ] {
        assert!(
            records.iter().any(|r| r.op == op),
            "missing host-call record for {op}: {records:?}",
        );
    }
}

#[test]
fn out_of_root_read_is_denied() {
    let tmp = tempfile::tempdir().unwrap();
    // A real sibling file outside the workspace root; the read must still be
    // denied by containment, not merely by absence.
    std::fs::write(tmp.path().join("escape.txt"), b"secret sibling").unwrap();
    let root = tmp.path().join("ws");
    std::fs::create_dir(&root).unwrap();

    let rt = runtime();
    let loaded = load(&rt);

    let result = run(
        &rt,
        &loaded,
        request_in(&root),
        HostCapabilities::default(),
        serde_json::json!({ "read_path": "../escape.txt" }),
    );

    assert!(
        matches!(result, Err(HostError::OutsideRoot)),
        "out-of-root read should be denied with outside-root, got {result:?}",
    );
}

#[test]
fn secret_exists_reports_presence_only() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = runtime();
    let loaded = load(&rt);
    let caps = || HostCapabilities {
        secret_names: ["PRESENT_KEY".to_string()].into_iter().collect(),
        clock: HostClock::Fixed(1),
        ..Default::default()
    };

    let present = run(
        &rt,
        &loaded,
        request_in(tmp.path()),
        caps(),
        serde_json::json!({ "secret_name": "PRESENT_KEY" }),
    )
    .expect("guest run succeeds");
    let present_summary: serde_json::Value = serde_json::from_str(&present.result_json).unwrap();
    assert_eq!(present_summary["secret_present"], true);

    let absent = run(
        &rt,
        &loaded,
        request_in(tmp.path()),
        caps(),
        serde_json::json!({ "secret_name": "MISSING_KEY" }),
    )
    .expect("guest run succeeds");
    let absent_summary: serde_json::Value = serde_json::from_str(&absent.result_json).unwrap();
    assert_eq!(absent_summary["secret_present"], false);

    // The capability returns only a boolean — there is no value field in the
    // result at all.
    assert!(present.result_json.contains("\"secret_present\":true"));
    assert!(!present.result_json.contains("secret_value"));
}

#[test]
fn empty_secret_name_is_invalid_argument() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = runtime();
    let loaded = load(&rt);

    let result = run(
        &rt,
        &loaded,
        request_in(tmp.path()),
        HostCapabilities::default(),
        serde_json::json!({ "secret_name": "" }),
    );
    assert!(
        matches!(result, Err(HostError::InvalidArgument(_))),
        "empty secret name should be invalid-argument, got {result:?}",
    );
}

#[test]
fn log_redacts_registered_secret_value_in_message() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = runtime();
    let loaded = load(&rt);
    let log_sink = LogSink::new();
    let caps = HostCapabilities {
        scrubber: Scrubber::with_secrets(vec!["lr_live_SECRET".to_string()]),
        clock: HostClock::Fixed(1),
        log_sink: log_sink.clone(),
        ..Default::default()
    };

    run(
        &rt,
        &loaded,
        request_in(tmp.path()),
        caps,
        serde_json::json!({ "message": "auth token is lr_live_SECRET here" }),
    )
    .expect("guest run succeeds");

    let logged = log_sink
        .records()
        .into_iter()
        .find(|r| r.op == "host-log.log")
        .expect("a host-log.log record");
    let message = logged.message.expect("log record carries a message");
    assert!(
        !message.contains("lr_live_SECRET"),
        "registered secret leaked into log message: {message}",
    );
    assert!(
        message.contains("***REDACTED***"),
        "expected redaction marker in {message}",
    );
}

#[test]
fn example_plugin_runs_allowed_linear_operation_without_leaking_token() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = runtime();
    let loaded = load(&rt);

    let token = "lr_live_HOSTONLY_TOKEN";
    let transport = MockLinearTransport::new(token, r#"{"data":{"viewer":{"id":"u_42"}}}"#);
    let log_sink = LogSink::new();
    let caps = HostCapabilities {
        // The scrubber knows the host token so any accidental echo is redacted.
        scrubber: Scrubber::with_secrets(vec![token.to_string()]),
        clock: HostClock::Fixed(7),
        log_sink: log_sink.clone(),
        linear: Some(LinearCapability::with_default_allowlist(transport.clone())),
        ..Default::default()
    };

    let out = run(
        &rt,
        &loaded,
        request_in(tmp.path()),
        caps,
        serde_json::json!({
            "linear_operation": "Viewer",
            "linear_query": "query Viewer { viewer { id } }",
            "linear_variables": "{\"first\":1}",
            "linear_mode": "read",
        }),
    )
    .expect("guest run succeeds");

    assert!(!out.is_error);
    let summary: serde_json::Value = serde_json::from_str(&out.result_json).unwrap();
    assert_eq!(summary["linear"]["status"], 200);
    assert!(
        summary["linear"]["body_json"]
            .as_str()
            .unwrap()
            .contains("u_42"),
        "guest should observe the GraphQL response body: {}",
        out.result_json,
    );

    // The host transport saw the host-normalised call; the guest never
    // supplied a header or endpoint.
    let calls = transport.seen.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].operation_name.as_deref(), Some("Viewer"));
    assert_eq!(calls[0].endpoint, LinearCapability::DEFAULT_ENDPOINT);

    // The raw token must not appear in anything the guest can read or that is
    // logged.
    assert!(
        !out.result_json.contains(token),
        "token leaked into guest result: {}",
        out.result_json,
    );
    let audit = log_sink
        .records()
        .into_iter()
        .find(|r| r.op == "host-linear.linear-graphql")
        .expect("a host-linear audit record");
    let dump = format!("{audit:?}");
    assert!(!dump.contains(token), "token leaked into audit log: {dump}");
    // The audit fingerprints the query rather than logging it verbatim.
    let fields = audit.fields_json.expect("audit carries fields");
    assert!(fields.contains("query_fingerprint"), "fields: {fields}");
    assert!(
        !fields.contains("viewer { id }"),
        "raw query text leaked into audit: {fields}",
    );
}

#[test]
fn example_plugin_passes_object_form_linear_variables() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = runtime();
    let loaded = load(&rt);
    let transport = MockLinearTransport::new("tok", r#"{"data":{}}"#);
    let caps = HostCapabilities {
        clock: HostClock::Fixed(1),
        linear: Some(LinearCapability::with_default_allowlist(transport.clone())),
        ..Default::default()
    };

    run(
        &rt,
        &loaded,
        request_in(tmp.path()),
        caps,
        // Object form, not a string — must reach the host as object JSON, not {}.
        serde_json::json!({
            "linear_query": "query Q($first:Int){ issues(first:$first){ nodes { id } } }",
            "linear_variables": { "first": 1 },
        }),
    )
    .expect("guest run succeeds");

    let calls = transport.seen.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let vars: serde_json::Value = serde_json::from_str(&calls[0].variables_json).unwrap();
    assert_eq!(
        vars["first"], 1,
        "object variables were dropped: {:?}",
        calls[0].variables_json
    );
}

#[test]
fn example_plugin_linear_denied_when_capability_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = runtime();
    let loaded = load(&rt);

    // No linear capability configured: the guest call must fail closed.
    let result = run(
        &rt,
        &loaded,
        request_in(tmp.path()),
        HostCapabilities {
            clock: HostClock::Fixed(1),
            ..Default::default()
        },
        serde_json::json!({
            "linear_query": "query { viewer { id } }",
        }),
    );

    assert!(
        matches!(result, Err(HostError::Denied(_))),
        "unconfigured linear capability should deny, got {result:?}",
    );
}

#[test]
fn read_limit_truncates() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("big.txt"), b"0123456789").unwrap();
    let rt = runtime();
    let loaded = load(&rt);

    let out = run(
        &rt,
        &loaded,
        request_in(tmp.path()),
        HostCapabilities {
            clock: HostClock::Fixed(1),
            ..Default::default()
        },
        serde_json::json!({ "read_path": "big.txt", "read_limit": 4 }),
    )
    .expect("guest run succeeds");
    let summary: serde_json::Value = serde_json::from_str(&out.result_json).unwrap();
    assert_eq!(summary["read"]["text"], "0123");
    assert_eq!(summary["read"]["truncated"], true);
}

/// End-to-end proof for issue #75 case 1: a real component whose declared
/// minimum memory exceeds the host's per-store cap must surface as
/// `WasmHostError::LimitBreached` at `run_tool` time, NOT `Link`.
///
/// The `RuntimeLimiter` signals denial via a typed `ResourceLimitBreached`
/// payload inside `wasmtime::Error`; wasmtime propagates it through
/// `ToolRuntime::instantiate`; `classify_instantiate` downcasts it. The
/// previous classification (`Link`) was misleading — callers wishing to
/// branch on "the guest tripped a host cap" vs "wiring is broken" had no
/// reliable signal. This test guards the propagation chain end to end.
#[test]
fn instantiation_resource_limit_breach_surfaces_as_limit_breached() {
    let tmp = tempfile::tempdir().unwrap();
    let limits = WasmRuntimeLimits {
        // 4 KiB is well below the example plugin's declared minimum memory
        // (the Rust std runtime needs at least a few pages just to start), so
        // wasmtime will consult the limiter at instantiate time and our
        // `memory_growing` callback will deny it with the typed signal.
        max_memory_bytes: 4 * 1024,
        max_tables: 10_000,
        ..Default::default()
    };
    let rt = ComponentRuntime::new(limits).expect("engine init");
    let loaded = load(&rt);
    let mut store = rt.new_store_with(request_in(tmp.path()), HostCapabilities::default());
    let input = ToolInput {
        name: "demo".to_string(),
        args_json: "{}".to_string(),
    };
    let err = rt
        .run_tool(&mut store, &loaded, input)
        .expect_err("instantiation with a 4 KiB memory cap must fail");
    assert!(
        matches!(err, WasmHostError::LimitBreached(_)),
        "issue #75 case 1: over-cap instantiation must classify as LimitBreached, got {err:?}",
    );
}

/// Issue #82: a component declaring more *tables* than the configured count cap
/// must surface as `WasmHostError::LimitBreached` at `run_tool` time, NOT `Link`.
///
/// The host pre-checks the typed `Component::resources_required()` table count
/// before instantiation (`check_declared_resource_counts`) and rejects an
/// over-cap with a typed `LimitBreached`, so the breach never reaches wasmtime's
/// stringly-typed `bump_resource_counts` error that `classify_instantiate` would
/// label `Link`. A real wasip2 component declares one function table, so a
/// `max_tables` of 0 trips the pre-check. This guards the whole wiring end to
/// end: drop the pre-check and the denial regresses to `Link`.
#[test]
fn table_count_cap_denial_surfaces_as_limit_breached() {
    let limits = WasmRuntimeLimits {
        // Below the real component's one function table, so the table COUNT cap
        // trips. Distinct from the per-table *element* cap (`max_table_elements`,
        // left at its roomy default) and from the per-memory *size* cap.
        max_tables: 0,
        ..Default::default()
    };
    let rt = ComponentRuntime::new(limits).expect("engine init");
    let loaded = load(&rt);

    let tmp = tempfile::tempdir().unwrap();
    let mut store = rt.new_store_with(request_in(tmp.path()), HostCapabilities::default());
    let input = ToolInput {
        name: "demo".to_string(),
        args_json: "{}".to_string(),
    };
    let err = rt
        .run_tool(&mut store, &loaded, input)
        .expect_err("instantiation over the table count cap must fail");
    assert!(
        matches!(err, WasmHostError::LimitBreached(_)),
        "issue #82: table count-cap breach must classify as LimitBreached, got {err:?}",
    );
}

/// Issue #63, path 3 (now scoped to the *instance* count cap by #82): an
/// instance count-cap denial is excluded from the limiter's denial telemetry.
/// Wasmtime enforces the instance-count cap during instantiation
/// (`bump_resource_counts`) and never calls back into the `RuntimeLimiter`, so
/// the limiter cannot observe the denial. wasmtime 45 also exposes no public
/// component instance count to pre-check (unlike tables/memories, which #82 now
/// catches as `LimitBreached`), so an instance-count breach still surfaces as
/// `WasmHostError::Link` — a non-trap "resource limit exceeded" error, NOT a
/// trap-derived `LimitBreached` (tracked in #86). This drives a real component
/// against an instance count cap of 1 (a genuine wasip2 component instantiates
/// at least 2 instances), with a roomy table cap so the instance cap is what
/// trips, and asserts both the `Link` classification and that neither growth
/// counter moved — codifying the documented contract on `RuntimeLimiter`.
#[test]
fn instance_count_cap_denial_surfaces_as_link_and_leaves_counters_untouched() {
    let limits = WasmRuntimeLimits {
        // Roomy table *count* cap so the table pre-check (#82) does not trip
        // before the instance count cap — isolating the instance COUNT cap,
        // which is the sub-case still classified as `Link`.
        max_tables: 10_000,
        max_instances: 1,
        ..Default::default()
    };
    let rt = ComponentRuntime::new(limits).expect("engine init");
    let loaded = load(&rt);

    let tmp = tempfile::tempdir().unwrap();
    let mut store = rt.new_store_with(request_in(tmp.path()), HostCapabilities::default());
    let input = ToolInput {
        name: "demo".to_string(),
        args_json: "{}".to_string(),
    };
    let outcome = rt.run_tool(&mut store, &loaded, input);

    // The instance count-cap denial is a plain "resource limit exceeded" error,
    // not a `wasmtime::Trap`, so `classify_instantiate` maps it to `Link`.
    match outcome {
        Err(WasmHostError::Link(msg)) => {
            assert!(
                msg.contains("resource limit exceeded") && msg.contains("instance count"),
                "unexpected Link message: {msg}",
            );
        }
        other => panic!("expected WasmHostError::Link for instance-count cap, got {other:?}"),
    }

    // The limiter was never consulted for the count-cap denial, so neither
    // telemetry counter must have moved.
    assert_eq!(store.data().limiter.denied_growth(), 0);
    assert_eq!(store.data().limiter.grow_failed_after_allow(), 0);
}
