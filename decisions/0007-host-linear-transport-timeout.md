# ADR 0007: Bound the wall-clock time of a host-linear transport call

## Status

Accepted.

## Context

Follow-up to ADR 0006 (issue #17), tracked as issue #77 (a P1 from the
@codex review on PR #76).

`host-linear.linear-graphql` is implemented in `cadenza-wasm-host` by
`StoreState::dispatch_linear`, which validates the call and then runs the
configured transport **synchronously** inside the Wasmtime host function:

```rust
match cap.transport().execute(call) { /* ... */ }
```

The runtime already bounds *guest* execution with Wasmtime epoch
interruption (`epoch_timeout_ms`), but epoch interruption only preempts code
running *inside the guest*. While the host function is blocked in
`transport.execute`, the guest is not executing, so the epoch deadline never
fires. A slow or hung transport (slow network, unresponsive upstream) would
therefore block the host thread with **no wall-clock bound** — the exact gap
ADR 0006 recorded in its Consequences:

> The synchronous `LinearTransport::execute` call has no host-enforced
> wall-clock timeout in this PR … enforcing a host-side timeout (or requiring
> the transport to set its own client deadline) is tracked as a follow-up …

This is not reachable today: PR #76 ships only a mock transport that returns
immediately, and the real outbound HTTP transport is explicitly out of scope.
The hang vector appears once a real network transport lands, so the bound must
be in place *before* that transport is wired in production.

No frozen contract is in the blast radius. `host-error::timeout` is already a
variant of the frozen `host-error` in `wit/runtime.wit`
(`cadenza:runtime@0.2.0`), so surfacing a timeout needs **no** WIT/ABI change.
`WasmRuntimeLimits` is a plain `cadenza-wasm-host` type with no cross-crate
consumer and is not part of the contract registry, the Codex schema, or
`tools/versions.toml`; adding a field to it is not a pinned-contract change.

## Decision

1. **Host-enforced wall-clock deadline, independent of the transport.**
   `dispatch_linear` runs `transport.execute(call)` on a detached worker thread
   and waits on a bounded `mpsc::recv_timeout(deadline)`. If the transport does
   not return within the deadline, the host stops waiting and the call fails
   with `host-error::timeout` instead of blocking the host thread. The bound is
   enforced by the host and does **not** depend on the transport cooperating —
   a transport that ignores every deadline still cannot pin the host thread
   past `deadline`. The worker thread is intentionally detached: a hung
   transport leaks one thread (plus whatever it holds) until it returns on its
   own, but the host thread is freed immediately. The detached thread is
   eventually reclaimed by the transport's own client timeout (point 3).

2. **New configurable limit `WasmRuntimeLimits::linear_transport_timeout_ms`.**
   The existing `epoch_timeout_ms` (default 5_000) is the guest *CPU* budget; a
   single network round-trip has different, longer semantics, so reusing it
   would conflate two distinct bounds and starve a legitimately slow upstream.
   The new field defaults to `30_000` (30 s) and is `#[serde(default)]` so a
   config predating it still deserializes — mirroring the `max_table_elements`
   precedent (issue #74). It is plumbed onto `StoreState` at store-construction
   time, exactly as `http_body_limit` already is.

3. **Transport client timeout as defence-in-depth.** `LinearCall` gains a
   `timeout: Duration` field carrying the same host deadline, so the future
   `reqwest`-backed transport can set its own client/connect timeout from it.
   The host watchdog is the primary, non-cooperative bound; the transport's own
   timeout is a second layer that also reclaims the detached worker thread. The
   real transport itself is out of scope for this issue (a non-goal); only the
   channel it will consume is added here.

4. **Failure is audited like every other `host-linear` outcome.** A timeout
   records the standard audit entry (issue/plugin context, query fingerprint,
   duration, mode) with a generic `timeout: …` detail in the scrubbed `error`
   field. As with every other failure mode, the guest receives only the typed
   `host-error::timeout` variant — no transport internals cross the boundary,
   and the raw token never reaches the watchdog thread (it lives inside the
   transport, not in `LinearCall`).

5. **Bound concurrent in-flight workers (PR #83 review P1).** The detached-
   thread design means a hung transport leaves its worker running after the
   host has already returned `timeout`. To stop repeated timeouts under a hung
   upstream from accumulating unbounded threads (a host-level DoS), the number
   of concurrent in-flight workers is capped process-wide
   (`MAX_INFLIGHT_LINEAR_WORKERS`, 64); past the cap a call fails closed with
   `host-error::rate-limited` rather than spawning another worker. A worker
   releases its slot when it actually finishes (via an RAII guard it owns), so
   the cap bounds *live* workers, not call rate. It is process-wide (a `static`)
   because the threads it bounds are a process resource shared by every
   runtime/store in the host.

## Consequences

- `cadenza-wasm-host` gains `linear_transport_timeout_ms` on
  `WasmRuntimeLimits`, a `linear_transport_timeout: Duration` on `StoreState`,
  and a `timeout: Duration` on `LinearCall`. No other crate is affected; no
  frozen contract changes, so the WIT-ABI, Codex-schema, and contract-registry
  gates stay green without any snapshot edit.
- A `linear-graphql` call now costs one detached thread per invocation. Linear
  calls are infrequent and the thread is short-lived in the happy path; this is
  the minimal mechanism that bounds a *synchronous* blocking call without
  pulling in an async runtime.
- A transport that hangs leaks its worker thread until it returns, but at most
  `MAX_INFLIGHT_LINEAR_WORKERS` such workers can be live at once (Decision 5);
  past that, calls fail fast with `rate-limited`. With the real transport
  setting its own client timeout (point 3) each leaked worker is also
  eventually reclaimed; until that transport lands, only the in-tree mock
  (which returns immediately) is reachable, so no leak occurs in practice
  today.
- The real `reqwest`-backed transport, and wiring `LinearCall::timeout` into
  its client builder, remain part of the separate "real HTTP transport" work
  (ADR 0006 Consequences); this ADR ships the host-side bound and the channel
  that work will consume.

## Known limitations

- **A timed-out call cannot be cancelled (PR #83 review P1).** The host cannot
  interrupt a synchronous, blocking `transport.execute`; after `timeout` the
  detached worker keeps running, so a `GraphqlMode::Write` mutation may still
  complete upstream after the guest has observed a timeout (and possibly
  retried). `host-linear` therefore offers **at-least-once** semantics for
  writes under timeout, not exactly-once. True cancellation requires the
  transport to abort its own request (e.g. a `reqwest` client/connect timeout
  dropping the connection), which lands with the real HTTP transport — out of
  scope here per the issue's non-goals, and tracked as a follow-up. The
  in-flight ceiling (Decision 5) bounds how many such writes can be in flight
  at once but does not make a single one cancellable.
