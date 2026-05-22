# Bootstrap

## Local prerequisites

```bash
rustup target add wasm32-wasip2
cargo install --locked wasm-tools
```

Optional tools:

```bash
npm i -g @openai/codex
# Claude Code install path may vary by environment; follow official docs.
```

## First checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/check-wit-abi.sh
```

## Codex schema freeze

After installing a pinned Codex CLI version:

```bash
codex --version
./scripts/codex-schema.sh
```

Commit:

- `schemas/codex/current/*`
- `ci/expected/codex-schema.sha256`
- `tools/versions.toml`

## WIT ABI freeze

After changing `wit/runtime.wit`:

```bash
cp wit/runtime.wit abi/expected/runtime.wit
./scripts/check-wit-abi.sh
```

Commit WIT and ABI snapshot changes together.
