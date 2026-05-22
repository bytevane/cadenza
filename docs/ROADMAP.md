# MVP roadmap

## Week 1: freeze boundaries

- Pin `SPEC.md` commit.
- Pin Codex version and generate schema artifacts.
- Freeze `wit/runtime.wit` v0.1.0.
- Commit `CONTRIBUTING_AI.md` and PR template.

## Week 2: workflow and workspace

- Implement `WORKFLOW.md` loader.
- Add strict prompt rendering.
- Add workspace root containment tests.
- Add hot-reload shell.

## Week 3: Codex client

- Implement app-server process launcher over stdio.
- Add initialize/initialized handshake tests.
- Add JSONL replay fixtures.

## Week 4: tracker and orchestrator

- Implement Linear read adapter.
- Implement single-authority dispatch state.
- Add retry/backoff/reconcile tests.

## Week 5: Wasm extension runtime

- Implement Wasmtime host runtime.
- Add resource limits and epoch timeouts.
- Add `linear-graphql` example component.

## Week 6-8: hardening

- Add observability API.
- Add real integration smoke profile.
- Add secret scrubber.
- Add rollback and deployment runbooks.
