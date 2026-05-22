# Security policy

## Trust boundaries

- Tracker content is untrusted input.
- `WORKFLOW.md` is trusted configuration, but reloads must be validated before adoption.
- Shell hooks are trusted and high risk; prefer Wasm `safe-hook` extensions over time.
- Wasm components are untrusted extension code.
- Codex app-server is an external versioned dependency.

## Secrets

- Do not expose raw secrets to Wasm components.
- Do not log secrets, token-bearing headers, full environment dumps, or complete GitHub contexts.
- `secret-exists` may disclose only presence/absence.
- Host-mediated `linear-graphql` injects credentials on the host side only.

## Workspace safety

- All per-issue workspace paths must be derived from sanitized issue identifiers.
- All filesystem access must be checked against `workspace.root`.
- Symlink-aware checks must be added before any write-capable Wasm workspace API is introduced.

## Reporting vulnerabilities

Until a public security contact exists, report vulnerabilities to the repository owner privately.
