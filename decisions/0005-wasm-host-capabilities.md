# ADR 0005: Initial Wasm host capabilities

## Status

Accepted.

## Context

`wit/runtime.wit` (`cadenza:runtime@0.2.0`) freezes the host import surface
for Wasm extensions, but `cadenza-wasm-host` only loaded and resource-limited
components — no host import was actually linked, and the example plugin
(`cadenza-linear-graphql-plugin`) was a placeholder `cdylib` whose extracted
component world was empty (`world root {}`).

Issue #16 asks for the first *minimal capability-based* host API so a guest
can run end-to-end:

- `host-log.log`
- `host-time.now-millis`
- `host-workspace.workspace-read`
- `host-secrets.secret-exists`

Non-goals (#16): no outbound HTTP (`host-http`), no Linear GraphQL
(`host-linear`), no nested tool invocation (`host-tools`).

These functions are already declared in the frozen WIT; the work is to
*implement* them host-side, link them into a Wasmtime `Linker`, and make the
example plugin actually exercise them. Several frozen contracts are in the
blast radius:

- **WIT ABI (plugin component world snapshot).** Materializing the example
  plugin from a no-op into a real component changes
  `abi/expected/cadenza-linear-graphql-plugin.world.wit` from the empty
  `world root {}` to a world that imports the four host interfaces above and
  exports `tool`.
- **Secret handling.** `secret-exists` must disclose presence only.
- **Workspace path safety.** `workspace-read` must stay inside
  `workspace.root` via the `cadenza-workspace` containment APIs.
- **Observability.** Host calls must log with canonical `cadenza-obs` field
  names and apply redaction; raw secrets must never reach guest memory.

## Decision

1. **No host WIT change, no version bump.** `wit/runtime.wit` and
   `abi/expected/runtime.wit` are untouched; the package stays
   `cadenza:runtime@0.2.0`. This ADR implements the already-frozen surface.

2. **Plugin world snapshot is materialized (additive).** The example plugin
   now uses `wit-bindgen` to generate `tool-runtime` guest bindings and
   implements `tool.run`, which calls exactly the four in-scope host
   functions. The regenerated `cadenza-linear-graphql-plugin.world.wit`
   snapshot reflects that the example now *implements* the frozen world. This
   is a materialization of an existing contract, **not** a host-ABI break:
   the host package surface plugins compile against is unchanged. Per
   `docs/operations/wit-abi-versioning.md` this is recorded here as
   additive-only.

3. **Link only the four in-scope interfaces.** The `Linker` defines
   `host-log`, `host-time`, `host-workspace`, `host-secrets`. The example
   guest imports only those, so instantiation succeeds without `host-http` /
   `host-linear` / `host-tools`, which remain deferred to their own issues.

4. **Capability implementations (`cadenza-wasm-host`):**
   - `now-millis` reads an injectable `HostClock` (system by default; a fixed
     value in tests) so the function is deterministic under test.
   - `workspace-read` resolves the guest-supplied path with
     `cadenza_workspace::safe_join` against the per-issue workspace root, then
     `canonicalize_inside` for symlink safety. Containment escapes map to
     `host-error::outside-root`; missing files to `not-found`; other IO to
     `io`. `offset`/`limit` slice the bytes and set `truncated` when content
     remains past the window.
   - `secret-exists` answers from a host-side set of configured secret
     *names*; no value is ever readable through the WIT (structurally
     enforced — there is no value-returning function).
   - `log` scrubs the message and `fields-json` through
     `cadenza_obs::Scrubber` (key-shape + value-substring redaction) before
     emitting a `tracing` event.
   - Every host call emits a `tracing` event carrying `issue_id` /
     `plugin_name` / `component` via the `cadenza-obs` field-name constants,
     satisfying "all host calls include issue/plugin context in logs".

5. **Shared error model.** All fallible host functions return the WIT
   `host-error` variant; `WorkspaceError` is mapped onto it rather than
   surfacing a string.

## Consequences

- `cadenza-wasm-host` gains a real `Linker`/instantiate/`run_tool` path and a
  dependency on `cadenza-workspace` and `cadenza-obs`.
- The example plugin gains a `wit-bindgen` dependency (pinned to
  `wasm.wit_bindgen_version` in `tools/versions.toml`) and is no longer a
  placeholder.
- `abi/expected/cadenza-linear-graphql-plugin.world.wit` is regenerated in
  this PR; the WIT ABI gate stays green.
- Raw secrets remain host-side; the guest can only observe presence.
- Write access, HTTP, Linear GraphQL, and `host-tools` are still unbuilt and
  require their own ADRs/issues before landing.
