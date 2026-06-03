# 反偏离治理 · 批次 1 实现计划（PR-A 原则 + PR-B 台账）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地反偏离治理体系的「规则层」——把反过度设计原则写进 `CONTRIBUTING_AI.md`（PR-A），新建 `DEVIATIONS.md` 偏离台账 + 配套 ADR（PR-B）。

**Architecture:** 纯文档变更，零 CI 风险。PR-A 是规则文本 + `CLAUDE.md` 指针；PR-B 是台账文件 + 一条 ADR + `CONTRIBUTING_AI.md` 指针。两个 PR 都改 `CONTRIBUTING_AI.md` 不同段落，故**串行执行 PR-A → PR-B** 以免合并冲突。

**Tech Stack:** Markdown 文档；`scripts/new-decision.sh`（ADR 取号）；`git`。

**Spec:** `docs/superpowers/specs/2026-06-03-anti-deviation-governance-design.md`（§5 PR-A / PR-B）。

**测试说明:** 批次 1 无可执行代码，cadenza 的 failing-first TDD 不适用。每个 task 用**内容验证**（`grep`/`rg` 断言关键节与短语存在）替代单元测试，并以原子 commit 收尾。

---

## 分支与 PR 边界

- 当前分支 `docs/anti-deviation-governance` 已持有 spec（commit `7bb1c6c`）。
- **PR-A** 与 **PR-B** 是两个独立 PR（one PR = one surface）。推荐做法：
  - 在 `main` 上为 PR-A 开分支 `docs/anti-over-design-principles`，完成 Task A1–A2，开 PR。
  - PR-A 合并后，从更新的 `main` 为 PR-B 开分支 `docs/deviation-ledger`，完成 Task B1–B3，开 PR。
  - 若两个 PR 需并行评审，PR-B 分支基于 PR-A 分支创建，避免 `CONTRIBUTING_AI.md` 冲突。
- spec 本身（已在 `docs/anti-deviation-governance`）可作为独立 docs PR，或并入 PR-A 一起评审——由 operator 决定，不影响下列 task。

---

## PR-A:CONTRIBUTING_AI.md 反过度设计原则

### Task A1: 在 CONTRIBUTING_AI.md 新增 "Anti-over-design principles" 节

**Files:**
- Modify: `CONTRIBUTING_AI.md`（在 `## Patch scope` 之后、`## Branch naming` 之前插入新节）

- [ ] **Step 1: 定位插入点**

Run: `grep -n "## Branch naming" CONTRIBUTING_AI.md`
Expected: 输出一行（约 `51:## Branch naming`）。新节插在该行之前。

- [ ] **Step 2: 插入新节**

用 Edit 工具，`old_string` 锚定 `## Branch naming` 标题，在其前插入下面整节（保留 `## Branch naming` 不变）。新节完整内容：

```markdown
## Anti-over-design principles

Earned by the aiops-platform port (a sibling Symphony runtime), which spent a
dozen `remove/drop` PRs unwinding gates, caps, and config the AI added that the
contract never asked for. These principles exist to keep cadenza from repeating
that rework.

- **Contract absence is an over-design signal.** Before adding any
  orchestrator/host-side stage, gate, artifact, or config that acts on agent
  output, confirm the behaviour is actually permitted there by `wit/runtime.wit`,
  the generated Codex schema, or SPEC. If the contract has no equivalent, that is
  a strong signal the component is over-design, not a feature gap to fill.
  **Delete it — do not relocate it (move-to-prompt) or merely document it (a
  `DEVIATIONS.md` row).** Relocating or documenting preserves scaffolding that no
  longer earns its place.

- **Research to a verdict before proposing; bring the verdict, not a menu.** When
  SPEC + WIT + reference research settles whether a component belongs, decide and
  act on it. Do not hand a keep / relocate / document multiple-choice back to the
  reviewer — that menu is usually a symptom that the research which would rule out
  "keep" was not finished. Reserve genuine choices for scope, intent, or safety
  forks the contract leaves open.

- **Unbounded semantics get no caps.** The orchestrator state machine (`claimed`
  / `running` / `retry_attempts`) gets no new terminal state, retry cap, or
  continuation cap unless SPEC gives a give-up branch. Adding any cap requires an
  ADR citing the contract basis. SPEC retries are unbounded with backoff; do not
  "make it safer" by inventing a ceiling.

- **Earn your rules.** A new discipline rule should trace to a specific observed
  failure. Annotate it `Earned by: #PR (symptom)` so it can be audited and
  removed when a future model no longer needs it. When in doubt, leave a rule out
  until a failure demands it.

```

- [ ] **Step 3: 验证四条原则都在**

Run: `grep -c -E "Contract absence is an over-design signal|Research to a verdict|Unbounded semantics get no caps|Earn your rules" CONTRIBUTING_AI.md`
Expected: `4`

- [ ] **Step 4: 验证节标题与节序**

Run: `grep -n -E "^## (Patch scope|Anti-over-design principles|Branch naming)" CONTRIBUTING_AI.md`
Expected: 三行按 `Patch scope` → `Anti-over-design principles` → `Branch naming` 顺序出现。

- [ ] **Step 5: Commit**

```bash
git add CONTRIBUTING_AI.md
git commit -m "docs(contributing): add anti-over-design principles (delete-don't-relocate, verdict-not-menu, no-caps, earned-rules)"
```

### Task A2: CLAUDE.md Patch discipline 加指针

**Files:**
- Modify: `CLAUDE.md`（`## Patch discipline` 段末尾追加一个 bullet）

- [ ] **Step 1: 读取并定位 Patch discipline 段末尾**

Run: `grep -n -E "^## Patch discipline|failing-first test|Do not skip tests" CLAUDE.md`
Expected: 找到 `## Patch discipline` 标题行，及其下最后一个 bullet（含 `Do not skip tests` / `failing-first test`）。用 Read 确认该 bullet 的精确文本，作为 Edit 锚点。

- [ ] **Step 2: 追加指针 bullet**

用 Edit 工具，在 `## Patch discipline` 段的最后一个 bullet（`- Do not skip tests via ... (TDD).`）之后追加下面一行（不复述原则,只指针,避免双份维护漂移）：

```markdown
- Anti-over-design rules (delete don't relocate; research to a verdict, not a menu; no caps on unbounded semantics; earn your rules) live in `CONTRIBUTING_AI.md` → "Anti-over-design principles". Don't duplicate them here.
```

- [ ] **Step 3: 验证指针存在且只一行**

Run: `grep -c "Anti-over-design rules" CLAUDE.md`
Expected: `1`

- [ ] **Step 4: 验证未复述原则正文（防双份维护）**

Run: `grep -c "Earned by: #PR" CLAUDE.md`
Expected: `0`（原则正文只在 CONTRIBUTING_AI.md，CLAUDE.md 仅指针）

- [ ] **Step 5: Commit**

```bash
git add CLAUDE.md
git commit -m "docs(claude): point Patch discipline to CONTRIBUTING_AI anti-over-design principles"
```

---

## PR-B:DEVIATIONS.md 偏离台账 + ADR

> 从更新后的 `main` 开 `docs/deviation-ledger` 分支后开始。

### Task B1: 生成并填充 deviation-ledger ADR

**Files:**
- Create: `decisions/0009-deviation-ledger.md`（实际编号以 `new-decision.sh` 输出为准）

- [ ] **Step 1: 用脚本取号生成 ADR 骨架**

Run: `./scripts/new-decision.sh deviation-ledger`
Expected: 打印新文件路径（现有到 0008，预期 `decisions/0009-deviation-ledger.md`）。**记下实际编号 NNNN**，后续 Task B2 引用它。

- [ ] **Step 2: 用完整内容覆盖该文件**

用 Write 覆盖 `decisions/0009-deviation-ledger.md`（编号替换为脚本实际输出；Status 设为 `Accepted.`，与现有 0001–0008 一致，随 PR 合并即生效）：

```markdown
# ADR 0009: deviation-ledger

## Status

Accepted.

## Context

cadenza is contract-first: SPEC, `wit/runtime.wit`, the generated Codex schema,
and `tools/versions.toml` are frozen contracts. `decisions/` records *direction*
(why a contract is shaped the way it is), but there is no single place that
tracks *gaps* — confirmed or accepted divergences from those contracts and their
resolution status. The sibling aiops-platform port demonstrated the cost of that
omission: deviations accumulated invisibly until a batch audit, then took a dozen
`remove/drop` PRs to unwind.

## Decision

Introduce `DEVIATIONS.md`, a single-table ledger that is the gap/progress half of
governance, complementing `decisions/`:

- `decisions/` (ADRs) = direction and rationale.
- `DEVIATIONS.md` = per-deviation gap tracking: ID, area, contract reference,
  severity, status, tracking issue/PR.

Three rules govern it: IDs are never reused, rows are never deleted, and severity
reflects the contract gap rather than implementation effort. A
`Closed (accepted deviation)` row must be backed by an ADR.

## Rationale

Turning silent drift into a visible, append-only ledger lets audits see what was
resolved and stops over-design from reappearing under a new name. Keeping the
ledger separate from `decisions/` preserves the direction-vs-progress distinction
that a single ADR list blurs. Severity is decoupled from effort so a hard-to-fix
deviation cannot be quietly down-graded.

## Consequences

- Any change that violates a frozen contract must either close an existing row,
  add a new tracked row, or be reverted — it cannot silently disappear.
- `Closed (accepted deviation)` rows pair with an ADR.
- The author-time PR gate (a later batch) will treat a `DEVIATIONS.md` row as one
  compliant way to land a contract-touching change.
- The ledger starts empty; it is a prevention framework, not a backlog.
```

- [ ] **Step 3: 验证 ADR 格式完整**

Run: `grep -n -E "^## (Status|Context|Decision|Rationale|Consequences)" decisions/0009-deviation-ledger.md`
Expected: 五个标题都在；`Status` 下为 `Accepted.`

- [ ] **Step 4: Commit**

```bash
git add decisions/0009-deviation-ledger.md
git commit -m "docs(adr): 0009 deviation-ledger — gap/progress half of governance"
```

### Task B2: 新建 DEVIATIONS.md 台账

**Files:**
- Create: `DEVIATIONS.md`（仓库根）

- [ ] **Step 1: 写台账文件**

用 Write 创建 `DEVIATIONS.md`（把 `decisions/0009-deviation-ledger.md` 的编号替换为 Task B1 实际编号）：

```markdown
# Deviations ledger

The gap/progress half of cadenza's governance, paired with `decisions/` (which
records direction). Every confirmed or accepted divergence from a frozen contract
(SPEC, `wit/runtime.wit`, the generated Codex schema, `tools/versions.toml`) gets
one row here. See `decisions/0009-deviation-ledger.md` for why this exists, and
`CONTRIBUTING_AI.md` → "Anti-over-design principles" for what counts as a
deviation.

## Three rules

1. **IDs are never reused.** `D1`, `D2`, … monotonically. A removed deviation
   keeps its ID and its row.
2. **Rows are never deleted.** A `Closed` or `Reverted` row stays visible so
   future audits can see what was resolved and how.
3. **Severity reflects the contract gap, not implementation effort.** A one-line
   fix can be P0; a large refactor can be Low.

## Status vocabulary

| Status | Meaning |
|---|---|
| `Open` | Confirmed deviation, not yet addressed. |
| `Reverting` | Behaviour still ships; removal is planned. |
| `Reverted` | The over-design was deleted. |
| `Closed` | Resolved (aligned to contract). |
| `Closed (accepted deviation)` | A live divergence accepted on purpose; rationale stays visible and **must** be backed by an ADR under `decisions/`. |

## Contract reference anchors

Prefer locally verifiable anchors, in this order:

1. `wit/runtime.wit` function signature.
2. Generated Codex schema field.
3. Symphony SPEC § + `symphony_spec_sha` (recorded as `symphony_spec_sha@§path`;
   cadenza does not vendor SPEC.md, so the reference is checked against the pinned
   commit in `tools/versions.toml`).

## Ledger

| ID | Area | Contract reference | Severity | Status | Tracking |
|----|------|--------------------|----------|--------|----------|
| _none yet_ | | | | | |

<!--
Row template (copy, fill, assign the next unused D-number):
| D1 | <what deviates + short fix narrative> | <wit sig / schema field / SPEC §> | P0|P1|High|Medium|Low | Open|Reverting|Reverted|Closed|Closed (accepted deviation) | #issue / #pr |
-->
```

- [ ] **Step 2: 验证关键结构都在**

Run: `grep -c -E "^## (Three rules|Status vocabulary|Contract reference anchors|Ledger)" DEVIATIONS.md`
Expected: `4`

- [ ] **Step 3: 验证三条铁律与 Status 词汇表**

Run: `grep -c -E "IDs are never reused|Rows are never deleted|Severity reflects the contract gap|Closed \(accepted deviation\)" DEVIATIONS.md`
Expected: `4`（最后一项 `Closed (accepted deviation)` 在词汇表中出现）

- [ ] **Step 4: Commit**

```bash
git add DEVIATIONS.md
git commit -m "docs: add empty DEVIATIONS.md ledger (ids never reused, rows never deleted, severity decoupled from effort)"
```

### Task B3: CONTRIBUTING_AI.md "When the AI tool is wrong" 加指针

**Files:**
- Modify: `CONTRIBUTING_AI.md`（`## When the AI tool is wrong` 段末尾追加一个 bullet）

- [ ] **Step 1: 定位段末**

Run: `grep -n -E "## When the AI tool is wrong|Record recurring mistakes" CONTRIBUTING_AI.md`
Expected: 找到该节标题与其最后一个 bullet（`- Record recurring mistakes in ...`）。用 Read 确认该 bullet 精确文本作 Edit 锚点。

- [ ] **Step 2: 追加 bullet**

用 Edit 工具，在 `- Record recurring mistakes in ...` bullet 之后追加：

```markdown
- When you confirm a deviation from a frozen contract, log it as a row in `DEVIATIONS.md` (do not silently make the discrepancy disappear); an accepted deviation also needs an ADR under `decisions/`.
```

- [ ] **Step 3: 验证指针存在**

Run: `grep -c "log it as a row in \`DEVIATIONS.md\`" CONTRIBUTING_AI.md`
Expected: `1`

- [ ] **Step 4: Commit**

```bash
git add CONTRIBUTING_AI.md
git commit -m "docs(contributing): log confirmed deviations as DEVIATIONS.md rows"
```

---

## 完成后的验证(整批)

- [ ] `grep -rn "DEVIATIONS.md" CONTRIBUTING_AI.md CLAUDE.md decisions/0009-deviation-ledger.md` —— 确认三处交叉引用闭环（CONTRIBUTING 指向台账、ADR 解释台账、台账指回 ADR/CONTRIBUTING）。
- [ ] `grep -rn "Anti-over-design principles" CONTRIBUTING_AI.md CLAUDE.md` —— 确认 CLAUDE.md 指针指向 CONTRIBUTING_AI.md 的节名。
- [ ] 人工 review:四条原则措辞可操作、台账三铁律清晰、ADR Status 为 Accepted、空台账行模板可复制。

---

## Self-Review(计划作者已执行)

- **Spec 覆盖**:§5 PR-A 四要点（原则 6 删除不搬家 / 原则 7 结论不给菜单 / 无界语义不加上限 / Earned-by 格式）→ Task A1 全覆盖；CLAUDE.md 指针 → Task A2。§5 PR-B（台账 6 字段表头、三铁律、Status 词汇表、本地优先锚点、空表 + 使用说明、deviation-ledger ADR、CONTRIBUTING 指针）→ Task B1（ADR）+ B2（台账）+ B3（指针）全覆盖。
- **Placeholder 扫描**:无 TBD/TODO；所有文档正文为完整可粘贴内容；ADR/台账编号以 `new-decision.sh` 实输出替换，已在 step 注明（非占位,是脚本取号约定,符合 spec「不钉死编号」）。
- **一致性**:节名 "Anti-over-design principles" 在 A1 定义、A2 指针、B2 台账引用三处一致；`DEVIATIONS.md` 文件名、`decisions/0009-deviation-ledger.md` 路径在 B1/B2/B3 与整批验证处一致;Status 词汇表五项与 spec §5 PR-B 一致。
