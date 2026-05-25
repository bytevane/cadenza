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
   snapshot reflects that the example now *implements* the frozen world.

   Reconciliation with `docs/operations/wit-abi-versioning.md`: that policy
   versions the **host package WIT** (`wit/runtime.wit` / `abi/expected/
   runtime.wit`). Those files are byte-identical in this PR — no interface,
   function, or variant case was added — so the package version stays
   `cadenza:runtime@0.2.0` and no minor/patch bump is due. The second
   snapshot, `cadenza-linear-graphql-plugin.world.wit`, is a *separate
   artifact*: the world extracted from the built example component. It changes
   from the empty `world root {}` to a world that imports a **subset** of the
   already-frozen host interfaces and exports `tool`. That is the example
   conforming to the existing contract, not a change to the contract — hence
   additive, no version bump, ADR-recorded.

3. **Link only the four in-scope interfaces; trap everything else.** The
   `Linker` defines real implementations for `host-log`, `host-time`,
   `host-workspace`, `host-secrets`. The example guest also pulls in
   incidental WASI imports via the Rust std runtime; rather than granting any
   WASI capability, every unknown import is stubbed as a trap
   (`Linker::define_unknown_imports_as_traps`) and the four host interfaces
   are then shadowed in with their real impls. The guest therefore gets no
   filesystem, env, clocks, random, or sockets — only the four host
   functions. `host-http` / `host-linear` / `host-tools` remain deferred to
   their own issues; a guest importing them would trap until then.

4. **Capability implementations (`cadenza-wasm-host`):**
   - `now-millis` reads an injectable `HostClock` (system by default; a fixed
     value in tests) so the function is deterministic under test.
   - `workspace-read` resolves the guest-supplied path with
     `cadenza_workspace::safe_join` against the per-issue workspace root, then
     `cadenza_workspace::resolve_inside` for symlink safety — which returns the
     canonical path so the host opens exactly the path it validated (no second
     canonicalisation that could diverge under a concurrent symlink swap).
     Containment escapes map to `host-error::outside-root`; missing files to
     `not-found`; other IO to `io`. The read seeks and reads only the
     `offset`/`limit` window (hard-capped at 4 MiB when no `limit` is given) so
     a small slice of a huge file never forces a whole-file host allocation;
     `truncated` is set when content remains past the window.
   - The captured host-call log sink is bounded (default 4096 records, with a
     dropped counter) so a guest looping cheap imports cannot grow host memory
     without bound before its epoch deadline.
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
- `cadenza-workspace` gains an additive `resolve_inside` helper (same
  containment semantics as `canonicalize_inside`, but returns the resolved
  path so callers open exactly what was validated). `canonicalize_inside` now
  delegates to it.
- The example plugin gains a `wit-bindgen` dependency (pinned to
  `wasm.wit_bindgen_version` in `tools/versions.toml`) and is no longer a
  placeholder.
- `abi/expected/cadenza-linear-graphql-plugin.world.wit` is regenerated in
  this PR; the WIT ABI gate stays green.
- Raw secrets remain host-side; the guest can only observe presence.
- Write access, HTTP, Linear GraphQL, and `host-tools` are still unbuilt and
  require their own ADRs/issues before landing.
