//! Acceptance tests for Issue #3 — Freeze upstream versions and contract registry.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const EXPECTED_REPOSITORY: &str = "https://github.com/bytevane/cadenza";
const PLACEHOLDER_HOST: &str = "example.invalid";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("cadenza-cli is two levels below the workspace root")
        .to_path_buf()
}

fn cargo_metadata_json() -> serde_json::Value {
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
    serde_json::from_slice(&output.stdout).expect("cargo metadata emits valid JSON")
}

#[test]
fn repository_metadata_uses_bytevane_url_on_every_workspace_package() {
    let metadata = cargo_metadata_json();

    let workspace_ids: HashSet<&str> = metadata["workspace_members"]
        .as_array()
        .expect("workspace_members is an array")
        .iter()
        .map(|v| v.as_str().expect("member id is a string"))
        .collect();
    assert!(
        !workspace_ids.is_empty(),
        "workspace has no members — metadata shape changed?",
    );

    let mut checked = 0;
    for pkg in metadata["packages"]
        .as_array()
        .expect("packages is an array")
    {
        let id = pkg["id"].as_str().expect("package id is a string");
        if !workspace_ids.contains(id) {
            continue;
        }
        let name = pkg["name"].as_str().unwrap_or("<unknown>");
        let repository = pkg["repository"].as_str();
        assert_eq!(
            repository,
            Some(EXPECTED_REPOSITORY),
            "package `{name}` advertises repository {repository:?}, expected {EXPECTED_REPOSITORY:?}",
        );
        let repo = repository.unwrap();
        assert!(
            !repo.contains(PLACEHOLDER_HOST),
            "package `{name}` still references placeholder host `{PLACEHOLDER_HOST}`",
        );
        checked += 1;
    }
    assert_eq!(
        checked,
        workspace_ids.len(),
        "did not visit every workspace package; visited {checked}, expected {}",
        workspace_ids.len(),
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
            body.lines().any(|line| {
                let line = line.trim_start();
                let Some(rest) = line.strip_prefix(required) else {
                    return false;
                };
                matches!(rest.bytes().next(), Some(b' ' | b'\t' | b'='))
                    && rest.trim_start().starts_with('=')
            }),
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
