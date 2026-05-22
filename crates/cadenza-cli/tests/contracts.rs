//! Acceptance tests for Issue #3 — Freeze upstream versions and contract registry.

use std::path::{Path, PathBuf};
use std::process::Command;

const EXPECTED_REPOSITORY: &str = "https://github.com/bytevane/cadenza";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("cadenza-cli is two levels below the workspace root")
        .to_path_buf()
}

#[test]
fn repository_metadata_uses_bytevane_url() {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(workspace_root())
        .output()
        .expect("`cargo metadata` should execute");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let body = String::from_utf8(output.stdout).expect("metadata is utf-8");

    let placeholder = "https://example.invalid";
    assert!(
        !body.contains(placeholder),
        "cargo metadata still exposes placeholder repository {placeholder}",
    );
    assert!(
        body.contains(EXPECTED_REPOSITORY),
        "cargo metadata does not advertise {EXPECTED_REPOSITORY}",
    );
}

#[test]
fn registry_documents_targeted_upstreams() {
    let body = std::fs::read_to_string(workspace_root().join("tools/versions.toml"))
        .expect("tools/versions.toml is readable");

    for required in [
        "symphony_spec_sha",
        "cli_version",
        "schema_sha256_file",
        "wit_package",
        "wasmtime_version",
    ] {
        assert!(
            body.lines()
                .any(|line| line.trim_start().starts_with(required)),
            "tools/versions.toml is missing `{required}` — a fresh clone cannot identify the targeted upstream",
        );
    }
}

#[test]
fn contracts_md_exists_and_documents_change_process() {
    let body = std::fs::read_to_string(workspace_root().join("CONTRACTS.md"))
        .expect("CONTRACTS.md should exist");
    for marker in ["How to change a pinned contract", "MVP_CRITICAL_KEYS"] {
        assert!(
            body.contains(marker),
            "CONTRACTS.md missing required marker `{marker}`",
        );
    }
}
