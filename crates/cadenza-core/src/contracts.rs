//! Contract registry helpers.
//!
//! `tools/versions.toml` is the single ledger pinning the upstream facts that
//! Cadenza targets. These helpers operate on a TOML body **as text** so the
//! checks do not depend on a TOML parser and can run from any crate.

/// MVP-critical keys that must be pinned to a concrete value (no `TODO`
/// placeholder) in `tools/versions.toml`. Adding a new MVP-critical contract
/// means appending here and adding its acceptance test.
pub const MVP_CRITICAL_KEYS: &[&str] = &[
    "symphony_spec_sha",
    "cli_version",
    "toolchain_version",
    "wasmtime_version",
    "wasm_tools_version",
    "wit_bindgen_version",
];

const TODO_MARKER: &str = "TODO";

/// True iff `line` (already trimmed of leading whitespace) is a TOML
/// assignment whose key is exactly `key`. A bare key followed by optional
/// whitespace and `=` qualifies; prefixed keys like `wasmtime_version_backup`
/// do not.
fn line_assigns_key(line: &str, key: &str) -> bool {
    let Some(rest) = line.strip_prefix(key) else {
        return false;
    };
    matches!(rest.bytes().next(), Some(b' ' | b'\t' | b'=')) && rest.trim_start().starts_with('=')
}

/// Return the value side of `line` with any inline `#` comment stripped.
/// Tracks TOML basic strings (`"..."`, with backslash escapes) and literal
/// strings (`'...'`, no escapes) so a `#` inside any string form stays part of
/// the value; only an unquoted `#` terminates it.
fn strip_inline_comment(line: &str) -> &str {
    enum State {
        Plain,
        Basic,
        Literal,
    }
    let mut state = State::Plain;
    let mut escaped = false;
    for (i, ch) in line.char_indices() {
        match state {
            State::Basic => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    state = State::Plain;
                }
            }
            State::Literal => {
                if ch == '\'' {
                    state = State::Plain;
                }
            }
            State::Plain => match ch {
                '"' => state = State::Basic,
                '\'' => state = State::Literal,
                '#' => return &line[..i],
                _ => {}
            },
        }
    }
    line
}

/// Return every line in `body` that assigns an MVP-critical key but still
/// carries a `TODO` placeholder in its value. `TODO` markers appearing only
/// inside an inline `#` comment do not count.
pub fn pending_mvp_critical_keys(body: &str) -> Vec<String> {
    let mut offenders = Vec::new();
    for raw in body.lines() {
        let line = raw.trim_start();
        if !strip_inline_comment(line).contains(TODO_MARKER) {
            continue;
        }
        for key in MVP_CRITICAL_KEYS {
            if line_assigns_key(line, key) {
                offenders.push(line.to_string());
                break;
            }
        }
    }
    offenders
}

/// Return the unquoted, comment-stripped value assigned to `key` in `body`, if any.
/// Reuses the registry's text-only parsing (no TOML crate). Used by drift checks.
pub fn assigned_value<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    for raw in body.lines() {
        let line = raw.trim_start();
        if line_assigns_key(line, key) {
            let value = strip_inline_comment(line);
            let after_eq = value.split_once('=')?.1.trim();
            let unquoted = after_eq
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| {
                    after_eq
                        .strip_prefix('\'')
                        .and_then(|s| s.strip_suffix('\''))
                })
                .unwrap_or(after_eq);
            return Some(unquoted);
        }
    }
    None
}

/// Return MVP-critical keys that are not assigned anywhere in `body`.
pub fn missing_mvp_critical_keys(body: &str) -> Vec<&'static str> {
    MVP_CRITICAL_KEYS
        .iter()
        .copied()
        .filter(|key| {
            !body
                .lines()
                .any(|line| line_assigns_key(line.trim_start(), key))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_PINNED: &str = r#"
symphony_spec_sha = "deadbeef"
cli_version = "rust-v0.133.0"
toolchain_version = "1.95.0"
wasmtime_version = "45.0.0"
wasm_tools_version = "1.250.0"
wit_bindgen_version = "0.57.1"
"#;

    #[test]
    fn exactly_n_keys_filled_yields_no_offenders_and_no_missing() {
        assert!(pending_mvp_critical_keys(ALL_PINNED).is_empty());
        assert!(missing_mvp_critical_keys(ALL_PINNED).is_empty());
    }

    #[test]
    fn n_plus_one_with_extra_unrelated_key_still_passes() {
        let body = format!("{ALL_PINNED}\nunrelated_key = \"TODO-something\"\n");
        assert!(pending_mvp_critical_keys(&body).is_empty());
        assert!(missing_mvp_critical_keys(&body).is_empty());
    }

    #[test]
    fn upper_edge_single_todo_returns_single_offender() {
        let body = ALL_PINNED.replace(
            r#"symphony_spec_sha = "deadbeef""#,
            r#"symphony_spec_sha = "TODO-pin""#,
        );
        let offenders = pending_mvp_critical_keys(&body);
        assert_eq!(offenders.len(), 1, "got: {offenders:?}");
        assert!(offenders[0].starts_with("symphony_spec_sha"));
    }

    #[test]
    fn lower_edge_single_missing_key_is_reported() {
        let body = ALL_PINNED.replace("wit_bindgen_version", "wit_bindgen_renamed");
        let missing = missing_mvp_critical_keys(&body);
        assert_eq!(missing, vec!["wit_bindgen_version"]);
    }

    #[test]
    fn todo_marker_in_comment_lines_is_ignored_when_key_not_at_line_start() {
        let body = format!("# TODO revisit later\n{ALL_PINNED}");
        assert!(pending_mvp_critical_keys(&body).is_empty());
    }

    #[test]
    fn suffixed_prefix_key_does_not_satisfy_presence() {
        // `wasmtime_version` removed, `wasmtime_version_backup` added — must
        // still report wasmtime_version as missing.
        let body = ALL_PINNED.replace(
            r#"wasmtime_version = "45.0.0""#,
            r#"wasmtime_version_backup = "45.0.0""#,
        );
        let missing = missing_mvp_critical_keys(&body);
        assert_eq!(missing, vec!["wasmtime_version"]);
    }

    #[test]
    fn suffixed_prefix_key_with_todo_is_not_an_offender() {
        let body = ALL_PINNED.replace(
            r#"wasmtime_version = "45.0.0""#,
            r#"wasmtime_version_backup = "TODO-foo"
wasmtime_version = "45.0.0""#,
        );
        assert!(
            pending_mvp_critical_keys(&body).is_empty(),
            "{:?}",
            pending_mvp_critical_keys(&body),
        );
    }

    #[test]
    fn inline_comment_todo_after_pinned_value_is_not_an_offender() {
        let body = ALL_PINNED.replace(
            r#"toolchain_version = "1.95.0""#,
            r#"toolchain_version = "1.95.0" # TODO revisit after 1.96 lands"#,
        );
        assert!(
            pending_mvp_critical_keys(&body).is_empty(),
            "{:?}",
            pending_mvp_critical_keys(&body),
        );
    }

    #[test]
    fn todo_inside_quoted_value_is_still_an_offender() {
        // The `#` here lives inside the string literal, so it is part of the
        // value and the TODO is real, not a comment.
        let body = ALL_PINNED.replace(
            r#"toolchain_version = "1.95.0""#,
            r#"toolchain_version = "TODO-#42 pin once upstream cuts""#,
        );
        let offenders = pending_mvp_critical_keys(&body);
        assert_eq!(offenders.len(), 1, "got: {offenders:?}");
        assert!(offenders[0].starts_with("toolchain_version"));
    }

    #[test]
    fn todo_inside_literal_single_quoted_value_is_still_an_offender() {
        // TOML literal strings use single quotes. `#` inside `'...'` is still
        // part of the value, not the start of a comment.
        let body = ALL_PINNED.replace(
            r#"toolchain_version = "1.95.0""#,
            "toolchain_version = 'TODO-#123 pin once upstream cuts'",
        );
        let offenders = pending_mvp_critical_keys(&body);
        assert_eq!(offenders.len(), 1, "got: {offenders:?}");
        assert!(offenders[0].starts_with("toolchain_version"));
    }

    #[test]
    fn literal_string_value_followed_by_todo_comment_is_not_an_offender() {
        let body = ALL_PINNED.replace(
            r#"toolchain_version = "1.95.0""#,
            "toolchain_version = '1.95.0' # TODO revisit when 1.96 lands",
        );
        assert!(
            pending_mvp_critical_keys(&body).is_empty(),
            "{:?}",
            pending_mvp_critical_keys(&body),
        );
    }

    #[test]
    fn assignment_without_space_around_equals_is_still_recognised() {
        let body = ALL_PINNED.replace(
            r#"wit_bindgen_version = "0.57.1""#,
            r#"wit_bindgen_version="0.57.1""#,
        );
        assert!(missing_mvp_critical_keys(&body).is_empty());
    }

    #[test]
    fn registry_text_has_no_pending_critical_keys() {
        let body = include_str!("../../../tools/versions.toml");
        let offenders = pending_mvp_critical_keys(body);
        assert!(
            offenders.is_empty(),
            "tools/versions.toml has unresolved MVP-critical TODOs: {offenders:?}",
        );
    }

    #[test]
    fn registry_text_documents_every_critical_key() {
        let body = include_str!("../../../tools/versions.toml");
        let missing = missing_mvp_critical_keys(body);
        assert!(
            missing.is_empty(),
            "tools/versions.toml is missing MVP-critical keys: {missing:?}",
        );
    }

    #[test]
    fn toolchain_channel_matches_pinned_version() {
        let versions = include_str!("../../../tools/versions.toml");
        let toolchain = include_str!("../../../rust-toolchain.toml");
        let pinned = assigned_value(versions, "toolchain_version")
            .expect("toolchain_version pinned in versions.toml");
        let channel =
            assigned_value(toolchain, "channel").expect("channel set in rust-toolchain.toml");
        assert_eq!(
            pinned, channel,
            "rust-toolchain.toml channel ({channel}) must match versions.toml toolchain_version ({pinned})"
        );
    }

    #[test]
    fn assigned_value_strips_quotes_and_comments() {
        assert_eq!(
            assigned_value("channel = \"1.95.0\"\n", "channel"),
            Some("1.95.0")
        );
        assert_eq!(
            assigned_value("channel = \"1.95.0\" # note\n", "channel"),
            Some("1.95.0")
        );
        assert_eq!(assigned_value("other = \"x\"\n", "channel"), None);
    }
}
