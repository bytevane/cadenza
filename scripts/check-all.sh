#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p cadenza-linear-graphql-plugin --target wasm32-wasip2 --release
./scripts/check-wit-abi.sh
