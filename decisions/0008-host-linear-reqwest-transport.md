# ADR 0008: Production reqwest host-linear transport with abort-on-timeout

## Status

Accepted.

## Context

Follow-up to ADR 0007 (issue #77, PR #83), tracked as issue #84 (a P1 from the
@codex review on PR #83).

ADR 0007 bounds the wall-clock time of a synchronous `host-linear` transport
call: `StoreState::execute_within_deadline` runs `LinearTransport::execute` on a
detached worker thread and returns `host-error::timeout` if the deadline
elapses, and caps concurrent in-flight workers process-wide. Its **Known
limitations** record what it could not do — *cancel* the in-flight request:

> A timed-out call cannot be cancelled … after `timeout` the detached worker
> keeps running, so a `GraphqlMode::Write` mutation may still complete upstream
> … `host-linear` therefore offers **at-least-once** semantics for writes under
> timeout … True cancellation requires the transport to abort its own request
> (e.g. a `reqwest` client/connect timeout dropping the connection), which lands
> with the real HTTP transport.

That real transport did not exist: `cadenza-wasm-host` ships only a mock
`LinearTransport` (used in tests, returns immediately) and has no network
dependency. The `cadenza-tracker-linear` crate has an unrelated reqwest
transport, but it implements that crate's *own* async `LinearTransport` trait
for the `IssueTrackerClient` boundary — not the synchronous
`cadenza_wasm_host::LinearTransport` the `host-linear` capability consumes.

The host-side wall-clock bound (ADR 0007) is the non-cooperative authority that
frees the *host thread*; it cannot, on its own, stop the *upstream request*. The
only way to actually abort the request is for the transport to set its own
client deadline. `LinearCall::timeout` was already plumbed by PR #83 for exactly
this. This ADR adds the transport that consumes it.

### Blast radius / contract surface

- No frozen contract is touched. `host-error::timeout` / `rate-limited` /
  `upstream` / `io` are already variants of the frozen `host-error` in
  `wit/runtime.wit` (`cadenza:runtime@0.2.0`); surfacing them needs no WIT/ABI
  change. The Codex schema, the contract registry, and `tools/versions.toml` are
  untouched — `reqwest` is not a pinned contract dependency, and enabling its
  `blocking`/`http2` cargo features is not a version change.
- This **does** touch secret handling: the transport is the host-side injector
  of the operator's Linear API token (per ADR 0006). The token lives inside the
  transport, never in `LinearCall`, never crosses the WIT into guest memory, and
  is never logged. Per the repo's ADR policy a secret-handling change requires an
  ADR — this one.

## Decision

1. **A dedicated crate `cadenza-host-linear-http`.** The production transport
   pulls in `reqwest` (and transitively a TLS stack). `cadenza-wasm-host` is the
   lean Wasmtime boundary and deliberately has no network dependency; putting the
   egress path in its own crate keeps the boundary minimal and isolates the crate
   that holds the raw token and opens outbound sockets. It depends on
   `cadenza-wasm-host` only for the `LinearTransport` trait and its
   `LinearCall` / `LinearHttpResult` / `LinearTransportError` types. This mirrors
   the existing split where `cadenza-tracker-linear` owns its HTTP transport
   rather than embedding it in a core boundary.

2. **`reqwest::blocking`, not async.** `cadenza_wasm_host::LinearTransport::execute`
   is synchronous and runs on the detached worker thread (ADR 0007), which is a
   plain `std::thread` with no async runtime. `reqwest::blocking` is a self-
   contained synchronous client; using it avoids dragging a tokio runtime into
   the host for a single infrequent round-trip. (`reqwest::blocking` must not be
   built or called from within an async runtime; the worker thread satisfies
   this.)

3. **Per-request client timeout from `LinearCall::timeout` — the cancellation
   mechanism.** Each request sets `.timeout(call.timeout)` — the same deadline
   the host watchdog enforces. When it fires during the **send / await-response**
   phase (where a `GraphqlMode::Write` mutation actually reaches the server),
   reqwest stops the request and **drops/cancels it at the transport layer**: on
   HTTP/1.1 the connection is dropped; on HTTP/2 the stream is reset
   (`RST_STREAM`). The client stops sending/awaiting and the worker is reclaimed,
   so a timed-out write is no longer silently left in flight.

   Precise scope (do not overclaim): reqwest's *blocking* client applies
   `call.timeout` **per `read`** on the response body, not as one total budget.
   The send/await-response phase — the security-relevant one for write
   cancellation — is bounded by `call.timeout`. A *trickling* response body can
   hold the detached worker past `call.timeout`; that occupancy is bounded
   instead by `LinearCall::max_response_bytes` (total bytes read) and the
   process-wide in-flight worker ceiling of ADR 0007 Decision 5 (which fails
   closed with `host-error::rate-limited`). The endpoint is host-configured and
   allowlist-checked, not guest-chosen, so a trickle requires a compromised
   configured upstream and still degrades fail-closed — the same hung-worker
   posture ADR 0007's ceiling was designed for. A tighter total-deadline body
   read is possible future hardening but is not required for the write-
   cancellation guarantee this issue is about.

4. **HTTP/2 via ALPN, with HTTP/1.1 fallback.** The crate enables reqwest's
   `http2` cargo feature (the workspace pin sets `default-features = false`,
   which drops the otherwise-default `http2`). With `rustls-tls`, ALPN negotiates
   `h2` when the endpoint offers it and falls back to HTTP/1.1 otherwise. We do
   **not** force `http2_prior_knowledge` — that would hard-fail an endpoint that
   does not negotiate h2. HTTP/2 also gives the cleaner per-stream cancellation
   above.

5. **Honest cancellation contract (resolving ADR 0007's at-least-once, without
   overclaiming).** The client now *actively cancels* a timed-out request rather
   than leaking it. But neither a dropped connection nor an `RST_STREAM`
   *guarantees* the server rolls back a mutation it has already begun — that is
   upstream-dependent and not client-observable. So the contract is:

   - The transport aborts the request at the deadline and the host returns
     `host-error::timeout`; the worker is reclaimed promptly. This is the
     security improvement over ADR 0007 (no silently-leaked in-flight write).
   - Exactly-once for writes is **not** a client guarantee. Non-idempotent
     `GraphqlMode::Write` mutations that may be retried after a timeout MUST be
     made idempotent by the caller (e.g. an idempotency key / a conditional
     mutation), per AC2's "make the at-least-once contract explicit" branch.

   The `cadenza_wasm_host::LinearTransport` trait doc is updated to require that
   implementations set their own deadline from `LinearCall::timeout` and abort
   (not complete) on it.

6. **The transport bounds its own response read to `LinearCall::max_response_bytes`.**
   It reads the body through a capped reader and fails closed if the body exceeds
   the cap, so an oversized upstream response cannot force a large host
   allocation before the capability's backstop length check even runs (the same
   division of responsibility `LinearCall::max_response_bytes` already documents).

7. **Error mapping mirrors the existing capability expectations.** A completed
   HTTP exchange returns `LinearHttpResult { status, body_json }` (a 200 carrying
   a GraphQL `errors` array is still `Ok` here — the capability owns GraphQL-level
   interpretation). `429` maps to `LinearTransportError::RateLimited` with the
   parsed `retry-after`; other non-2xx maps to `Upstream`; a timeout or any other
   reqwest/IO failure maps to `Io`. The host watchdog (ADR 0007) remains the
   authority for `host-error::timeout`; the transport's own timeout error is
   primarily what reclaims the worker. Transport error strings never carry the
   token or the URL, and are scrubbed + capped by the capability layer before
   they cross to the guest regardless.

8. **Not wired into the orchestrator/CLI here.** This ADR ships the abort-capable
   transport and its contract; constructing it with the operator token and wiring
   it into a live `LinearCapability` is production-wiring work outside this issue
   (#84 is "resolve cancellation *before* the transport is wired in production").

## Consequences

- A new workspace crate `cadenza-host-linear-http` with `reqwest`
  (`blocking` + `http2`) as its only non-cadenza dependency. `Cargo.lock` gains
  the resolved graph (CI runs `--locked`, so the lock file is committed).
- Enabling reqwest's `blocking` / `http2` features unifies them across the
  workspace (cargo features are additive). `cadenza-tracker-linear`'s async usage
  is unaffected; its build simply also has those features available.
- `cadenza-wasm-host` gains no new dependency and no behavioural change beyond a
  doc-comment on the `LinearTransport` trait stating the abort-on-deadline
  contract. ADR 0007's "Known limitations" is amended to point here.
- `host-linear` writes under timeout move from "leaked in-flight request,
  at-least-once" to "request actively cancelled at the deadline, worker reclaimed
  promptly." Exactly-once is still not client-guaranteed; non-idempotent writes
  require caller-side idempotency, now documented rather than implicit.
