---
description: Take a tracked GitHub issue in cadenza from triage through contract investigation, TDD implementation, and local gates to an open PR, then hand off to handle-pr / gh-pr-follow-through. Use when starting work on an issue here — "处理下一个 issue", "修 #N", "实现这个 issue", "handle issue 48". Manual invoke only. Do NOT use when a PR already exists (use handle-pr / gh-pr-follow-through) or for trivial changes needing no contract investigation.
argument-hint: "[issue-number]"
disable-model-invocation: true
allowed-tools: Bash(bash .claude/skills/handle-issue/scripts/bootstrap.sh *) Bash(git *) Bash(ls *) Bash(grep *) Bash(find *) Bash(cargo *) Bash(./scripts/*) Bash(gh *)
metadata:
  pattern: inversion+pipeline+reviewer
  phase: issue→PR (hands off to handle-pr / gh-pr-follow-through)
---

# Handle Issue #$ARGUMENTS

cadenza 是 **contract-first** 的 Rust + WebAssembly 编排运行时。冻结契约(WIT ABI / Codex schema / contract registry,加 secret / workspace / orchestrator / obs 纪律)是硬要求,gate 在 CI 里失败得很响。本 skill 覆盖 **issue → 开 PR** 阶段;PR 之后交给 `handle-pr`(契约对齐审查轮)+ `gh-pr-follow-through`(盯到 merge-ready)。处理 issue #$ARGUMENTS(bytevane/cadenza)按下面流程。

## 何时用 / 不该用
- **用**:要开始处理本仓库一个已建的 GitHub issue("处理下一个 issue"、"修 #N"、"实现这个 issue")。
- **不该用**:PR 已存在 → 用 `handle-pr` + `gh-pr-follow-through`;无需契约调研的琐碎改动。

## 上下文就位(issue 全文 + main HEAD)

!`bash .claude/skills/handle-issue/scripts/bootstrap.sh "$ARGUMENTS" 2>&1`

`scripts/bootstrap.sh`(`set -euo pipefail`,fail-fast)依次做:校验参数为数字 issue 号(坏参数 exit 2,与后续 fetch 失败区分)→ 打印 issue **全文**(pin `--repo bytevane/cadenza`,不截断)→ `git fetch origin main` 后打印 main HEAD(fetch 失败即中止,无管道掩盖状态)。cadenza 不是端口,没有上游 SPEC 镜像;契约的权威是仓库内冻结的快照(`abi/expected/`、`schemas/codex/`、`tools/versions.toml`)。

开工前完整读 issue 正文,尤其逐条 Acceptance criteria。

## 流程(按序)

### 1. 读 issue
读 labels 与正文。**把正文的 Acceptance criteria 复选框当作 definition-of-done**——每条都要满足或显式说明为何不在范围内。

### 2. 契约调研(写代码之前)
- 对照 `CLAUDE.md` 的「Contract gates」「Patch discipline」与 `CONTRACTS.md`:判定本 issue 是否触及任一冻结契约 —— **WIT ABI(`wit/runtime.wit` / `abi/expected/*`)/ Codex schema(`schemas/codex/` / `ci/expected/codex-schema.sha256`)/ contract registry(`crates/cadenza-core/src/contracts.rs` + `tools/versions.toml`)/ secret 处理 / workspace 路径安全 / orchestrator 状态语义 / obs 字段名**。
- **触及任一 → 必须配 `decisions/` 下的 ADR**(用 `./scripts/new-decision.sh <slug>` 生成骨架)**且作为单独 PR**,不与 feature work 捆绑(`CONTRACTS.md`)。pre-1.0 每次 WIT minor bump 视为 breaking,除非 ADR 明确写 additive-only,并按 `docs/operations/wit-abi-versioning.md` 升版。
- 读 `CONTRIBUTING_AI.md` 确认 Codex/Claude 分工:protocol/app-server 相关代码是 Codex 的 lane(用 `prompts/codex-runtime.md`),一般实现用 `prompts/claude-dev.md`。需要时读对应 crate、`wit/runtime.wit`、`schemas/codex/`。
- **绝不臆造** schema 里不存在的协议字段,或 `wit/runtime.wit` 里没有的 WIT 函数。
- **审查相邻 crate**:grep 你要改的概念符号,列出其它 consumer;改动要么在它们那里也一致生效,要么写明为何不同(tracker 写操作刻意不在 orchestrator 路径,而走 Wasm `host-linear`——别把它挪进 orchestrator)。

### 3. 分支 + 实现
- 从 `main` 开 `issue-<n>-<slug>`(如 `issue-48-wasm-host-fixes`);未跟踪的杂活用 `feat/`/`fix/`/`infra/`/`sec/`/`docs/`/`chore/`。
- **默认一个 crate 或一个 doc surface**;跨 crate patch 需在 PR 描述里给理由(`CLAUDE.md` Patch discipline)。
- 不把重构与修复捆绑;不顺手重命名无关变量、re-export 类型或删无关注释。
- orchestrator 改动保持**确定性**(排序 priority → created_at → identifier;state 单一权威,无 I/O)。

### 4. 测试纪律(TDD)
- **新行为配 failing-first 测试**(`CLAUDE.md` 明令);每个修复配回归测试 + **变异测试**:删掉新代码关键行 → 新测试必须 FAIL;恢复 → PASS。安慰剂测试是最隐蔽的陷阱。
- **禁止** `#[ignore]` / `--no-verify` / `if false` 跳过测试(`CLAUDE.md` 明令)。
- 负向断言(如「未发生」)要有确定性 barrier,别把概率性 barrier 写成 race-free。

### 5. 本地门禁(必须,且与 CI 一致)
```bash
./scripts/check-all.sh   # cargo fmt --check + clippy -D warnings + test --workspace + wasm build + check-wit-abi.sh
```
- 触及 Codex schema 时另跑 `./scripts/codex-schema.sh --check`(CI 里是独立 job)。
- contract registry 两个测试随 `cargo test -p cadenza-core` 跑(`registry_text_has_no_pending_critical_keys` / `registry_text_documents_every_critical_key`)。
- **绝不靠编辑契约快照(`abi/expected/*`、`schemas/codex/`、`ci/expected/codex-schema.sha256`)来糊弄 gate 失败**——那是 ABI/schema 变更,要配 ADR。
- **暂存只 add 明确路径,绝不 `git add -A` / `git add .`**。commit 前 `git status --short` 核对只暂存了预期文件。

### 6. 开 PR + 审查环(关键)
1. 开 **一个** PR 对应该 issue,body `Closes #N` 并**完整填写强制 PR 模板**(`.github/pull_request_template.md`:触及哪些契约、测试清单、AI 辅助声明)。治理/文档/契约类改动**单开 PR**,不要塞进 fix PR。
2. **默认每次 push 都派两个独立盲审**——不是每个 PR 一次,是每个 commit。同时派 Claude `general-purpose` subagent + Codex(`codex:codex-rescue`)审 `git diff origin/main...HEAD`,**不透露你的结论**,要求 severity 标注 + 末尾 `MERGE-READY / NEEDS-CHANGES / BLOCKED` 判决。两者抓不同缺陷类。(注:codex-rescue 沙箱可能被网络限制;受限时以本地审查为主。)
3. **若仓库配了 `@codex` GitHub bot**:每次 push 另跑 `gh pr comment <pr> --body "@codex review"`,记下该 trigger comment 的 id → **轮询**它自带的 reactions 计数摘要:`gh api repos/bytevane/cadenza/issues/comments/<id> --jq '.reactions.eyes'`。Codex review 不是 check run、reactions 没有 watch API,只能轮询;**CI 绿**则用原生 `gh pr checks <pr> --watch --fail-fast`,别 sleep 轮询。**只 `eyes==0` 不算完成**(可能还没开始 👀):必须等到先出现 👀、再消失,**且**有正向完成信号(Codex 在该 head 贴了 review/comment,或 trigger comment 拿到 👍),然后查 `reviewThreads` 有无新的未解决 actionable thread。
4. 每条 finding 归入 ≥1 类(**契约漂移 / 跨 crate 一致性 / Rust 正确性 / 安慰剂测试**),然后修掉或**开 follow-up issue 延后**(body 含相关契约/文件引用 + acceptance criteria,并从 PR 链接)。
5. **审查深度匹配 blast radius**:触及契约、secret、workspace 安全、orchestrator 状态的破坏性路径要穷尽对抗式审查;纯增量改动一轮即可。
6. 收敛后交给 `gh-pr-follow-through` 盯 CI + 线程到 merge-ready。**该 skill 不可用时就地内联这步**:`gh pr checks <pr> --watch --fail-fast` 等 CI 收敛 → 查 `reviewThreads` 解决所有未决 actionable thread → 直到 merge-ready,别因为缺 skill 而跳过这步。follow-through 期间若推了修复,按 step 2–3 对新 head 重跑审查环(新 push 会重开审查轮)。

### 7. 合并
- **必须等用户明确许可**再合并。
- 例外:用户给了**按批次、按 scope 的显式授权**时,可走 `docs/runbooks/batch-issue-processing.md` 的 opt-in 自动合并流程(全部放行门槛 + hard stops;优先 GitHub 原生 auto-merge)。授权不是长期的,批次外/scope 外仍要重新确认。
- squash + 删分支;commit message 写**最终状态**,不要按轮次罗列。
- 强推统一 `--force-with-lease=<branch>:<known-sha>`。

## 反模式备忘(踩过的坑)
- **靠编辑快照糊弄 gate**:`check-wit-abi.sh` / `codex-schema.sh --check` 失败时,改 `abi/expected/*` 或 `ci/expected/*.sha256` 让它过 —— 这是把 ABI/schema 变更伪装成无变更。每个 gate 失败都自带 regen 命令,真要变就配 ADR + 升版。
- **把 tracker 写挪进 orchestrator**:写操作刻意走 Wasm `host-linear`,orchestrator 是无 I/O 的单一权威状态机(`SECURITY.md` / `CLAUDE.md`)。
- **raw secret 跨进 guest 内存**:`host-secrets` 只透露存在性,`host-linear` 在 host 侧注入凭证;别让原始 token 进 guest。
- **在 feature PR 里改 `tools/versions.toml` 的 pinned 版本**:那需要专属 PR + ADR(`CONTRACTS.md`),不能与 feature work 捆绑。
- **自造 obs 字段名/串**:用 `cadenza-obs` 的字段名常量 + `redact_value`,别发明新字符串。
- **绕过 `ensure_inside` 直接做 FS 访问**:所有 workspace FS 访问都过 `cadenza-workspace`(`SECURITY.md`)。
- 修复 A 引入缺陷 B:每次「修复」后重新审整条路径。

## 验证(完成判定)
- 本地门禁全绿:`./scripts/check-all.sh` 通过;触及 Codex schema 时 `./scripts/codex-schema.sh --check` 也通过。
- 新测试经变异验证(删关键行 FAIL / 恢复 PASS)。
- CI 全绿:`gh pr checks <pr> --watch --fail-fast` 阻塞到完成。
- 触及契约/secret/workspace/orchestrator/obs 的改动配了 `decisions/` 下的 ADR,且 PR 模板里如实勾选。
- 配了 `@codex` bot 时,PR 最新 head 上 `@codex review` 收敛(👀 出现后消失 **且** 有正向完成信号),无新的未解决 actionable thread。
- issue 的每条 Acceptance criteria 满足或显式延后到 tracked issue。
- 用户明确许可后才合并。

## 默认行为
- 中文回复,简洁;每次只汇报变化,不复述。
- worker 永不擅自 push 到 main / 合并 PR / 改 `tools/versions.toml` 的 pinned 版本。
- Rust 工具链由 `tools/versions.toml` + `rust-toolchain.toml` 锁定,别顺手改。
- **批处理多个 issue 时**:独立 issue **默认并行**开分支推进,只在有真实依赖(共享 crate / 共享 `cadenza-core` 类型 / `tools/versions.toml`/`Cargo.lock` 冲突 / 后者消费前者的 API)时串行;决定某条 acceptance criterion 延后时**当场**告知用户并立即开 follow-up issue,别留到收尾汇报。完整批处理纪律见 `docs/runbooks/batch-issue-processing.md`。
