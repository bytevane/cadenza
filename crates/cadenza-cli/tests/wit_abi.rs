//! Acceptance tests for Issue #5 — WIT ABI gate.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("cadenza-cli is two levels below the workspace root")
        .to_path_buf()
}

#[test]
fn runtime_wit_matches_abi_snapshot() {
    let source = include_str!("../../../wit/runtime.wit");
    let expected = include_str!("../../../abi/expected/runtime.wit");
    assert_eq!(
        source, expected,
        "wit/runtime.wit drifted from abi/expected/runtime.wit; copy the source into the snapshot in the same PR and add an ADR",
    );
}

#[test]
fn check_wit_abi_script_fails_closed_when_wasm_tools_missing() {
    let root = workspace_root();
    let script = root.join("scripts/check-wit-abi.sh");
    assert!(script.is_file(), "expected {} to exist", script.display());

    // Strip wasm-tools out of PATH. /usr/bin and /bin only.
    let output = Command::new("env")
        .args([
            "-i",
            "PATH=/usr/bin:/bin",
            script.to_str().expect("script path is utf-8"),
        ])
        .current_dir(&root)
        .output()
        .expect("env(1) should be runnable");

    assert!(
        !output.status.success(),
        "expected check-wit-abi.sh to exit non-zero with no wasm-tools on PATH; got success.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("wasm-tools is required"),
        "expected stderr to mention `wasm-tools is required`, got: {stderr}",
    );
}
