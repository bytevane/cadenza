# Versioning and compatibility

## Compatibility tuple

Every production build should record this tuple:

```text
Cadenza host version
Symphony SPEC commit SHA
Codex CLI/app-server version
Codex schema SHA256
WIT package version
Wasmtime family version
Rust toolchain channel/version
```

## Rules

- Codex app-server schema drift is breaking until proven otherwise.
- WIT package minor changes are breaking before `1.0.0`.
- Wasmtime crates should remain in the same version family.
- `cargo-component` is not a baseline dependency; prefer native `wasm32-wasip2`.
- `latest` must not appear in production Dockerfiles or CI version ledgers.

## Upgrade process

1. Open a version-only PR.
2. Regenerate Codex schema artifacts.
3. Regenerate WIT/ABI snapshots if needed.
4. Run conformance tests and smoke tests.
5. Add a decision record explaining the upgrade.
