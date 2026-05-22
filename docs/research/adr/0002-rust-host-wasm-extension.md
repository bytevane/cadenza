# ADR 0002: Rust host with Wasm extension layer

## Status

Accepted.

## Context

Symphony-style orchestration requires long-running scheduling, workspace management, process launching, hot reload, retry/reconcile, app-server client logic, and observability. These responsibilities are host-centric.

## Decision

Implement the control plane as a native Rust service. Use WebAssembly components for extension points, sandboxed tools, safe hooks, `linear_graphql`, audit exporters and future multi-language plugins.

## Rationale

Rust provides strong typing and mature async/process/filesystem support. Wasmtime and WASI Preview 2 provide a good sandbox and component model for untrusted or semi-trusted extensions. This division keeps correctness-critical orchestration in one single-authority host while still enabling extensibility.

## Consequences

- The orchestrator state machine remains native Rust.
- Wasm plugins cannot directly access secrets, arbitrary network or arbitrary filesystem paths.
- Host functions mediate all external capabilities.
- A future split execution plane can be considered after MVP.
