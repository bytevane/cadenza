# ADR 0001: Rust host with Wasm extension layer

## Status

Accepted.

## Decision

Cadenza will implement the core Symphony-style orchestrator as a native Rust host service and use WebAssembly components for sandboxed extensions.

## Rationale

The core service must own polling, workspaces, subprocesses, hot reload, retries, and reconciliation. These responsibilities are host-level concerns. Wasm is better suited to controlled extension points with explicit capabilities.

## Consequences

- Wasmtime is embedded in the host.
- WIT defines plugin contracts.
- Shell hooks remain a transitional compatibility feature.
- The orchestrator does not become a pure Wasm component in MVP.
