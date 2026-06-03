//! author-time PR gate: a pure `evaluate` over changed files + PR body, plus an
//! IO driver in `main.rs`. Earned by the aiops-platform port, whose contract
//! deviations shipped because checks were audit-time judgement, not author-time
//! mechanics. See CONTRIBUTING_AI.md "Anti-over-design principles".

use cadenza_core::contracts::MVP_CRITICAL_KEYS;

/// MVP-critical keys whose assigned value differs between `base` and `head`
/// versions of `tools/versions.toml`. Comment/format-only edits don't count.
pub fn changed_version_keys(base: &str, head: &str) -> Vec<String> {
    MVP_CRITICAL_KEYS
        .iter()
        .filter(|k| assigned_value(base, k) != assigned_value(head, k))
        .map(|k| (*k).to_string())
        .collect()
}

/// Value side of `key = <value>` with any inline `#` comment stripped, trimmed.
/// Mirrors the parsing style of cadenza-core's contract registry (text-only).
///
/// Assumes double-quoted values (the `tools/versions.toml` format): a `#` after
/// the closing quote terminates the value. A `#` inside a single-quoted literal
/// is not treated comment-aware here — cadenza-core's `strip_inline_comment` is
/// the full implementation.
fn assigned_value<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    for raw in body.lines() {
        let line = raw.trim_start();
        if let Some(rest) = line.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(val) = rest.strip_prefix('=') {
                // strip an inline comment that is outside of quotes (simple form:
                // versions.toml values are quoted, so a `#` after the closing
                // quote terminates the value).
                let val = val.trim();
                let cut = if let Some(stripped) = val.strip_prefix('"') {
                    stripped.find('"').map(|e| &val[..e + 2]).unwrap_or(val)
                } else {
                    val.split('#').next().unwrap_or(val).trim()
                };
                return Some(cut.trim());
            }
        }
    }
    None
}

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
    Observability,
    WorkspaceSafety,
    OrchestratorState,
    Secret,
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
            Area::GateSelf => "", // no PR box; gate-self is ADR-only
            Area::Observability => "Observability field names",
            Area::WorkspaceSafety => "Workspace path safety",
            Area::OrchestratorState => "Orchestrator state machine",
            Area::Secret => "Secret handling",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Area::CodexSchema => "Codex schema",
            Area::WitAbi => "WIT ABI",
            Area::PinnedVersions => "pinned versions (tools/versions.toml)",
            Area::GateSelf => "the PR gate itself",
            Area::Observability => "observability field names",
            Area::WorkspaceSafety => "workspace path safety",
            Area::OrchestratorState => "orchestrator state machine",
            Area::Secret => "secret/redaction/log-field surface",
        }
    }

    /// Token used in the `no <token> semantics change` declaration for soft areas.
    fn declaration_token(self) -> &'static str {
        match self {
            Area::Observability => "observability",
            Area::WorkspaceSafety => "workspace",
            Area::OrchestratorState => "orchestrator",
            Area::Secret => "secret",
            _ => "", // hard areas don't use declarations
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
        if p == ".github/workflows/pr-metadata.yml"
            || p.starts_with("crates/cadenza-cli/src/pr_gate")
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
        if p.starts_with("crates/cadenza-host-linear-http/") {
            push(Area::Secret);
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

/// True if `path` is an ADR under `decisions/`.
fn is_adr(path: &str) -> bool {
    path.starts_with("decisions/") && path.ends_with(".md")
}

/// True if `body` carries the `no <token> semantics change` declaration.
fn declares_no_change(body: &str, token: &str) -> bool {
    !token.is_empty()
        && body
            .to_lowercase()
            .contains(&format!("no {token} semantics change"))
}

/// Count `Closes #<n>` closing references (case-insensitive keyword, digits).
/// A left word boundary is required so substrings like `Encloses #1` or
/// `discloses #2` are not miscounted as closing references.
fn count_closes(body: &str) -> usize {
    let lower = body.to_lowercase();
    let mut n = 0;
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..].find("closes #") {
        let i = search_from + rel;
        // left word boundary: the char before "closes" must be a non-letter
        // (or "closes" must be at the very start of the body).
        let left_ok = lower[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphabetic());
        let after = &lower[i + "closes #".len()..];
        if left_ok && after.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            n += 1;
        }
        search_from = i + "closes #".len();
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
    let has_adr = changed.iter().any(|f| is_adr(&f.path));
    let touches_ledger = changed.iter().any(|f| f.path == "DEVIATIONS.md");
    // Accepted-deviation escape hatch (Rule 5): a DEVIATIONS.md row + an ADR is a
    // compliant way to land a contract-touching change without claiming "no impact".
    let accepted_deviation = touches_ledger && has_adr;

    for area in &areas {
        if area.is_hard() {
            // GateSelf has no dedicated PR box; treat it as needing an ADR only.
            if *area != Area::GateSelf
                && !box_checked(pr_body, area.box_marker())
                && !accepted_deviation
            {
                violations.push(format!(
                    "changed {} but the matching Contract-impact box is unchecked",
                    area.label()
                ));
            }
            if !has_adr {
                violations.push(format!(
                    "changed {} but no ADR under decisions/ is included",
                    area.label()
                ));
            }
        } else {
            let declared = box_checked(pr_body, area.box_marker())
                || declares_no_change(pr_body, area.declaration_token())
                || accepted_deviation;
            if !declared {
                violations.push(format!(
                    "touched {} — declare contract impact (check the box) or add `no {} semantics change`",
                    area.label(),
                    area.declaration_token()
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

    // `Encloses #1` 是子串误匹配,不应计为 closing reference;只数真正的 `Closes #5`
    #[test]
    fn encloses_is_not_a_closing_reference() {
        let r = evaluate(&[cf("README.md")], "Encloses #1 in the box\nCloses #5");
        assert!(r.passed(), "{:?}", r.violations);
    }

    // 改了 wit/ 但没勾 WIT ABI box → fail
    #[test]
    fn wit_change_without_box_fails() {
        let r = evaluate(&[cf("wit/runtime.wit")], "Closes #1");
        assert!(
            r.violations
                .iter()
                .any(|m| m.contains("WIT ABI") && m.contains("unchecked"))
        );
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
        assert!(
            r.violations
                .iter()
                .any(|m| m.contains("Codex") && m.contains("unchecked"))
        );
    }

    // 嵌套子路径也算命中(防绕过)
    #[test]
    fn nested_wit_path_still_classified() {
        let r = evaluate(&[cf("wit/deep/sub.wit")], "Closes #1");
        assert!(r.violations.iter().any(|m| m.contains("WIT ABI")));
    }

    // 改了 wit/ 勾了 box 但没 ADR → fail（缺 ADR）
    #[test]
    fn hard_path_without_adr_fails() {
        let body = "Closes #1\n- [x] WIT ABI";
        let r = evaluate(&[cf("wit/runtime.wit")], body);
        assert!(
            r.violations.iter().any(|m| m.contains("ADR")),
            "{:?}",
            r.violations
        );
    }

    // 改了 wit/ 勾 box 且配 ADR → pass
    #[test]
    fn hard_path_with_box_and_adr_passes() {
        let changed = [cf("wit/runtime.wit"), cf("decisions/0011-x.md")];
        let body = "Closes #1\n- [x] WIT ABI";
        let r = evaluate(&changed, body);
        assert!(r.passed(), "{:?}", r.violations);
    }

    // 接受偏离逃生口:改 orchestrator(软) + DEVIATIONS.md 行 + ADR,无需勾 box → pass
    #[test]
    fn accepted_deviation_via_ledger_and_adr_passes() {
        let changed = [
            cf("crates/cadenza-orchestrator/src/lib.rs"),
            cf("DEVIATIONS.md"),
            cf("decisions/0011-x.md"),
        ];
        let r = evaluate(&changed, "Closes #1");
        assert!(r.passed(), "{:?}", r.violations);
    }

    // 改 orchestrator(软)无任何声明 → fail，提示要勾 box 或写 no-semantics
    #[test]
    fn soft_path_without_declaration_fails() {
        let r = evaluate(
            &[cf("crates/cadenza-orchestrator/src/state.rs")],
            "Closes #1",
        );
        assert!(
            r.violations
                .iter()
                .any(|m| m.contains("orchestrator") && m.contains("declare")),
            "{:?}",
            r.violations
        );
    }

    // 改 orchestrator + 写 `no orchestrator semantics change` → pass
    #[test]
    fn soft_path_with_no_semantics_declaration_passes() {
        let body = "Closes #1\n\nno orchestrator semantics change";
        let r = evaluate(&[cf("crates/cadenza-orchestrator/src/state.rs")], body);
        assert!(r.passed(), "{:?}", r.violations);
    }

    // 改 host-linear-http(软,secret 注入边界)无任何声明 → fail，提示 secret + declare
    #[test]
    fn soft_secret_path_without_declaration_fails() {
        let r = evaluate(
            &[cf("crates/cadenza-host-linear-http/src/lib.rs")],
            "Closes #1",
        );
        assert!(
            r.violations
                .iter()
                .any(|m| m.contains("secret") && m.contains("declare")),
            "{:?}",
            r.violations
        );
    }

    // 改 host-linear-http + 写 `no secret semantics change` → pass
    #[test]
    fn soft_secret_path_with_no_semantics_declaration_passes() {
        let body = "Closes #1\n\nno secret semantics change";
        let r = evaluate(&[cf("crates/cadenza-host-linear-http/src/lib.rs")], body);
        assert!(r.passed(), "{:?}", r.violations);
    }

    // 改 workspace + 勾 box → pass(声明了影响)；注意软路径勾 box 也要按规则3配 ADR? 否：软路径不强制 ADR
    #[test]
    fn soft_path_with_box_checked_passes_without_adr() {
        let body = "Closes #1\n- [x] Workspace path safety / containment rules";
        let r = evaluate(&[cf("crates/cadenza-workspace/src/lib.rs")], body);
        assert!(r.passed(), "{:?}", r.violations);
    }

    // 反作弊:一段等同 PR 模板的空 body 不能满足门禁(否则照抄模板就过)。
    // 用内联 fixture(不读真模板),与 PR-E 改模板解耦。
    const TEMPLATE_EMPTY_BODY: &str = "\
## Linked issue
Closes #
## Contract impact
- [ ] Codex app-server schema or `codex.cli_version`
- [ ] WIT ABI (`wit/runtime.wit`, `abi/expected/*.wit`)
- [ ] Pinned dependency versions in `tools/versions.toml`
";

    #[test]
    fn empty_template_body_does_not_satisfy_gate() {
        // 模板的 `Closes #` 无数字 → 不算一个 closing ref → 规则1 fail
        let r = evaluate(&[cf("wit/runtime.wit")], TEMPLATE_EMPTY_BODY);
        assert!(!r.passed());
        assert!(r.violations.iter().any(|m| m.contains("Closes")));
    }

    // git mv 一个已有文件进敏感目录也算命中(分类只看路径,改名后的新路径命中)
    #[test]
    fn git_mv_into_wit_is_classified() {
        let r = evaluate(&[cf("wit/moved.wit")], "Closes #1");
        assert!(r.violations.iter().any(|m| m.contains("WIT ABI")));
    }

    // `Closes #` 占位符(无数字)不计数
    #[test]
    fn placeholder_closes_without_digit_is_zero() {
        let r = evaluate(&[cf("README.md")], "Closes #\nsummary");
        assert!(r.violations.iter().any(|m| m.contains("found 0")));
    }

    #[test]
    fn changed_version_keys_detects_value_change_only() {
        let base = "cli_version = \"rust-v0.133.0\"\ntoolchain_version = \"1.95.0\"\n";
        let head = "cli_version = \"rust-v0.134.0\"\ntoolchain_version = \"1.95.0\"\n";
        let keys = changed_version_keys(base, head);
        assert_eq!(keys, vec!["cli_version".to_string()]);
    }

    #[test]
    fn changed_version_keys_ignores_comment_only_edit() {
        let base = "cli_version = \"rust-v0.133.0\"\n";
        let head = "cli_version = \"rust-v0.133.0\" # bump soon\n";
        assert!(changed_version_keys(base, head).is_empty());
    }

    // 测试辅助:构造一个无版本键变化的 ChangedFile
    fn cf(path: &str) -> ChangedFile {
        ChangedFile {
            path: path.to_string(),
            changed_version_keys: Vec::new(),
        }
    }
}
