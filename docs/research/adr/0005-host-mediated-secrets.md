# ADR 0005: Host-mediated secrets only

## Status

Accepted.

## Context

Cadenza handles credentials for OpenAI/Codex, Linear and possibly other external services. Wasm plugins and AI-generated code are not appropriate places to expose raw secrets.

## Decision

Secrets are held by the Rust host or deployment secret provider. Wasm components can only check existence or request host-mediated operations. No `get-secret` API will be exposed to Wasm guests.

## Rationale

Host-mediated credentials reduce leakage risk, provide a central audit point, and align with capability-based sandbox design. It also lets the host enforce allowlists, redaction and rate limits.

## Consequences

- `secret-exists` can return only boolean/status.
- `linear-graphql` uses host-injected auth.
- `http-request` cannot set sensitive headers unless host policy allows.
- All logs and outputs pass through a scrubber.
