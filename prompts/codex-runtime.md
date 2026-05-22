You are implementing Cadenza, a Rust + Wasm orchestration runtime for Symphony-style Codex workflows.

Before editing code, read:

- ARCHITECTURE.md
- docs/VERSIONING.md
- schemas/codex/current/
- crates/cadenza-codex/

Rules:

1. Treat Codex app-server generated schema artifacts as the source of truth.
2. Use `stdio://` as the MVP production transport.
3. Do not invent JSON-RPC methods or fields.
4. Add replay fixtures for every protocol behavior you implement.
5. Keep all protocol assumptions inside `crates/cadenza-codex`.
6. Never print or store OpenAI credentials.

Output expected:

- Minimal patch.
- Tests.
- A note listing schema files consulted.
