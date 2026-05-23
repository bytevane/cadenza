# Rollback drill (MVP)

This is the operator runbook for backing Cadenza out of a deployment
when a regression is suspected, and validating that rollback worked.
It is intentionally manual — MVP is single-host, single-operator;
production-grade automation lands later.

## Pre-flight: what rollback MUST preserve

Two things survive a rollback:

1. **Per-issue workspaces** under `<workspace.root>/<workspace-key>/`.
   These contain the agent's in-flight branch, uncommitted changes, and
   any artifacts the run produced. They are owned by the operator,
   not by Cadenza, and must never be discarded by a rollback step.
2. **Structured logs** (whatever destination the operator wired — local
   files, journald, Loki, etc.). The diagnostic trail is what makes
   the rollback decision auditable.

If a step in this drill would touch either of those, that step is
**wrong**. Cadenza state in memory is fine to discard — the
orchestrator reconciles from tracker + workspace on the next boot
(see `cadenza_orchestrator::reconcile_from_tracker`).

## Drill steps

### 1. Stop the running process

```bash
# If running via a supervised service:
systemctl stop cadenza

# If running by hand:
pkill -SIGTERM cadenza
# Wait for it to settle; SIGTERM lets in-flight Codex turns drain.
```

SIGTERM is enough. The Codex app-server subprocess + any spawned
hooks are killed via process-group cleanup (#12 / #11).

### 2. Verify workspaces are intact

```bash
ls -la <workspace.root>/
# Expect one directory per recently-active issue. Each must still
# have its `.git` directory and any uncommitted files.
```

If a workspace is missing or empty, **stop the rollback** and
investigate before re-deploying — the Cadenza process should never
remove a workspace directory.

### 3. Check out the previous Cadenza release

```bash
cd <cadenza-checkout>
git fetch --tags
git checkout v0.0.0   # or whichever tag is the prior good state
./scripts/bootstrap-dev.sh
cargo build --release
```

Tags are produced by the release process (see
`docs/operations/release-notes-template.md`). If no tag exists yet
(very early MVP), check out the prior commit hash recorded in your
deployment notes.

### 4. Verify compatibility before restart

```bash
./scripts/check-wit-abi.sh
./scripts/codex-schema.sh --check
./scripts/mvp-smoke.sh
```

All three MUST pass before bringing the rolled-back binary back up.
If the schema or ABI gate fails, you are rolling back across a
pinned contract change; consult `docs/operations/compatibility-matrix.md`
to identify the divergent row and decide whether to also roll the
upstream artifact (Codex CLI version, WIT package) back to match.

### 5. Restart with the same workflow

The MVP `cadenza-cli` binary currently exposes only operator helpers
(`doctor`, `workspace-key`, `workspace-path`). The supervised
long-running run loop is the operator's responsibility for now —
typical wiring is a systemd unit that invokes the operator's
preferred entrypoint into the orchestrator library:

```bash
# Sanity-check the workflow parses against the rolled-back binary
# before bringing the supervised process back up.
cargo run --release -p cadenza-cli -- doctor --workflow WORKFLOW.md

# Then start the supervised service that hosts cadenza_orchestrator's
# poll loop (operator-specific; example systemd unit shown).
systemctl start cadenza
```

A first-class `cadenza-cli run` subcommand will land in a future
issue alongside #18/#19 wiring; until then this step is intentionally
operator-side.

The orchestrator will reconcile from the tracker on boot. Per the
`reconcile_from_tracker` contract from #19:

- Issues in `active_states` whose workspaces still exist → resumed on
  the next poll tick.
- Issues now in `terminal_states` whose workspaces still exist →
  reported in the reconcile plan's `cleanup`; the operator decides
  whether to remove them.
- Issues with no local workspace → reported as `fresh`; the next
  poll tick treats them as new candidates.

### 6. Confirm the rollback succeeded

```bash
curl -s http://127.0.0.1:8080/api/v1/state | jq '.workflow_version, .running, .last_reload'
```

`workflow_version` should match the version reload outcome the new
binary reports. The number of running issues should converge to a
reasonable value within `poll.interval_ms * 2` after boot.

## Known limitations of the MVP

- Rollback is single-host. There is no multi-node coordination.
- The orchestrator has no durable persistence; in-flight Codex turns
  that were *running* at SIGTERM are lost (their workspace remains;
  they will be re-dispatched as continuations on the next poll tick
  per #19 lifecycle policy).
- The compatibility gates assume the operator stopped the process
  cleanly. A hard kill (`-SIGKILL`) may leave a half-written log file;
  no harm to workspaces.
- See `decisions/0009-orchestrator-state-and-recovery.md` for the
  detailed design rationale.

## What to do AFTER rollback

1. File the regression that triggered the rollback.
2. Reproduce it against the rolled-back binary (it should NOT
   reproduce; if it does, the bug is older than the rollback target).
3. Open a follow-up PR to fix forward.
4. Update `docs/operations/release-notes-template.md` for the next
   release with a "Known regressions in [version]" entry.
