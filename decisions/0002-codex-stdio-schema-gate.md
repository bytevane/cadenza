# ADR 0002: Codex app-server over stdio with schema gate

## Status

Accepted.

## Decision

The MVP uses `codex app-server --listen stdio://` and treats generated Codex schema artifacts as versioned contracts.

## Rationale

The stdio transport is the default JSONL transport. WebSocket and Unix socket transports introduce additional authentication and exposure concerns. Schema artifacts are specific to the Codex version that generated them, so they must be committed and hashed.

## Consequences

- CI checks schema hash drift.
- Codex version upgrades happen in dedicated PRs.
- App-server protocol code stays inside `crates/cadenza-codex`.
