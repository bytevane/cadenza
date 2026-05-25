---
description: Audit and ship a GitHub PR through contract-aligned review rounds in cadenza. Manual invoke only.
argument-hint: "[pr-number]"
disable-model-invocation: true
allowed-tools: Bash(git *) Bash(ls *) Bash(grep *) Bash(find *) Bash(cargo *) Bash(./scripts/*) Bash(gh *)
---

# Handle PR #$ARGUMENTS

cadenza 是 **contract-first** 的 Rust + WebAssembly 编排运行时,冻结契约是硬要求。处理 PR #$ARGUMENTS(bytevane/cadenza)按以下流程做契约对齐审查轮。

## 当前 canonical main HEAD

!`git fetch https://github.com/bytevane/cadenza main && git --no-pager log --oneline FETCH_HEAD -1`

按 repo slug fetch canonical main(不依赖 `origin`,它可能是个人 fork),且用 `&&` 不让管道掩盖 fetch 失败——fetch 失败即中止,不在 stale ref 上继续。`FETCH_HEAD` 即刚 fetch 的 canonical main。读 PR head SHA 与 `git diff FETCH_HEAD...HEAD`(对 canonical main 出 diff,而非可能是 fork 的 `origin/main`)。契约的权威是仓库内冻结的快照(`abi/expected/`、`schemas/codex/`、`tools/versions.toml`)与 `CLAUDE.md` / `CONTRACTS.md` / `SECURITY.md`。

## 必做的几件不常规的事

1. **读 `CLAUDE.md` 的「Contract gates」「Patch discipline」与 `CONTRACTS.md`** 作为审查清单。每个 finding 至少归到一类:
   - **契约门禁完好** —— diff 是否动了 `wit/runtime.wit`、`abi/expected/*`、`schemas/codex/`、`ci/expected/codex-schema.sha256`、`tools/versions.toml`、`crates/cadenza-core/src/contracts.rs`?动了是否配了 `decisions/` 下 ADR、按 `docs/operations/wit-abi-versioning.md` 升版?**是否在用编辑快照糊弄 gate 失败(红旗——每个 gate 失败都自带 regen 命令,改快照应来自有意的变更 + ADR,而非压住失败)**?
   - **secret / workspace / orchestrator / obs** —— 各需 ADR。核:`cadenza-obs` 用字段名常量 + `redact_value` 而非自造串;`cadenza-workspace` 的 FS 访问都过 `ensure_inside`;raw secret 不进 guest 内存(`host-secrets` 只透露存在性);tracker 写不在 orchestrator 路径而走 `host-linear`;orchestrator 状态单一权威、无 I/O、排序确定性。
   - **跨 crate 一致性** —— 默认一个 crate / 一个 doc surface;跨 crate 改动是否给了理由?grep 改动概念的其它 consumer 是否一致。
   - **Rust 正确性** —— clippy 干净、无 `#[ignore]`/`--no-verify`/`if false` 跳过的测试、错误处理与边界。
   - **测试是否安慰剂** —— assertion 真的读到新代码改的字段吗?

2. **每修一轮派 subagent 独立审计一轮**:
   - `general-purpose` agent,背景跑。
   - prompt brief 完整(PR head SHA、改动文件路径、相关契约快照路径、`CLAUDE.md` + `CONTRACTS.md` + `SECURITY.md` 政策)。
   - **不透露你的结论**。
   - 要求 ≤700 字 + severity 标注 + 末尾 `MERGE-READY / NEEDS-CHANGES / BLOCKED` 判决。
   - 一般 2–3 轮收敛。可并行加派 Codex(`codex:codex-rescue`)盲审,两者抓不同缺陷类。

3. **若仓库配了 `@codex` GitHub bot**:每次 push 另跑 `gh pr comment <pr> --body "@codex review"`,轮询 trigger comment 的 reactions(等 👀 出现 → 消失 **且** 有正向完成信号),再查 `reviewThreads` 有无未解决 actionable thread。Codex review 不是 check run、reactions 没 watch API,只能轮询;CI 绿用原生 `gh pr checks <pr> --watch --fail-fast`。

4. **Mutation test 验证新测试有效**:删掉新代码的关键行,跑新测试,确认 fail;恢复,确认 pass。安慰剂测试是最隐蔽的陷阱。

5. **Deferred 缺口必须开 issue**:body 含相关契约/文件引用 + acceptance criteria,并从 PR 链接。决定延后就**当场**告知用户并立即开 issue,别攒到收尾汇报。

6. **Scope 分离**:治理 / 文档 / 契约改动从 main 开新分支单独 PR,不要塞进 fix PR。

## 验证(完成判定)
- 本地门禁全绿:`./scripts/check-all.sh`(+ 触及 Codex schema 时 `./scripts/codex-schema.sh --check`)。
- 新测试经变异验证。
- CI 全绿:`gh pr checks <pr> --watch --fail-fast` 阻塞到完成。
- 触及契约/secret/workspace/orchestrator/obs 的改动配了 ADR 且 PR 模板如实勾选。
- 无新的未解决 actionable review thread。

## 默认行为
- 工作分支:系统会告诉你具体名字。
- 合并方式:squash(本仓库惯例),commit_message 写最终状态不要按轮次罗列。
- 强推统一 `--force-with-lease=<branch>:<known-sha>`。
- merge 前必须等用户明确许可;例外:用户给了**按批次、按 scope 的显式授权**时,走 `docs/runbooks/batch-issue-processing.md` 的 opt-in 自动合并流程(全门槛 + hard stops,优先 GitHub 原生 auto-merge),授权不跨批次/scope 沿用。
- 批处理多个 PR 时的并行与状态清单纪律见 `docs/runbooks/batch-issue-processing.md`。
- 中文回复,简洁;每次只汇报变化不复述。
