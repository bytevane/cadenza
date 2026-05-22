# WIT ABI versioning policy

`wit/runtime.wit` is a versioned ABI contract. Two snapshots under
`abi/expected/` are checked on every PR by `scripts/check-wit-abi.sh`
(wired into the `ci/rust` job):

- `abi/expected/runtime.wit` — byte-identical copy of the source package
  WIT under `wit/`. Catches accidental edits to the package surface.
- `abi/expected/cadenza-linear-graphql-plugin.world.wit` — the world WIT
  extracted from the built example component plugin. Catches accidental
  changes to what the example plugin imports/exports as a component.

## Pre-1.0 rule

The WIT package is `cadenza:runtime@0.x.y`. Until we cut a `1.0` release,
**every minor version bump is breaking** unless the PR description and the
matching ADR explicitly call out the change as additive-only.

In practice that means:

- A typo fix or comment rewrite still bumps the patch and updates both the
  source and the snapshot.
- Adding a new interface, function, or variant case bumps the **minor**
  and is treated as breaking — downstream component crates must rebuild
  and re-snapshot in the same PR.
- Removing or renaming anything is always breaking and requires:
  1. ADR under `decisions/` describing the rationale.
  2. The PR body lists "ABI break" up front so reviewers know to look for
     downstream call sites and snapshot regenerations.
  3. Both snapshot files updated alongside `wit/runtime.wit` so the gate
     turns green.

This is stricter than semver for `0.x.y`; it matches the
"every commit is the new contract" baseline `CONTRACTS.md` codifies.

## How to intentionally update the snapshots

```bash
# 1. Edit wit/runtime.wit and/or the plugin source.
# 2. Refresh the source snapshot:
cp wit/runtime.wit abi/expected/runtime.wit

# 3. Rebuild and refresh the component world snapshot:
cargo build -p cadenza-linear-graphql-plugin --target wasm32-wasip2 --release
wasm-tools component wit \
  target/wasm32-wasip2/release/cadenza_linear_graphql_plugin.wasm \
  > abi/expected/cadenza-linear-graphql-plugin.world.wit

# 4. Re-run the gate locally:
./scripts/check-wit-abi.sh
```

The `cargo test -p cadenza-cli --test wit_abi` source-diff test must also
remain green; the script catches the component-world drift separately.

## Error message shape

Both snapshot mismatches print the form:

```
WIT mismatch: <which snapshot> drifted from <path>.
<actionable hint with the regen command>

<unified diff>
```

— so PR authors get the exact command to refresh the snapshot without
having to read this document first.
