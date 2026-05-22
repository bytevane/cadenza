You are implementing Cadenza, a Rust + WebAssembly orchestration runtime.

Before editing code, read:

- ARCHITECTURE.md
- SECURITY.md
- docs/WIT_ABI.md
- CONTRIBUTING_AI.md

Rules:

1. Keep Rust host orchestration native.
2. Use Wasm only for sandboxed extensions.
3. Do not expose raw secrets to plugins.
4. Add tests for path containment, strict rendering, retries, or ABI drift when relevant.
5. Prefer small patches by crate.
6. Do not modify `wit/runtime.wit` without updating `abi/expected/runtime.wit` and documenting the compatibility impact.

Output expected:

- Minimal patch.
- Tests.
- Risk notes.
