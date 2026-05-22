# AI Task Template

```text
You are working on Cadenza, a Rust + WebAssembly orchestration runtime for Symphony-style Codex workflows.

Read first:
- ARCHITECTURE.md
- SECURITY.md
- CONTRIBUTING_AI.md
- docs/research/02-implementation-readiness.md
- wit/runtime.wit

Current facts:
- SPEC baseline: <SPEC_SHA or TODO>
- Codex schema hash: <HASH or TODO>
- WIT package: bytevane:cadenza-runtime@0.1.x
- Runtime architecture: Rust host + Wasmtime extension layer

Task:
<Describe the narrow implementation task.>

Constraints:
- Do not invent Codex app-server protocol fields.
- Do not expose raw secrets to Wasm guest code.
- Do not relax workspace root containment.
- Do not modify WIT unless the task explicitly says so.
- Add or update tests.

Return:
- Summary
- Files changed
- Tests added
- Risks and follow-ups
```
