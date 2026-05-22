# Real integration smoke profile

Cadenza ships two smoke profiles. The default mock smoke (#22) runs in
every `cargo test --workspace` invocation and is the one CI enforces.
This document covers the **real integration smoke** (#23), which only
runs when an operator opts in by exporting credentials.

## What it covers

| Test | Talks to | Required env |
| --- | --- | --- |
| `real_codex_app_server_handshake` | local `codex app-server` subprocess (real signed-in account) | `CADENZA_REAL_SMOKE_CODEX=1` |
| `real_linear_read_returns_issue_set` | `https://api.linear.app/graphql` | `CADENZA_LINEAR_TOKEN`, `CADENZA_LINEAR_PROJECT_SLUG_ID` |

Both tests carry `#[ignore]` annotations so `cargo test --workspace`
**never** triggers them. They are only collected when `cargo test`
is invoked with `--ignored`.

When a required env var is unset, the test prints a single
`[real-smoke] SKIP: env var X is unset` line and returns OK. This is
deliberate per #23's acceptance criterion **"missing credentials
produce skip, not failure"** — the same binary is safe to invoke in
environments that do not have credentials.

## Running locally

```bash
# Codex only
export CADENZA_REAL_SMOKE_CODEX=1
./scripts/real-smoke.sh

# Linear only
export CADENZA_LINEAR_TOKEN=lin_api_xxx
export CADENZA_LINEAR_PROJECT_SLUG_ID=01234567-89ab-cdef-0123-456789abcdef
./scripts/real-smoke.sh

# Both
export CADENZA_REAL_SMOKE_CODEX=1
export CADENZA_LINEAR_TOKEN=lin_api_xxx
export CADENZA_LINEAR_PROJECT_SLUG_ID=01234567-89ab-cdef-0123-456789abcdef
./scripts/real-smoke.sh
```

`./scripts/real-smoke.sh` is a thin wrapper around `cargo test -p
cadenza-cli --test real_smoke -- --ignored --nocapture`. The
`--nocapture` flag lets the scrubbed status lines reach your terminal.

## Redaction

Every diagnostic line the real smoke prints is funnelled through
`cadenza_obs::Scrubber`. The scrubber is constructed per-test with:

- the Linear API token (for `real_linear_read_returns_issue_set`)
- the comma-separated values of `CADENZA_REAL_SMOKE_SECRETS` (for
  either test)

So an operator can register additional secret-shaped values they want
to keep out of CI logs, and the scrubber catches them in the test
output. This is layered on top of the standard
`looks_secret`-driven `KEY=VALUE` scrubber so a stray field with a
`*_TOKEN` / `*_KEY` / `secret` / `password` / `authorization` /
`cookie` key gets `[REDACTED]` regardless.

What this still cannot defend against:

- Bytes leaving the process before the scrubber sees them (kernel
  network capture, debugger).
- A secret that the operator did not register and whose key does not
  match the `looks_secret` predicate.

See `docs/operations/secret-redaction.md` for the full policy.

## CI

The real smoke is **not** wired into the default GitHub Actions CI.
That is by design: PR CI runs in a public/untrusted context and must
not see operator credentials. To enable it later in a protected
environment, add a separate job (`real-smoke`) that:

1. Is gated on `workflow_dispatch` only.
2. Loads the credentials from organisation-protected secrets.
3. Invokes `./scripts/real-smoke.sh`.

That follow-up is not required to close #23 because the acceptance
criteria explicitly says "CI can enable this profile in a protected
environment later" — i.e. the *capability* must exist, not the
deployment.

## Out of scope

- Real write paths (creating issues, updating states) — those land
  behind the `host-linear` Wasm capability (#17), which is currently
  blocked.
- Real production deployment.
