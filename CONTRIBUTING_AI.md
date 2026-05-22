# AI-assisted contribution policy

Cadenza allows Claude Code and Codex to generate implementation patches, but the project is contract-first.

## Required context for every AI coding task

Every generated patch must reference:

- `ARCHITECTURE.md`
- `docs/VERSIONING.md`
- `docs/WIT_ABI.md`
- `SECURITY.md`
- the relevant crate README or module docs

## Tool split

- Codex: protocol-adjacent code, app-server client code, replay fixtures, schema-bound tests.
- Claude Code: scaffolding, Rust module implementation, docs, CI templates, refactoring, test data.

## PR checklist for AI patches

- Did the patch change Codex app-server schema assumptions?
- Did the patch change WIT ABI?
- Did the patch change secret handling or logs?
- Did the patch add or update conformance tests?
- Does the patch keep the orchestrator as the single authoritative state owner?

## Prompt discipline

Use `prompts/codex-runtime.md` for Codex app-server work.
Use `prompts/claude-dev.md` for general implementation work.
Do not ask either tool to invent protocol fields not present in generated schemas.
