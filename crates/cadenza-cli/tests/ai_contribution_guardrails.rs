//! Acceptance tests for Issue #6 — AI contribution workflow guardrails.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("cadenza-cli is two levels below the workspace root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let path = workspace_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn pr_template_requires_linked_issue_and_contract_impact() {
    let body = read(".github/pull_request_template.md");
    for marker in [
        "Linked issue",
        "Closes #",
        "Contract impact",
        "Codex app-server schema",
        "WIT ABI",
        "Secret handling",
        "Observability field names",
    ] {
        assert!(
            body.contains(marker),
            ".github/pull_request_template.md is missing required marker `{marker}`",
        );
    }
}

#[test]
fn pr_template_requires_ai_assistance_metadata() {
    let body = read(".github/pull_request_template.md");
    for marker in [
        "AI assistance",
        "Tool used",
        "Prompt template used",
        "Scope of the AI patch",
    ] {
        assert!(
            body.contains(marker),
            ".github/pull_request_template.md is missing required marker `{marker}` in the AI assistance section",
        );
    }
}

#[test]
fn pr_template_carries_reviewer_checklist_for_ai_patches() {
    let body = read(".github/pull_request_template.md");
    for marker in [
        "Reviewer checklist",
        "No invented protocol fields",
        "Tests fail before the implementation lands",
        "Patch stays inside the declared scope",
    ] {
        assert!(
            body.contains(marker),
            ".github/pull_request_template.md is missing reviewer-checklist marker `{marker}`",
        );
    }
}

#[test]
fn contributing_ai_codifies_branch_naming_and_patch_scope() {
    let body = read("CONTRIBUTING_AI.md");
    for marker in [
        "Branch naming",
        "`issue-<n>-<slug>`",
        "`infra/<slug>`",
        "`sec/<slug>`",
        "Patch scope",
        "Cross-component patches",
        "One PR = one issue",
    ] {
        assert!(
            body.contains(marker),
            "CONTRIBUTING_AI.md is missing required marker `{marker}`",
        );
    }
}

#[test]
fn contributing_ai_references_current_contract_docs() {
    let body = read("CONTRIBUTING_AI.md");
    for marker in [
        "ARCHITECTURE.md",
        "CONTRACTS.md",
        "docs/operations/wit-abi-versioning.md",
        "SECURITY.md",
    ] {
        assert!(
            body.contains(marker),
            "CONTRIBUTING_AI.md should reference `{marker}` as required context",
        );
    }
}
