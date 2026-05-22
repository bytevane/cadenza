# WIT and ABI governance

`wit/runtime.wit` is the authoritative extension contract.

## Design rules

- Host functions expose capabilities, not implementation details.
- Plugins never receive raw secrets.
- All outbound network access is host mediated.
- Workspace access starts read-only. Writes should be added only after a dedicated ADR.
- Every function returns a typed `host-error` variant when it can fail.

## CI rules

- `wasm-tools component wit ./wit -t` must succeed.
- `abi/expected/runtime.wit` must match `wit/runtime.wit` unless the PR explicitly changes ABI.
- Plugin components must declare the expected package/world before production loading is implemented.

## First extension targets

1. `linear-graphql`: execute one host-authorized Linear GraphQL operation.
2. `safe-hook`: replace high-risk shell hooks with capability-limited Wasm hooks.
3. `audit-exporter`: emit audit bundles without direct secret access.
