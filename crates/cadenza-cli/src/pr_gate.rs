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

    let _ = changed; // classification lands in C2/C3/C4
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

    // 测试辅助:构造一个无版本键变化的 ChangedFile
    fn cf(path: &str) -> ChangedFile {
        ChangedFile {
            path: path.to_string(),
            changed_version_keys: Vec::new(),
        }
    }
}
