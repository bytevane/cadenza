//! author-time PR gate: a pure `evaluate` over changed files + PR body, plus an
//! IO driver in `main.rs`. Earned by the aiops-platform port, whose contract
//! deviations shipped because checks were audit-time judgement, not author-time
//! mechanics. See CONTRIBUTING_AI.md "Anti-over-design principles".

/// A file changed in the PR, with just enough info to classify contract impact.
#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub path: String,
    /// `tools/versions.toml` only: MVP-critical keys whose assigned value changed
    /// between base and head. Empty for every other file (and for versions.toml
    /// edits that only touched comments/formatting).
    pub changed_version_keys: Vec<String>,
}

/// Result of a gate run. Empty `violations` == pass.
#[derive(Debug)]
pub struct GateResult {
    pub violations: Vec<String>,
}

impl GateResult {
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Contract areas, mapped 1:1 to PR-template "Contract impact" boxes (+ gate self).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Area {
    CodexSchema,
    WitAbi,
    PinnedVersions,
    GateSelf,
    Secret,
    Observability,
    WorkspaceSafety,
    OrchestratorState,
}

impl Area {
    /// Hard areas are contract files: a hit almost always means contract impact,
    /// so they are blocking. Soft areas are behaviour crates (declaration only).
    fn is_hard(self) -> bool {
        matches!(
            self,
            Area::CodexSchema | Area::WitAbi | Area::PinnedVersions | Area::GateSelf
        )
    }

    /// Stable substring of the matching PR-template box (for `- [x]` detection).
    fn box_marker(self) -> &'static str {
        match self {
            Area::CodexSchema => "Codex app-server schema",
            Area::WitAbi => "WIT ABI",
            Area::PinnedVersions => "Pinned dependency versions",
            Area::GateSelf => "WIT ABI", // gate-self change is infra; reuse no box — see note
            Area::Secret => "Secret handling",
            Area::Observability => "Observability field names",
            Area::WorkspaceSafety => "Workspace path safety",
            Area::OrchestratorState => "Orchestrator state machine",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Area::CodexSchema => "Codex schema",
            Area::WitAbi => "WIT ABI",
            Area::PinnedVersions => "pinned versions (tools/versions.toml)",
            Area::GateSelf => "the PR gate itself",
            Area::Secret => "secret/redaction/log-field surface",
            Area::Observability => "observability field names",
            Area::WorkspaceSafety => "workspace path safety",
            Area::OrchestratorState => "orchestrator state machine",
        }
    }
}

fn starts_with_any(path: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| path.starts_with(p))
}

/// Classify all changed files into the set of contract areas they touch.
fn classify(changed: &[ChangedFile]) -> Vec<Area> {
    let mut areas = Vec::new();
    let mut push = |a: Area| {
        if !areas.contains(&a) {
            areas.push(a);
        }
    };
    for f in changed {
        let p = f.path.as_str();
        // hard: contract files
        if starts_with_any(p, &["wit/", "abi/expected/"]) {
            push(Area::WitAbi);
        }
        if starts_with_any(p, &["schemas/codex/"]) || p == "ci/expected/codex-schema.sha256" {
            push(Area::CodexSchema);
        }
        if p == "tools/versions.toml" {
            if f.changed_version_keys.iter().any(|k| k == "cli_version") {
                push(Area::CodexSchema);
            }
            if !f.changed_version_keys.is_empty() {
                push(Area::PinnedVersions);
            }
        }
        if p == ".github/workflows/pr-metadata.yml" || p.starts_with("crates/cadenza-cli/src/pr_gate")
        {
            push(Area::GateSelf);
        }
        // soft: behaviour crates (handled in C4)
        if p.starts_with("crates/cadenza-workspace/") {
            push(Area::WorkspaceSafety);
        }
        if p.starts_with("crates/cadenza-orchestrator/") {
            push(Area::OrchestratorState);
        }
        if p.starts_with("crates/cadenza-obs/") {
            push(Area::Observability);
        }
    }
    areas
}

/// True if the PR body checks the box whose text contains `marker`.
fn box_checked(body: &str, marker: &str) -> bool {
    body.lines().any(|l| {
        let t = l.trim_start();
        (t.starts_with("- [x]") || t.starts_with("- [X]")) && t.contains(marker)
    })
}

/// Count `Closes #<n>` closing references (case-insensitive keyword, digits).
fn count_closes(body: &str) -> usize {
    let lower = body.to_lowercase();
    let mut n = 0;
    let mut rest = lower.as_str();
    while let Some(i) = rest.find("closes #") {
        let after = &rest[i + "closes #".len()..];
        if after.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            n += 1;
        }
        rest = &rest[i + "closes #".len()..];
    }
    n
}

/// The pure gate. No IO — all git/env work happens in the caller.
pub fn evaluate(changed: &[ChangedFile], pr_body: &str) -> GateResult {
    let mut violations = Vec::new();

    // Rule 1: exactly one closing reference (one PR = one issue).
    let closes = count_closes(pr_body);
    if closes != 1 {
        violations.push(format!(
            "PR body must contain exactly one `Closes #<n>` (found {closes})"
        ));
    }

    let areas = classify(changed);

    for area in &areas {
        if area.is_hard() {
            // GateSelf has no dedicated PR box; treat it as needing an ADR only.
            if *area != Area::GateSelf && !box_checked(pr_body, area.box_marker()) {
                violations.push(format!(
                    "changed {} but the matching Contract-impact box is unchecked",
                    area.label()
                ));
            }
        }
    }

    GateResult { violations }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 无敏感改动 + 恰好一个 Closes → pass
    #[test]
    fn clean_pr_with_one_closes_passes() {
        let changed = [cf("README.md")];
        let r = evaluate(&changed, "Closes #12\n\nSome summary.");
        assert!(r.passed(), "{:?}", r.violations);
    }

    #[test]
    fn missing_closes_fails() {
        let r = evaluate(&[cf("README.md")], "no issue link here");
        assert!(!r.passed());
        assert!(r.violations.iter().any(|m| m.contains("Closes")));
    }

    #[test]
    fn two_closes_fails() {
        let r = evaluate(&[cf("README.md")], "Closes #1 and also Closes #2");
        assert!(!r.passed());
        assert!(r.violations.iter().any(|m| m.contains("exactly one")));
    }

    // 改了 wit/ 但没勾 WIT ABI box → fail
    #[test]
    fn wit_change_without_box_fails() {
        let r = evaluate(&[cf("wit/runtime.wit")], "Closes #1");
        assert!(r
            .violations
            .iter()
            .any(|m| m.contains("WIT ABI") && m.contains("unchecked")));
    }

    // 改了 wit/ 且勾了 box(且配 ADR)→ 该条不再报 box 未勾
    #[test]
    fn wit_change_with_box_and_adr_passes_box_rule() {
        let changed = [cf("wit/runtime.wit"), cf("decisions/0011-foo.md")];
        let body = "Closes #1\n- [x] WIT ABI (`wit/runtime.wit`, `abi/expected/*.wit`)";
        let r = evaluate(&changed, body);
        assert!(
            !r.violations.iter().any(|m| m.contains("unchecked")),
            "{:?}",
            r.violations
        );
    }

    // versions.toml 只改注释(无键右值变化)→ 不触发 pinned-version 硬门
    #[test]
    fn versions_comment_only_change_not_gated() {
        let changed = [ChangedFile {
            path: "tools/versions.toml".into(),
            changed_version_keys: vec![],
        }];
        let r = evaluate(&changed, "Closes #1");
        assert!(
            !r.violations.iter().any(|m| m.contains("Pinned")),
            "{:?}",
            r.violations
        );
    }

    // versions.toml 改了 cli_version 右值 → Codex schema 硬门 + 需勾 box
    #[test]
    fn versions_cli_version_change_requires_codex_box() {
        let changed = [ChangedFile {
            path: "tools/versions.toml".into(),
            changed_version_keys: vec!["cli_version".into()],
        }];
        let r = evaluate(&changed, "Closes #1");
        assert!(r
            .violations
            .iter()
            .any(|m| m.contains("Codex") && m.contains("unchecked")));
    }

    // 嵌套子路径也算命中(防绕过)
    #[test]
    fn nested_wit_path_still_classified() {
        let r = evaluate(&[cf("wit/deep/sub.wit")], "Closes #1");
        assert!(r.violations.iter().any(|m| m.contains("WIT ABI")));
    }

    // 测试辅助:构造一个无版本键变化的 ChangedFile
    fn cf(path: &str) -> ChangedFile {
        ChangedFile {
            path: path.to_string(),
            changed_version_keys: Vec::new(),
        }
    }
}
