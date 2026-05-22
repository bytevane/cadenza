# ADR 0004: WIT and ABI governance

## Status

Accepted.

## Context

Cadenza's Wasm extensions rely on WIT worlds and host functions. If the WIT contract drifts, plugins may fail to load or behave incorrectly.

## Decision

Treat `wit/runtime.wit` as the authority. Commit `abi/expected/` snapshots and enforce ABI diff checks in CI using `wasm-tools`.

## Rationale

WIT is the boundary between host and guest. Keeping an explicit ABI snapshot makes breaking changes visible. During 0.x development, minor version changes should be treated as potentially breaking.

## Consequences

- WIT changes require PR explanation.
- Host rejects incompatible components.
- Plugin authors target explicit package/world versions.
- ABI compatibility tests become part of release readiness.
