FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates nodejs npm jq \
    && rm -rf /var/lib/apt/lists/*

USER vscode

RUN rustup target add wasm32-wasip2 \
    && cargo install --locked wasm-tools || true

# Optional: install/pin Codex and Claude Code in a dedicated image layer after
# tools/versions.toml is finalized. Keep credentials out of the image.
