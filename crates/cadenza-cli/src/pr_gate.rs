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

/// How a file changed relative to the base. Renames map to `Modified` against
/// the destination path (we classify on the new path, treating a rename as an
/// edit of where it landed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
}

/// A file changed in the PR, with just enough info to classify contract impact.
#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub path: String,
    /// Add / modify / delete relative to base. A deleted ADR must not satisfy the
    /// "ADR present" requirement, so `has_adr` filters on this.
    pub status: ChangeStatus,
    /// `tools/versions.toml` only: MVP-critical keys whose assigned value changed
    /// between base and head. Empty for every other file (and for versions.toml
    /// edits that only touched comments/formatting).
    pub changed_version_keys: Vec<String>,
    /// `DEVIATIONS.md` only: true iff this PR's diff *adds* a real deviation table
    /// row (a `| D<n> |` line), not just a whitespace/comment edit. The
    /// accepted-deviation escape hatch requires a genuinely new row, so a no-op
    /// ledger touch can't unlock the hard gate.
    pub added_deviation_row: bool,
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
        // cadenza-wasm-host wires the host-log / host-workspace / host-secrets
        // capabilities (capabilities.rs); changes there can alter guest-visible
        // logging/redaction/workspace/secret behaviour. Its most sensitive surface
        // is host-secrets, so it falls under the Secret soft gate.
        if p.starts_with("crates/cadenza-wasm-host/") {
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

/// True if `line` (a `DEVIATIONS.md` content line) is a real ledger row: a Markdown
/// table row whose first cell is a `D<digits>` id, e.g. `| D7 | ... |`. Used by the
/// IO driver to decide `added_deviation_row` from the added (`+`) lines of the diff.
/// The header (`| ID |`) and placeholder (`| _none yet_ |`) rows are not matched.
fn is_deviation_row(line: &str) -> bool {
    let t = line.trim();
    // shape: "| D<digits> |"
    let Some(rest) = t.strip_prefix("| D") else {
        return false;
    };
    let mut chars = rest.chars();
    // at least one digit must follow "| D"
    if !chars.next().is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    // and the row must have a second cell separator somewhere after
    t.contains("| D") && rest.contains('|')
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

    // Rule 1: at most one closing reference. cadenza branch-naming
    // (CONTRIBUTING_AI.md) allows issue-less docs/feat/fix/infra/chore PRs, so the
    // gate must not force an issue link — it only rejects stuffing multiple issues
    // into one PR (one PR = one issue).
    let closes = count_closes(pr_body);
    if closes > 1 {
        violations.push(format!(
            "PR body must contain at most one `Closes #<n>` (found {closes}); cadenza branch-naming allows issue-less docs/feat/fix/infra/chore PRs"
        ));
    }

    let areas = classify(changed);
    // A *deleted* ADR doesn't satisfy "ADR present" — otherwise removing the ADR
    // that justified a contract change would itself look compliant.
    let has_adr = changed
        .iter()
        .any(|f| is_adr(&f.path) && f.status != ChangeStatus::Deleted);
    // Accepted-deviation escape hatch (Rule 5): the PR must *add* a real
    // `DEVIATIONS.md` row (not just touch the file) AND carry an ADR. A whitespace
    // edit of the ledger no longer unlocks the hard gate.
    let adds_deviation_row = changed
        .iter()
        .any(|f| f.path == "DEVIATIONS.md" && f.added_deviation_row);
    let accepted_deviation = adds_deviation_row && has_adr;

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
            // Soft (behaviour-crate) paths. Three compliant outcomes:
            //   1. `no <area> semantics change` declaration → pure-internal refactor,
            //      no ADR required (the no-ADR path).
            //   2. box checked (author *claims* contract impact) → an ADR is then
            //      required, mirroring CONTRIBUTING_AI.md's "contract change ⇒ ADR".
            //   3. accepted-deviation escape hatch (new DEVIATIONS row + ADR).
            // Anything else is a violation.
            if declares_no_change(pr_body, area.declaration_token()) || accepted_deviation {
                // compliant, no ADR needed
            } else if box_checked(pr_body, area.box_marker()) {
                if !has_adr {
                    violations.push(format!(
                        "declared {} impact (checked the box) but no ADR under decisions/ is included",
                        area.label()
                    ));
                }
            } else {
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

/// IO driver: read the PR body from the GitHub event, diff against `base`, build
/// the `ChangedFile` set, and run the pure `evaluate`. Lives here (not in
/// `main.rs`) so the GateSelf hard path (`crates/cadenza-cli/src/pr_gate*`)
/// covers the gate's *driver* as well as its pure logic. Exits non-zero on
/// violation; fails closed if the event or diff can't be produced.
pub fn run(base: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::fs;
    use std::process::Command as Proc;

    // PR body from the GitHub event payload. Fail-closed if unreadable.
    let event_path = std::env::var("GITHUB_EVENT_PATH")
        .context("GITHUB_EVENT_PATH not set (gate must run in GitHub Actions)")?;
    let event: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&event_path).with_context(|| format!("read {event_path}"))?,
    )?;
    let pr_body = event
        .pointer("/pull_request/body")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Changed files vs base, with status. Fail-closed if the diff can't be produced.
    let out = Proc::new("git")
        .args(["diff", "--name-status", &format!("{base}...HEAD")])
        .output()
        .context("running git diff")?;
    if !out.status.success() {
        anyhow::bail!(
            "git diff against {base} failed (need fetch-depth: 0). Refusing to pass (fail-closed)."
        );
    }

    let changed: Vec<ChangedFile> = String::from_utf8(out.stdout)?
        .lines()
        .filter_map(parse_name_status_line)
        .map(|(status, path)| {
            let changed_version_keys = if path == "tools/versions.toml" {
                let base_toml = Proc::new("git")
                    .args(["show", &format!("{base}:tools/versions.toml")])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                    .unwrap_or_default();
                let head_toml = fs::read_to_string("tools/versions.toml").unwrap_or_default();
                changed_version_keys(&base_toml, &head_toml)
            } else {
                Vec::new()
            };
            let added_deviation_row = if path == "DEVIATIONS.md" {
                Proc::new("git")
                    .args(["diff", &format!("{base}...HEAD"), "--", "DEVIATIONS.md"])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                    .map(|diff| diff_adds_deviation_row(&diff))
                    .unwrap_or(false)
            } else {
                false
            };
            ChangedFile {
                path,
                status,
                changed_version_keys,
                added_deviation_row,
            }
        })
        .collect();

    let result = evaluate(&changed, pr_body);
    if result.passed() {
        println!("pr-gate: ok ({} files checked)", changed.len());
        Ok(())
    } else {
        for v in &result.violations {
            eprintln!("pr-gate: {v}");
        }
        std::process::exit(1);
    }
}

/// Parse one `git diff --name-status` line into `(status, path)`. The status is a
/// single letter (`A`/`M`/`D`) or a rename/copy token like `R100` followed by the
/// old and new paths (tab-separated). Renames/copies map to `Modified` against the
/// destination (new) path. Returns `None` for unparseable lines.
fn parse_name_status_line(line: &str) -> Option<(ChangeStatus, String)> {
    let mut cols = line.split('\t');
    let code = cols.next()?;
    let first = code.chars().next()?;
    match first {
        'A' => Some((ChangeStatus::Added, cols.next()?.to_string())),
        'D' => Some((ChangeStatus::Deleted, cols.next()?.to_string())),
        'M' | 'T' => Some((ChangeStatus::Modified, cols.next()?.to_string())),
        // rename/copy: "R100\told\tnew" — classify on the destination path.
        'R' | 'C' => {
            let _old = cols.next()?;
            Some((ChangeStatus::Modified, cols.next()?.to_string()))
        }
        _ => None,
    }
}

/// True if a unified `git diff` of `DEVIATIONS.md` *adds* at least one real ledger
/// row (an added line, `+`-prefixed but not the `+++` file header, whose content is
/// a `| D<n> |` row).
fn diff_adds_deviation_row(diff: &str) -> bool {
    diff.lines().any(|l| {
        l.starts_with('+') && !l.starts_with("+++") && is_deviation_row(l.trim_start_matches('+'))
    })
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

    // 无 Closes(0 个)在 issue-less docs/feat/fix/infra/chore PR 中是合法的
    #[test]
    fn missing_closes_is_allowed() {
        let r = evaluate(&[cf("README.md")], "no issue link here");
        assert!(r.passed(), "{:?}", r.violations);
    }

    #[test]
    fn two_closes_fails() {
        let r = evaluate(&[cf("README.md")], "Closes #1 and also Closes #2");
        assert!(!r.passed());
        assert!(r.violations.iter().any(|m| m.contains("at most one")));
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
            status: ChangeStatus::Modified,
            changed_version_keys: vec![],
            added_deviation_row: false,
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
            status: ChangeStatus::Modified,
            changed_version_keys: vec!["cli_version".into()],
            added_deviation_row: false,
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

    // 接受偏离逃生口:改 orchestrator(软) + DEVIATIONS.md 新增真 D 行 + ADR,无需勾 box → pass
    #[test]
    fn accepted_deviation_via_ledger_and_adr_passes() {
        let changed = [
            cf("crates/cadenza-orchestrator/src/lib.rs"),
            cf_deviation_added(),
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
    // Fix 5: 软路径勾 box(声明有 contract 影响)但无 ADR → fail
    #[test]
    fn soft_path_with_box_but_no_adr_fails() {
        let body = "Closes #1\n- [x] Workspace path safety / containment rules";
        let r = evaluate(&[cf("crates/cadenza-workspace/src/lib.rs")], body);
        assert!(
            r.violations
                .iter()
                .any(|m| m.contains("workspace") && m.contains("ADR")),
            "{:?}",
            r.violations
        );
    }

    // Fix 5: 软路径勾 box + ADR → pass
    #[test]
    fn soft_path_with_box_and_adr_passes() {
        let body = "Closes #1\n- [x] Workspace path safety / containment rules";
        let changed = [
            cf("crates/cadenza-workspace/src/lib.rs"),
            cf("decisions/0012-x.md"),
        ];
        let r = evaluate(&changed, body);
        assert!(r.passed(), "{:?}", r.violations);
    }

    // Fix 5: 软路径 no-semantics 声明(无 ADR)→ pass(纯内部重构是 no-ADR 路径)
    #[test]
    fn soft_path_no_semantics_passes_without_adr() {
        let body = "Closes #1\n\nno workspace semantics change";
        let r = evaluate(&[cf("crates/cadenza-workspace/src/lib.rs")], body);
        assert!(r.passed(), "{:?}", r.violations);
    }

    // Fix 4: hard path + 勾 box + 只删了一个 ADR(status Deleted)→ 仍 fail(缺 ADR)
    #[test]
    fn deleting_an_adr_does_not_satisfy_adr_requirement() {
        let body = "Closes #1\n- [x] WIT ABI";
        let changed = [cf("wit/runtime.wit"), cf_deleted("decisions/0011-x.md")];
        let r = evaluate(&changed, body);
        assert!(
            r.violations.iter().any(|m| m.contains("ADR")),
            "{:?}",
            r.violations
        );
    }

    // Fix 2: hard path + box 未勾 + DEVIATIONS.md 只改空白(无新行)+ ADR → 仍 fail(box 未勾)
    #[test]
    fn whitespace_ledger_edit_does_not_unlock_hard_gate() {
        let changed = [
            cf("wit/runtime.wit"),
            cf_deviation_whitespace(),
            cf("decisions/0011-x.md"),
        ];
        let r = evaluate(&changed, "Closes #1");
        assert!(
            r.violations.iter().any(|m| m.contains("unchecked")),
            "{:?}",
            r.violations
        );
    }

    // Fix 2: hard path + box 未勾 + DEVIATIONS.md 新增真 D 行 + ADR → pass(逃生口生效)
    #[test]
    fn new_ledger_row_unlocks_hard_gate() {
        let changed = [
            cf("wit/runtime.wit"),
            cf_deviation_added(),
            cf("decisions/0011-x.md"),
        ];
        let r = evaluate(&changed, "Closes #1");
        assert!(r.passed(), "{:?}", r.violations);
    }

    // Fix 3: 改 cadenza-wasm-host(capabilities.rs)无声明 → fail,提示 secret + declare
    #[test]
    fn wasm_host_change_without_declaration_fails() {
        let r = evaluate(
            &[cf("crates/cadenza-wasm-host/src/capabilities.rs")],
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

    // Fix 3: 改 cadenza-wasm-host + 写 `no secret semantics change` → pass
    #[test]
    fn wasm_host_change_with_no_semantics_declaration_passes() {
        let body = "Closes #1\n\nno secret semantics change";
        let r = evaluate(&[cf("crates/cadenza-wasm-host/src/capabilities.rs")], body);
        assert!(r.passed(), "{:?}", r.violations);
    }

    // is_deviation_row: 真 D 行匹配,header/placeholder/template 注释行不匹配
    #[test]
    fn is_deviation_row_matches_only_real_rows() {
        assert!(is_deviation_row("| D1 | what | ref | P0 | Open | #1 |"));
        assert!(is_deviation_row("| D42 | x |"));
        assert!(!is_deviation_row("| ID | Area | ... |"));
        assert!(!is_deviation_row("| _none yet_ | | | | | |"));
        assert!(!is_deviation_row("| Dx | not a number |"));
        assert!(!is_deviation_row("some prose"));
    }

    // diff_adds_deviation_row: 仅当某新增行(+,非 +++)是真 D 行才 true
    #[test]
    fn diff_adds_deviation_row_detects_added_row() {
        let diff = "\
--- a/DEVIATIONS.md
+++ b/DEVIATIONS.md
@@ -50,1 +50,2 @@
 | _none yet_ | | | | | |
+| D1 | a real new deviation | wit sig | P1 | Open | #99 |
";
        assert!(diff_adds_deviation_row(diff));
    }

    #[test]
    fn diff_adds_deviation_row_ignores_whitespace_only() {
        let diff = "\
--- a/DEVIATIONS.md
+++ b/DEVIATIONS.md
@@ -10,1 +10,1 @@
-Prefer locally verifiable anchors, in this order:
+Prefer locally verifiable anchors, in this order:
";
        assert!(!diff_adds_deviation_row(diff));
    }

    // parse_name_status_line: A/M/D/R 解析,rename 用目标路径当 Modified
    #[test]
    fn parse_name_status_line_handles_all_codes() {
        assert_eq!(
            parse_name_status_line("A\tnewfile.rs"),
            Some((ChangeStatus::Added, "newfile.rs".to_string()))
        );
        assert_eq!(
            parse_name_status_line("M\tedited.rs"),
            Some((ChangeStatus::Modified, "edited.rs".to_string()))
        );
        assert_eq!(
            parse_name_status_line("D\tdecisions/0011-x.md"),
            Some((ChangeStatus::Deleted, "decisions/0011-x.md".to_string()))
        );
        assert_eq!(
            parse_name_status_line("R100\told/path.rs\tnew/path.rs"),
            Some((ChangeStatus::Modified, "new/path.rs".to_string()))
        );
        assert_eq!(parse_name_status_line(""), None);
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
        // 照抄空模板(WIT ABI box 是 `- [ ]` 未勾、无 ADR)改了 wit/ 仍不能过门禁:
        // 0-closes 已不再是 violation,反作弊改由硬路径未勾 box 把守。
        let r = evaluate(&[cf("wit/runtime.wit")], TEMPLATE_EMPTY_BODY);
        assert!(!r.passed());
        assert!(
            r.violations
                .iter()
                .any(|m| m.contains("WIT ABI") && m.contains("unchecked")),
            "{:?}",
            r.violations
        );
    }

    // 回归(#96):门禁要能放过它自己的无-issue 治理 PR。
    // #96 改了 pr_gate.rs + pr-metadata.yml(GateSelf 硬路径),body 无 Closes,
    // 并带 decisions/0011-*.md(ADR)。GateSelf 硬路径只要求 ADR(无 PR box),
    // 0-closes 在 at-most-one 规则下合法 → 整体 pass。
    #[test]
    fn gate_passes_on_its_own_issueless_pr() {
        let changed = [
            cf(".github/workflows/pr-metadata.yml"),
            cf("crates/cadenza-cli/src/pr_gate.rs"),
            ChangedFile {
                path: "decisions/0011-author-time-pr-gate.md".to_string(),
                status: ChangeStatus::Added,
                changed_version_keys: Vec::new(),
                added_deviation_row: false,
            },
        ];
        let r = evaluate(&changed, "n/a feat branch, depends on #94");
        assert!(r.passed(), "{:?}", r.violations);
    }

    // git mv 一个已有文件进敏感目录也算命中(分类只看路径,改名后的新路径命中)
    #[test]
    fn git_mv_into_wit_is_classified() {
        let r = evaluate(&[cf("wit/moved.wit")], "Closes #1");
        assert!(r.violations.iter().any(|m| m.contains("WIT ABI")));
    }

    // `Closes #` 占位符(无数字)计为 0 个有效 closing ref;0 个在无敏感路径的
    // PR 上是合法的(规则1 = at most one),所以 evaluate 通过。
    #[test]
    fn placeholder_closes_without_digit_is_zero() {
        assert_eq!(count_closes("Closes #\nsummary"), 0);
        let r = evaluate(&[cf("README.md")], "Closes #\nsummary");
        assert!(r.passed(), "{:?}", r.violations);
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

    // 测试辅助:构造一个无版本键变化的 ChangedFile(默认 Modified)
    fn cf(path: &str) -> ChangedFile {
        ChangedFile {
            path: path.to_string(),
            status: ChangeStatus::Modified,
            changed_version_keys: Vec::new(),
            added_deviation_row: false,
        }
    }

    // 测试辅助:构造一个被删除的 ChangedFile
    fn cf_deleted(path: &str) -> ChangedFile {
        ChangedFile {
            path: path.to_string(),
            status: ChangeStatus::Deleted,
            changed_version_keys: Vec::new(),
            added_deviation_row: false,
        }
    }

    // 测试辅助:构造一个新增了真 deviation 行的 DEVIATIONS.md
    fn cf_deviation_added() -> ChangedFile {
        ChangedFile {
            path: "DEVIATIONS.md".to_string(),
            status: ChangeStatus::Modified,
            changed_version_keys: Vec::new(),
            added_deviation_row: true,
        }
    }

    // 测试辅助:构造一个只改空白/无新行的 DEVIATIONS.md
    fn cf_deviation_whitespace() -> ChangedFile {
        ChangedFile {
            path: "DEVIATIONS.md".to_string(),
            status: ChangeStatus::Modified,
            changed_version_keys: Vec::new(),
            added_deviation_row: false,
        }
    }
}
