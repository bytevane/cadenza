# ADR 0003: Native Rust wasm32-wasip2 component build path

## Status

Accepted.

## Decision

Cadenza uses native Rust `wasm32-wasip2` builds for Wasm components.

## Rationale

The Rust and Component Model documentation now supports building WASI Preview 2 components with native Rust tooling. `cargo-component` may be useful for experiments, but is not a baseline dependency.

## Consequences

- `rustup target add wasm32-wasip2` is part of bootstrap.
- `wasm-tools component wit` is used for WIT/ABI inspection.
- Plugin crates should eventually use WIT-generated bindings.
