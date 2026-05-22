# ADR 0003: Codex app-server schema is a version gate

## Status

Accepted.

## Context

Cadenza integrates with Codex app-server. The app-server protocol shape must be determined by the target Codex version, not guessed from memory or from the Symphony SPEC alone.

## Decision

Pin the Codex CLI/app-server version, generate schema artifacts, commit them, and enforce a schema hash gate in CI.

## Rationale

Codex app-server is an external protocol dependency. The cost of schema drift is high: handshake failure, event parsing failure, or incorrect tool invocation. A generated schema and hash make protocol drift explicit and reviewable.

## Consequences

- `schemas/codex/current/` stores generated artifacts.
- `ci/expected/codex-schema.sha256` stores the expected hash.
- Codex upgrades require a dedicated PR.
- `cadenza-codex` should centralize protocol mapping.
