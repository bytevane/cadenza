//! Acceptance test for Issue #4 — schema regeneration script must fail closed
//! when the codex CLI is unavailable.

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
fn codex_schema_script_fails_closed_when_codex_missing() {
    let root = workspace_root();
    let script = root.join("scripts/codex-schema.sh");
    assert!(script.is_file(), "expected {} to exist", script.display());

    // Strip codex (and jq, etc.) out of PATH by running with a hermetic env.
    // The script's pre-flight must exit non-zero with a clear stderr message.
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
        "expected codex-schema.sh to exit non-zero with no codex on PATH; got success.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("codex is required"),
        "expected stderr to mention `codex is required`, got: {stderr}",
    );
}
