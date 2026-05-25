//! Example Wasm extension exercising the initial host capabilities (#16).
//!
//! This is the canonical `cadenza:runtime@0.2.0` example component. It is
//! intentionally a *demonstration* of the four in-scope host imports —
//! `host-log`, `host-time`, `host-workspace`, `host-secrets` — not the
//! eventual Linear GraphQL extension (that needs `host-linear`, which is a
//! non-goal for #16).
//!
//! `tool.run` reads a small JSON request out of `tool-input.args-json`,
//! performs the requested capability calls, and returns a JSON summary.
//! Host errors are propagated unchanged so callers observe the shared WIT
//! `host-error` model end to end.
//!
//! The component bindings only exist for the `wasm32` component target; on
//! the host (`cargo test --workspace`, `cargo check`) the crate compiles to
//! an empty library so the workspace build stays green without a wasm target.

#[cfg(target_arch = "wasm32")]
mod component {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "tool-runtime",
    });

    use cadenza::runtime::host_log::log;
    use cadenza::runtime::host_secrets::secret_exists;
    use cadenza::runtime::host_time::now_millis;
    use cadenza::runtime::host_workspace::workspace_read;
    use cadenza::runtime::types::{HostError, LogLevel, ToolInput, ToolOutput};
    use exports::cadenza::runtime::tool::Guest;

    struct Component;

    impl Guest for Component {
        fn run(input: ToolInput) -> Result<ToolOutput, HostError> {
            let args: serde_json::Value = if input.args_json.trim().is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::from_str(&input.args_json)
                    .map_err(|e| HostError::InvalidArgument(format!("args-json: {e}")))?
            };

            // host-time: always probe the clock.
            let now = now_millis();

            // host-log: emit a structured line when the caller supplies a
            // message. The host applies redaction; the guest never sees
            // secret values.
            let mut logged = false;
            if let Some(message) = args.get("message").and_then(|v| v.as_str()) {
                let fields = serde_json::json!({
                    "tool": input.name,
                    "now_millis": now,
                })
                .to_string();
                log(
                    LogLevel::Info,
                    Some("example-plugin"),
                    message,
                    Some(&fields),
                )?;
                logged = true;
            }

            // host-workspace: read a workspace-relative path when requested.
            // Containment + symlink safety are enforced host-side; an escape
            // surfaces as host-error::outside-root and is propagated here.
            let read = match args.get("read_path").and_then(|v| v.as_str()) {
                Some(path) => {
                    let offset = args.get("read_offset").and_then(|v| v.as_u64());
                    let limit = args.get("read_limit").and_then(|v| v.as_u64());
                    let as_text = args
                        .get("read_as_text")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    let result = workspace_read(path, offset, limit, as_text)?;
                    let text = String::from_utf8(result.bytes.clone()).ok();
                    Some(serde_json::json!({
                        "path": result.path,
                        "len": result.bytes.len(),
                        "truncated": result.truncated,
                        "text": text,
                    }))
                }
                None => None,
            };

            // host-secrets: presence-only probe. The guest can never read the
            // value — the WIT exposes no value-returning function.
            let secret_present = match args.get("secret_name").and_then(|v| v.as_str()) {
                Some(name) => Some(secret_exists(name)?),
                None => None,
            };

            let summary = serde_json::json!({
                "now_millis": now,
                "logged": logged,
                "read": read,
                "secret_present": secret_present,
            });

            Ok(ToolOutput {
                result_json: summary.to_string(),
                is_error: false,
            })
        }
    }

    export!(Component);
}
