# ADR 0006: Host-mediated linear-graphql Wasm capability

## Status

Accepted.

## Context

`wit/runtime.wit` (`cadenza:runtime@0.2.0`) already freezes the
`host-linear.linear-graphql` import:

```wit
linear-graphql: func(operation-name: option<string>, query: string,
                     variables-json: string, mode: graphql-mode)
    -> result<graphql-response, host-error>;
```

ADR 0005 implemented and linked only `host-log`, `host-time`,
`host-workspace`, `host-secrets`; `host-http`, `host-linear`, and
`host-tools` were explicitly deferred to their own issues. Issue #17 is the
`host-linear` follow-up: let a Wasm tool run a controlled Linear GraphQL
operation **without ever receiving raw credentials**.

The function is already in the frozen WIT, so — like #16 — the work is to
*implement* it host-side, link it into the Wasmtime `Linker`, and make the
example plugin exercise it. Several frozen contracts sit in the blast radius:

- **WIT ABI (plugin component world snapshot).** The example plugin now calls
  `linear-graphql`, so the world extracted from the built component gains
  `import cadenza:runtime/host-linear@0.2.0;`. The host package WIT
  (`wit/runtime.wit` / `abi/expected/runtime.wit`) is byte-identical.
- **Secret handling.** Linear auth must be injected host-side; the raw token
  must never reach guest memory, logs, or error messages crossing back to the
  guest.
- **Observability.** The operation must be audited with canonical
  `cadenza-obs` field names and redaction.

## Decision

1. **No host WIT change, no version bump.** `wit/runtime.wit` and
   `abi/expected/runtime.wit` are untouched; the package stays
   `cadenza:runtime@0.2.0`. This ADR implements an already-frozen surface,
   exactly as ADR 0005 did. Per `docs/operations/wit-abi-versioning.md`, the
   policy versions the host package WIT — those files are unchanged, so no
   minor/patch bump is due.

2. **Plugin world snapshot is materialized further (additive).** The example
   plugin gains a `linear-graphql` call path, so the regenerated
   `abi/expected/cadenza-linear-graphql-plugin.world.wit` adds a single import
   (`cadenza:runtime/host-linear@0.2.0`) of an **already-frozen** host
   interface. That is the example conforming to the existing contract, not a
   change to the contract — additive, ADR-recorded, no version bump (same
   reasoning as ADR 0005 §2).

3. **Auth is injected behind a host-side transport; the capability layer
   never touches the raw token.** `cadenza-wasm-host` defines a
   `LinearTransport` trait and a `LinearCapability` bundle
   (host-configured endpoint + endpoint allowlist + `Arc<dyn LinearTransport>`)
   carried on `HostCapabilities`. The capability layer validates the endpoint
   and arguments, then hands the operation to the transport, which is the
   **sole** injector of the `Authorization` header host-side. The token lives
   in the transport (host memory), never in `RequestContext`, never in a WIT
   value, never copied into guest memory. The transport is injectable so tests
   drive a mock without standing up a server (mirrors the `HostClock` and
   `cadenza-tracker-linear::LinearTransport` patterns).

4. **Plugin-supplied `Authorization` headers are structurally impossible.**
   The frozen `linear-graphql` signature gives the guest **no header channel**
   — only `operation-name`, `query`, `variables-json`, `mode`. `query` and
   `variables-json` are the GraphQL POST body, not transport headers, so a
   guest cannot supply or override auth. The host transport is the only source
   of headers. This satisfies the issue's "deny plugin-supplied auth header"
   criterion by construction (the policy is *ignore*: there is no channel).

5. **Endpoint allowlist enforced at the capability boundary.** The
   host-configured endpoint must be a member of the capability's allowlist
   (default: `https://api.linear.app/graphql`). A misconfigured endpoint
   denies the guest call with `host-error::denied` and is still audited.

6. **Fail closed when unconfigured.** `host-linear` is linked unconditionally,
   but if `HostCapabilities.linear` is `None` a `linear-graphql` call returns
   `host-error::denied("linear capability not configured")`. A guest that does
   not import `host-linear` is unaffected.

7. **Typed error mapping with redaction across the boundary.** Transport
   failures map to the shared WIT `host-error`:
   rate-limit → `rate-limited(option<u32>)`, upstream → `upstream(string)`,
   IO/network → `io(string)`. Upstream/IO message strings are scrubbed through
   the `cadenza-obs::Scrubber` **before** the `host-error` crosses into guest
   memory, so a token echoed by an upstream error cannot leak to the guest. A
   GraphQL operation that completes at the HTTP layer (including a 200 carrying
   a GraphQL `errors` array) is returned as `graphql-response` (status +
   body-json); only HTTP/transport-level failures become `host-error`.

8. **Audit with canonical, redacted fields.** Every `linear-graphql` call —
   success, denial, or error — records one host-call log entry stamped with
   issue/plugin context and a structured `fields-json` keyed by new
   `cadenza-obs` field-name constants: `operation_name`, `query_fingerprint`,
   `duration_ms`, `graphql_mode`, and (on failure) a scrubbed `error`. The
   raw query is **not** logged; instead a non-cryptographic FNV-1a-64
   fingerprint correlates identical operations without exposing query text.
   `operation_name` and the error string are scrubbed before recording.

## Consequences

- `cadenza-wasm-host` gains `LinearCapability` / `LinearTransport` /
  `LinearMode` types, a `host-linear` `Host` impl, and links the fifth host
  interface into the `Linker`. `host-http` and `host-tools` remain deferred.
- `cadenza-obs` gains five additive field-name constants for the Linear audit
  log; the field-name contract is extended, not changed (existing constants
  are untouched).
- `abi/expected/cadenza-linear-graphql-plugin.world.wit` is regenerated to add
  the `host-linear` import; the WIT ABI gate stays green.
- The example plugin gains a `linear_query` request path demonstrating an
  allowed mock operation end to end.
- Raw Linear credentials remain host-side; the guest observes only the GraphQL
  response body or a redacted typed error.
- Real outbound HTTP for the transport (a `reqwest`-backed impl wiring the
  operator's token) is left to the integration/orchestrator layer; this PR
  ships the host boundary, the policy, and a mock transport for tests.
