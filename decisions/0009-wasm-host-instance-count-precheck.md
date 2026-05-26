# ADR 0009: Pre-count component core instances with `wasmparser` to classify instance-count breaches as `LimitBreached`

## Status

Accepted.

## Context

Closes issue #86 (follow-up to #82, which fixed the table/memory sub-cases of
the same defect; both originally surfaced from the issue #75 audit of host
error classification).

`cadenza-wasm-host` classifies guest failures into a small typed
`WasmHostError` so the orchestrator can branch on the cause: `LimitBreached`
means "the guest tripped a host cap"; `Link` means "the host's wiring is
broken". Before #82, every component instantiation that exceeded a wasmtime
resource cap landed in `Link` because wasmtime's `Store::bump_resource_counts`
bails with a stringly-typed error:

```text
resource limit exceeded: {desc} count too high at {n}
```

— neither a typed payload nor a `wasmtime::Trap`, so `classify_instantiate`
had no signal to downcast and fell through to `Link`.

#82 fixed the **table** and **memory** sub-cases by pre-checking
`wasmtime::component::Component::resources_required()` host-side, before
`instantiate`, and surfacing an over-cap as `LimitBreached` directly. That
left the **component instance** sub-case as the residual `Link` case: wasmtime
45's public `ResourcesRequired` carries `num_tables` / `num_memories` only —
there is no `num_component_instances` field. The count wasmtime actually
checks is derived from `env_component().initializers` /
`num_runtime_component_instances`, both private.

We confirmed before adopting this ADR that this gap is not closed by simply
bumping wasmtime: as of May 2026 the latest published `wasmtime` (45.0.0 stable;
46.0.0 in upstream `main`) still exposes neither a `num_component_instances`
field on `ResourcesRequired` nor a typed/`Trap`-shaped error from
`bump_resource_counts` (still `bail!("resource limit exceeded: ...")`). So the
upstream-only path from #86's option 1 is not available without a wasmtime PR
+ release cycle.

## Decision

Pre-count the component's static core module instantiations host-side by
walking the component binary with `wasmparser`, then extend the existing
`check_declared_resource_counts` pre-check (#82) with an instance branch that
compares the counted value against the per-store `ResourceLimiter::instances`
cap and surfaces an over-cap as `WasmHostError::LimitBreached` with a typed
message — never an error-message substring (issue #86 AC2). This keeps all
three classifiable count caps (tables, memories, component instances) flowing
through one cohesive pre-check; wasmtime's own `bump_resource_counts` remains
the fail-closed backstop for any case the pre-check cannot derive statically.

### Implementation

- New `wasmparser` workspace dependency, pinned to `0.244` so it shares
  wasmtime 45's transitive copy (`Cargo.lock` already lists `wasmparser
  0.244.0`).
- New `cadenza_wasm_host::capabilities::count_component_core_instances(&[u8])`
  walks the component with a `Parser` stack: every `Payload::CoreInstanceSection`
  contributes its entry count; `Payload::ModuleSection` /
  `Payload::ComponentSection` push the sub-parser so nested components are
  drained too. `Payload::End` pops back to the parent. This is the same
  pattern wasmtime/wit-tools use for component traversal.
- The walk runs once at `ComponentRuntime::load` time; the result is stashed
  on `LoadedComponent::core_instance_count` so per-call `run_tool`
  classification stays cheap (no re-parse).
- A walker failure (bytes wasmtime just compiled cleanly but wasmparser
  rejects) maps to `WasmHostError::Compile`, matching how other structural
  rejections surface. It does **not** mask the bytes as "0 instances".
- `check_declared_resource_counts` gains the `core_instance_count` and
  `instance_cap` parameters and checks the instance branch first (it does not
  depend on `Component::resources_required` returning `Some`).
- Doc comments on `classify_instantiate` and `RuntimeLimiter` drop the
  "instance count … left as Link" caveat from #82.

### Why not bump wasmtime

Option 1 from issue #86 ("a wasmtime release that exposes either a downcastable
count-limit error type or a public component instance count") is not yet
available: as of May 2026 the latest published `wasmtime` (45.0.0; 46.0.0 in
upstream main) still bails with the stringly-typed message and still ships
`ResourcesRequired` without a `num_component_instances` field. A wasmtime
version bump alone would not satisfy issue #86 AC2 (typed signal, not a
substring) and the registry rules in `CONTRACTS.md` require any
`tools/versions.toml` change to be a dedicated PR + ADR with no bundled
feature work. Filing the upstream PR and consuming a future release remains a
clean long-term path; this ADR closes #86 without blocking on it.

### Why not parse the WIT/component type structure via wasmtime

`Component::component_type()` exposes imports/exports for the world contract
but does not expose internal initializers (the field wasmtime itself derives
its instance count from). `wasmparser` is the public, stable boundary that
sees the same sections wasmtime sees and that the bytecode-alliance maintains
as the canonical inspector for component binaries.

## Consequences

- A component instantiating more core module instances than `max_instances`
  now surfaces as `WasmHostError::LimitBreached` with a message of the form
  `component declares N core instances, exceeding the host cap of M`,
  matching the existing table/memory phrasing (#82). The orchestrator can
  branch on `LimitBreached` uniformly without parsing wasmtime strings.
- The classification uses a typed `u32` derived from `wasmparser` rather than
  any error-message substring — satisfies issue #86 AC2.
- `RuntimeLimiter::denied_growth` and `grow_failed_after_allow` still do not
  move for count-cap denials. The pre-check is the only host signal; the
  type-level contract on `RuntimeLimiter` (updated here) calls this out so a
  future reader does not expect the counters to move.
- The wasmparser walk runs once at load time, not per invocation. Cost is
  linear in component byte size and dominated by `Component::new` already
  paid on the same path.

### Known limitations

- `count_component_core_instances` counts *static* declarations. A workflow
  that instantiates the same nested component repeatedly would under-count;
  wasmtime's own `bump_resource_counts` then remains the fail-closed
  backstop and surfaces as the residual `Link` branch of
  `classify_instantiate`. No cadenza plugin uses that shape today, and
  upgrading the pre-check to track dynamic instantiations would re-implement
  the very wasmtime-internal counting #86 set out to *avoid* approximating.
  If a future plugin pattern needs it, the right move is the upstream wasmtime
  bump described above, not a richer walker here.
- A wasmtime upgrade that later exposes a typed `num_component_instances` on
  `ResourcesRequired` (or a downcastable count-limit error) supersedes this
  ADR — the walker can be retired in favour of that field with a follow-up
  ADR. Until then this ADR is the source of truth for why a `wasmparser`
  dependency lives in `cadenza-wasm-host`.

## References

- Issue #86 — wasm-host: classify instance count-cap breaches as `LimitBreached`
- Issue #82 — the table/memory siblings, fixed via the same pre-check
- Issue #75 — original classification audit motivating the typed-signal rule
- `CONTRACTS.md` — pinned-contract change procedure (this ADR adds a
  workspace dependency but does not move any pinned key in
  `tools/versions.toml`)
- `tools/versions.toml` — `wasm.wasmtime_version` left at `45.0.0`
