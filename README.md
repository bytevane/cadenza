# Cadenza

**Cadenza** is a Rust + WebAssembly orchestration runtime for Symphony-style Codex workflows.

It implements a conservative version of the recommended architecture:

- Rust native host service owns orchestration, workspace safety, Codex app-server client integration, observability, and hot reload.
- WebAssembly components provide sandboxed extension points through WIT contracts.
- Codex app-server protocol artifacts are versioned and treated as generated external contracts.
- Linear is the first tracker target; tracker writes are intentionally kept out of the core orchestrator path.

## Status

This repository is an initial scaffold. It is intentionally contract-first:

1. Freeze upstream facts and decisions.
2. Define WIT and schema boundaries.
3. Build conformance tests before expanding feature code.
4. Allow Claude Code and Codex to generate implementation patches only against these contracts.

## Repository layout

```text
.
├── crates/
│   ├── cadenza-cli              # CLI entrypoint and local operator commands
│   ├── cadenza-core             # shared domain model
│   ├── cadenza-workflow         # WORKFLOW.md parsing and validation
│   ├── cadenza-workspace        # workspace key/path safety
│   ├── cadenza-orchestrator     # single-authority runtime state skeleton
│   ├── cadenza-codex            # Codex app-server client boundary
│   ├── cadenza-tracker-linear   # Linear tracker adapter boundary
│   ├── cadenza-wasm-host        # Wasmtime host runtime boundary
│   └── cadenza-obs              # log/metric field conventions
├── plugins/
│   └── cadenza-linear-graphql-plugin
├── wit/                         # authoritative WIT package/world definitions
├── schemas/codex/current/        # generated Codex app-server schema artifacts
├── abi/expected/                 # expected WIT/ABI snapshots
├── docs/                         # architecture, bootstrap, operations
├── decisions/                    # architecture decision records
├── prompts/                      # AI coding prompts for Claude Code and Codex
└── scripts/                      # bootstrap and verification scripts
```

## Quick start

```bash
./scripts/bootstrap-dev.sh
cargo test --workspace
cargo run -p cadenza-cli -- doctor --workflow WORKFLOW.example.md
cargo run -p cadenza-cli -- workspace-key ABC-123/foo
```

## End-to-end MVP smoke

A single mock-driven integration test wires the full Cadenza loop —
tracker candidate → workflow parse → orchestrator dispatch → mock
Codex event stream → lifecycle decision → observability snapshot — and
runs in `cargo test`. To run just the smoke:

```bash
./scripts/mvp-smoke.sh
```

The smoke is also wired into the `rust` CI job, so every PR exercises
the loop. Wasm host capability calls are intentionally skipped while
issues #16 / #17 are blocked.

Build the first Wasm example component:

```bash
rustup target add wasm32-wasip2
cargo build -p cadenza-linear-graphql-plugin --target wasm32-wasip2 --release
```

Validate WIT/ABI boundaries:

```bash
./scripts/check-wit-abi.sh
```

Generate Codex app-server schema artifacts after installing and authenticating `codex`:

```bash
./scripts/codex-schema.sh
```

## Design principles

- The orchestrator has one authoritative in-memory state owner.
- `WORKFLOW.md` is a repository contract, not a casual config file.
- Per-issue workspace paths are deterministic and must remain under `workspace.root`.
- Codex app-server schema and WIT package versions are separate compatibility gates.
- Wasm components do not receive raw secrets; host functions mediate credentials and outbound calls.
- CI must fail on unexpected schema or ABI drift.

## Current non-goals

- No distributed control plane in the initial implementation.
- No full web UI in the initial implementation.
- No direct Claude Code runtime integration into Cadenza production orchestration.
- No tracker write business logic inside the core orchestrator.

## Naming

Cadenza refers to a solo passage in a concerto. Here the Rust host is the conductor, Codex is the performer, and Wasm components are constrained soloists operating inside explicit capability boundaries.
