# Batch issue processing

How an agent should run a batch of issues (a `/goal` over a set of tracker
issues) so the work stays reviewable, parallel where it can be, and safe to
merge.

cadenza is a **contract-first** Rust + WebAssembly runtime: WIT ABI, the Codex
app-server schema, the contract registry, and secret / workspace / orchestrator
/ observability discipline are frozen gates that fail loudly in CI. The rules
below adapt the "one issue → one PR" batch workflow to those gates. Pair with
the `handle-issue` and `handle-pr` skills, which drive each single issue and PR.

## The unit of work: one issue → one PR

1. **One issue per branch, one branch per PR.** Never bundle multiple issues
   into a single PR. Small blast radius is what makes a batch reviewable: CI
   failures localize to one issue, the human can merge at their own cadence,
   and a revert touches exactly one concern. This is `CLAUDE.md`'s "One PR =
   one issue" discipline applied across the batch.
2. **Each PR is self-contained and self-verifying.** It includes the fix plus a
   failing-first regression test, and the test must fail when the production
   code is broken (mutation or negative assertion). A PR whose tests would pass
   with the fix reverted does not count as done. Never skip tests via
   `#[ignore]`, `--no-verify`, or `if false`.
3. **Run the local gate before pushing**, then confirm CI green and run the
   adversarial pass on the head commit. The local gate is
   `./scripts/check-all.sh` (`cargo fmt --check` + `clippy -D warnings` + `test
   --workspace` + wasm build + `check-wit-abi.sh`); add `./scripts/codex-schema.sh
   --check` when the change touches the Codex schema. Resolve or explicitly
   defer every finding before marking ready.

## Parallelize independent issues by default

Sequential execution is the single biggest throughput cost when most issues in
a set have no ordering dependency.

1. **Map dependencies before starting.** For the issue set, sketch which issues
   touch overlapping paths or have a real ordering constraint (one's fix
   depends on another's merge). Issues touching disjoint paths are independent
   — but "disjoint" means more than different files: two issues can collide
   through the **same crate**, **shared `cadenza-core` types**, an atomic
   rename, or `tools/versions.toml` / `Cargo.lock`. Two issues both touching a
   frozen contract (WIT, Codex schema, the registry) are not independent — each
   needs its own dedicated PR + ADR and they must not race the same snapshot.
   Treat shared crate/module surface as a dependency, not as independent.
2. **Default to parallel for independent issues.** Open a branch per
   independent issue and progress them concurrently rather than draining them
   in series. Serialize **only** where a dependency is real — a shared crate, a
   shared `cadenza-core` type, or an API a later issue consumes.
3. **Cap concurrency to what you can keep green.** Each in-flight PR still owes
   the full per-PR gate (local `check-all.sh`, CI green, adversarial review
   triage). The orchestrator's `max_concurrent_agents` cap models a hard
   ceiling; your review bandwidth is the practical one. Do not open more
   parallel work than you can drive to mergeable without letting reviews rot.
4. **Respect cadenza's patch discipline per PR regardless of parallelism.**
   Default to one crate or one doc surface per PR; cross-crate patches need a
   justification in the PR description (`CLAUDE.md`). Contract-touching changes
   are dedicated PRs carrying their own ADR — never bundled with feature work,
   never bundled with a pinned-version bump in `tools/versions.toml`.
   Parallelism is about more small PRs, never bigger ones.

## Surface deferrals at the moment you defer, not at the end

A deferral carried silently and only disclosed in the final summary robs the
human of the chance to weigh in while the context is fresh.

1. **The instant you decide an acceptance criterion cannot be met in this PR,
   say so** — in the PR body and to the human — with the reason and the
   proposed follow-up.
2. **File the follow-up issue immediately**, categorized by which contract or
   surface it touches (WIT ABI / Codex schema / registry / secret / workspace /
   orchestrator / obs, or plain feature/tech-debt), and link it from the PR. A
   deferral without a tracked issue is just a silent gap.
3. **Never let "done" hide a deferral.** The end-of-batch summary should
   restate deferrals already raised, not introduce them.

## Keep a live status checklist

Mid-batch, the only way the human can reconstruct state is by reading the PR
list. Maintain a running checklist instead and refresh it on every meaningful
transition:

```text
issue → branch → PR → state (draft | CI green | review clean | mergeable | merged | deferred→#NNN)
```

## Merge and goal-clear are human actions by default

The safe default leaves every merge to the human, and it is intentional:

1. **The agent drives each PR to *mergeable*, then stops.** It does not merge,
   it does not force-push `main`, it does not edit the pinned versions in
   `tools/versions.toml`, and it does not edit a contract snapshot
   (`abi/expected/*`, `schemas/codex/`, `ci/expected/*.sha256`) to paper over a
   gate failure. These are durable guardrails, not per-session reminders.
2. **The batch's terminal state depends on human actions** — merging the PRs
   and clearing the goal. Plan for PRs to queue waiting for a human; that is
   expected, not a stall.

## Authorized auto-merge flow (opt-in, scope-bounded)

The default above (human merges) stands unless the human grants explicit,
scope-bounded authorization for the agent to merge. Authorization is per-batch
and per-scope — "you may merge the PRs for issues #X–#Y once they pass the
gate" — never a standing grant. Approving one merge does not authorize the next.

When auto-merge **is** authorized, a PR may be merged only when **all** of
these hold:

1. **CI is green** on the head commit (not a stale run).
2. **Review is clean** — no unresolved HIGH/MEDIUM findings from the subagent /
   Codex audits (and, if the repo has the `@codex` GitHub bot, from it too) —
   and **zero unresolved, non-outdated review threads**.
3. **Every acceptance criterion is met, or each gap is deferred to a tracked,
   linked issue** (per the deferral protocol above).
4. **The PR respects cadenza's patch discipline.** One crate or one doc surface
   by default; any cross-crate change is justified in the description. A change
   touching a frozen contract (WIT ABI, Codex schema, contract registry) or
   secret / workspace / orchestrator-state / obs-field semantics carries its
   paired ADR under `decisions/` and is a dedicated PR; no pinned-version bump
   in `tools/versions.toml` is bundled in.
5. **Branch protection's required reviews are satisfied.**
6. **The merge uses the agreed method** (default: squash) into the agreed base.

**Native auto-merge enforces only required status checks (CI) and branch
protection's required reviews — not gates 2–4.** `gh pr merge --auto` merges the
instant required CI passes, regardless of an open review finding, an unresolved
review thread, an unmet acceptance criterion, or a contract/scope breach. So:

- Confirm gates 2–4 **yourself, immediately before** enabling auto-merge.
- A new commit re-opens them — any push restarts the review round, so
  re-confirm before re-enabling.
- Prefer native auto-merge over an immediate merge only to win the CI race, not
  as a substitute for checking the policy gates.

Hard stops — **always require human sign-off even under an auto-merge grant:**

- Force-pushing or merging into `main` out of band, or any history rewrite.
- Editing the pinned versions in `tools/versions.toml`, or editing a contract
  snapshot (`abi/expected/*`, `schemas/codex/`, `ci/expected/*.sha256`) to make
  a gate pass without a paired ADR.
- A PR that breached its scope (cross-crate without justification, unrelated
  refactors, bundled contract change) — flag, don't merge.
- Anything the human's instructions for this batch put off-limits. When a PR
  sits at the edge of the grant, use `AskUserQuestion` rather than assuming the
  grant covers it.

Stop the moment the human revokes the grant or asks you to stop.
