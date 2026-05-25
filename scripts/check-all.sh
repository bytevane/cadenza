#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked -p cadenza-linear-graphql-plugin --target wasm32-wasip2 --release
./scripts/check-wit-abi.sh
