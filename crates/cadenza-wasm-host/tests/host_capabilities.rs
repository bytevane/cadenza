//! End-to-end host-capability tests for issue #16.
//!
//! These build the real example component (`cadenza-linear-graphql-plugin`)
//! for `wasm32-wasip2`, link the four in-scope host functions, instantiate,
//! and call `tool.run`. They cover the acceptance criteria: the example
//! plugin logs and reads an allowed workspace file, an out-of-root read
//! fails, `secret-exists` reports presence only, and host errors surface via
//! the shared WIT `host-error` model.
//!
//! The guest is built on demand (once) via a nested `cargo build`. The
//! `wasm32-wasip2` target is a hard requirement (CI installs it); a missing
//! target makes these tests fail rather than silently skip.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use cadenza_obs::Scrubber;
use cadenza_wasm_host::{
    ComponentRuntime, HostCapabilities, HostClock, HostError, LogSink, RequestContext, ToolInput,
    ToolOutput, WIT_PACKAGE, WIT_WORLD, WasmComponentRef, WasmRuntimeLimits,
};

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
    // The default `max_tables` (64) is consulted by `RuntimeLimiter` as a
    // *per-table element* cap (see lib.rs `table_growing`), and a real
    // component's function table needs more than that. Use a roomier cap so
    // a genuine component instantiates; capability behaviour is what these
    // tests exercise, not the resource-limit edges (those live in the unit
    // tests). The limiter's count/size conflation is tracked in #74.
    let limits = WasmRuntimeLimits {
        max_tables: 10_000,
        ..Default::default()
    };
    ComponentRuntime::new(limits).expect("engine init")
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
