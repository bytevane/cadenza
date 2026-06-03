# 设计:反偏离治理体系(借鉴 aiops-platform 的返工教训)

- 日期:2026-06-03
- 状态:已批准骨架,经双 reviewer(Claude + Codex)对抗式审查并修订(v2),待最终 review 后进实现计划
- 来源:对 `/Users/yvan/developer/aiops-platform` 的返工史分析(DEVIATIONS.md D1–D33、AGENTS.md harness principles 6/7、`validate-pr-metadata.mjs` author-time 门禁、`#588` 治理提交)

## 1. 背景与动机

aiops-platform 是 Symphony SPEC 的 Go 移植,与 cadenza 同类(都是 contract-first 的 Symphony-style 运行时)。它在一周内集中爆发了一轮"集体回退过度设计":十几个 `fix: remove/drop X (Dxx)` 提交,把 AI 自作主张加的东西删回 SPEC。

根因一句话:**AI 出于"增值感/安全感",把本属于 agent(prompt、预防式)或本不存在(上游没有)的东西,实现成了 orchestrator/worker 侧的阶段、门禁或上限。** 这造成大量"偏离 SPEC → 拉回对齐"的返工,浪费时间与 token。

aiops 的决定性止血点不是"写更多文档",而是**把 SPEC 对齐从 review 时的人工判断,变成 author-time 的机械 CI 门禁**(`#588` 原话:那些删除当初都带着规则照样合并了,因为检查是审计时判断而非作者时机械)。

cadenza 目前还是早期 scaffold(几乎没有 feature code),所以本设计对 cadenza 是**预防**而非回退——趁项目小、成本最低时把防护建起来。

## 2. 现状映射

### 2.1 cadenza 已有(不重复造)
- PR 模板已有 **Contract impact** 7 项敏感面 checklist + "勾选即必须链接 ADR" + Reviewer checklist(`.github/pull_request_template.md`)
- `decisions/` 8 个 ADR(0001–0008)+ `scripts/new-decision.sh`(**自动取下一个编号**)
- `tools/versions.toml` 单一版本真相源 + `crates/cadenza-core/src/contracts.rs` 两个 `cargo test`(`registry_text_has_no_pending_critical_keys`、`registry_text_documents_every_critical_key`)机械校验
- 三道契约门:WIT ABI(`scripts/check-wit-abi.sh`)、Codex schema(`scripts/codex-schema.sh --check`)、contract registry(上面两个 test)
- CI `--locked`;`CONTRIBUTING_AI.md` 已有 patch scope 纪律("AI tools tend to fix everything they notice. Keep them on a leash")

### 2.2 缺口(本设计要补)
| # | 缺口 | aiops 对应物 |
|---|---|---|
| G1 | Contract impact 全靠**人工勾选**,无 author-time 机械门禁——改了 `wit/` 却谎称"无影响",CI 不拦 | `validate-pr-metadata.mjs` + `pr-metadata.yml` |
| G2 | 无**偏离台账**(`decisions/` 是"方向/ADR",缺"差距/进度"那一半) | `DEVIATIONS.md` |
| G3 | `CONTRIBUTING_AI.md` 无**原则 6/7**(删除而非搬家;研究到结论不给菜单)、无"契约缺失=过度设计信号"显式规则、无 `Earned by` 溯源格式 | AGENTS.md harness principles |
| G4 | `rust-toolchain.toml`↔`versions.toml`↔`ci.yml` 三处重复 `1.95.0`,**无漂移机械校验**(只有注释);rust job 还硬编码版本而非从 ledger 读 | "scan==ship" 漂移门 |
| G5 | 无函数大小预算门、无 bootstrap 契约快照脚本 | `funlen`/`gocognit`、`bootstrap.sh` |

### 2.3 需改造(不能机械照抄 aiops)
- aiops 门禁要求引用 "Elixir reference `file:line`"——cadenza 没有 Elixir。对应权威源是 **`wit/runtime.wit` 签名 + 生成的 schema 字段 + Symphony SPEC §(已在 `versions.toml` pin commit sha `symphony_spec_sha`)**。优先本地可验证锚点(WIT/schema),SPEC § 次之。
- aiops 门禁脚本是 Node.js `.mjs`——cadenza 是纯 Rust workspace。门禁用 **Rust** 实现(决策见 §4 / §5 PR-C),避免引入 Node 工具链。
- aiops 删了一堆 worker gate——cadenza 还没实现这些 feature,故对 cadenza 是预防式条款而非删除 PR。

## 3. 目标与非目标

### 目标
1. 把"改了敏感路径必须声明契约影响 + 配 ADR"从人工自觉变成 author-time 机械门禁(补 G1)。
2. 建立偏离台账,作为 `decisions/`(方向)之外的"差距/进度"那一半(补 G2)。
3. 把反过度设计的元规则(原则 6/7、无界语义不加上限、Earned-by 溯源)写进 `CONTRIBUTING_AI.md`(补 G3)。
4. 用机械校验消除工具链版本三处重复与漂移(补 G4)。
5. 趁项目小,加函数大小门、bootstrap 契约快照脚本(补 G5)。

### 非目标(YAGNI,本次明确不做)
- **文件大小门(800 行类)**:Rust 无现成 lint,要自写脚本,cadenza 文件还小,收益不抵成本。
- **governance ruleset.json 入库**:那是 GitHub 仓库设置而非代码(见 §7 部署前置)。
- **引入 AGENTS.md 单一真相源 + 桥接重构**:本次保留现有 `CLAUDE.md` + `CONTRIBUTING_AI.md` 结构;Codex 读不到新规则的问题记为已知局限,留作未来可选项。
- **回退任何现有功能**:cadenza 还没有 aiops 那类越界 gate,本设计是预防,不删 feature。
- **PR size 声明(模板必填的体量三态)**:纯人工声明、无机械强制,cadenza 已有 scope 纪律(One PR = one surface)且早期 PR 天然小,边际收益不抵填写/维护负担——等真出现大 PR 苗头再加(原则 2:earned rules)。

## 4. 总体架构

治理体系分三层,互相配合:

```
规则层(文档)        CONTRIBUTING_AI.md 原则 6/7 + DEVIATIONS.md 台账
   │  立规矩:什么算偏离、发现偏离怎么办、偏离记在哪
   ▼
机械层(CI 门禁)     cadenza-cli pr-gate 子命令 + toolchain 漂移校验
   │  加牙齿:改敏感路径没声明/没 ADR → 合并被拦;版本漂移 → test 红
   ▼
预防层(开发期)      clippy 函数大小门 + bootstrap 契约快照
      降门槛:动手前先看冻结契约,写代码时函数别失控
```

每个组件是一个独立可交付的 PR,按 cadenza `CONTRIBUTING_AI.md` 的 "one PR = one surface" 纪律拆分。门禁逻辑做成 `cadenza-cli` 的子命令(而非新建 crate)——见 §5 PR-C 的取舍说明。

**ADR 编号约定**:本 spec **不钉死 ADR 编号**(`scripts/new-decision.sh` 自动取下一个号,硬编码会在任何计划外 ADR 先落地时失真)。下文用符号名引用(如「deviation-ledger ADR」「pr-gate ADR」),实现时用 `new-decision.sh` 取号。

## 5. 可交付件详细设计(6 个 PR,3 批)

### 批次 1 — 纯文档,先立规矩(零 CI 风险)

#### PR-A · `CONTRIBUTING_AI.md` 增补反过度设计原则
- **改动文件**:`CONTRIBUTING_AI.md`(增补一节 "Anti-over-design principles");`CLAUDE.md`(Patch discipline 段加一行指针,不复述,避免双份维护漂移)。
- **内容**:
  - **原则 6 — 契约缺失 = 过度设计信号**:新增任何作用于 agent 产出的 orchestrator/host 侧阶段/门/产物前,先确认 `wit/runtime.wit` / SPEC / 生成 schema 是否真允许它在这一侧;契约里没有对等物 = 强过度设计信号,**删除,不搬进 prompt、不只记一行文档**。
  - **原则 7 — 研究到结论再提**:面对可疑组件,把 SPEC + WIT + 参考研究做到能定结论再行动;不要把 keep/relocate/document 当多选题甩回给人(给菜单本身是研究没做完的症状)。
  - **无界语义不加上限**:`cadenza-orchestrator` 状态机除非 SPEC 给出 give-up 分支,不新增终结态/重试 cap/续跑 cap;任何 cap 需 ADR + 引用契约依据。
  - **Earned-by 溯源格式**:约定后续新增的纪律性规则尽量标注 `Earned by: #PR(失败症状)`,让规则可追溯、可在过时后删除。
- **验收标准**:`CONTRIBUTING_AI.md` 含上述四条且各有一句可操作判定;`CLAUDE.md` 有指针不复述;无 CI 改动。
- **ADR**:不需要(纯流程文档,非契约面)。

#### PR-B · 新建 `DEVIATIONS.md` 偏离台账 + deviation-ledger ADR
- **改动文件**:新建 `DEVIATIONS.md`;新建一条 deviation-ledger ADR(`new-decision.sh` 取号);`CONTRIBUTING_AI.md` 的 "When the AI tool is wrong" 段加指针(发现偏离 → 登记一行)。
- **台账结构**(单一 Markdown 表,表头 6 字段):
  - **ID**:`D` + 递增整数,**永不复用**。
  - **领域**:偏离了什么 + 简短修复叙事。
  - **契约引用**:**优先本地可验证锚点**——`wit/runtime.wit` 函数签名 / 生成 schema 字段;其次 SPEC §(配 `symphony_spec_sha`,记为 `symphony_spec_sha@§路径`,因 cadenza 仓内无 SPEC 全文,引用需对外部仓按 sha 核对)。
  - **Severity**:P0/P1/High/Medium/Low,**反映与契约的差距和风险,不是实现工作量**(与工作量解耦)。
  - **Status**:Open / Reverting(行为还在、计划删)/ Reverted(已删的过度设计)/ Closed / `Closed (accepted deviation)`(显式接受的活分歧、理由可见)。
  - **Tracking**:issue + PR 链接。
- **三条铁律**(写进文件顶部说明):ID 永不复用;行永不删(Closed 也留,供后续审计);Severity 与工作量解耦。
- **初始状态**:空表 + 使用说明(cadenza 还没 feature code,这是预防框架;`Closed (accepted deviation)` 必须配 ADR)。
- **验收标准**:`DEVIATIONS.md` 存在且含表头、三条铁律、Status 词汇表、使用说明;deviation-ledger ADR 记录"为何引入偏离台账、它与 decisions/ 的分工(方向 vs 进度)"。
- **ADR**:deviation-ledger ADR(本身是流程契约,值得一条 ADR 固化分工)。

### 批次 2 — 机械门禁,加牙齿

#### PR-C · author-time PR 门禁(`cadenza-cli pr-gate` 子命令)+ pr-gate ADR

**实现载体取舍(原则 6 自我适用)**:门禁**不是一个 contract surface**——它是 CI 工具,没有领域类型、无下游消费者、不被任何 crate 依赖。按 CLAUDE.md 的 crate-per-boundary(每 crate 隔离一个 contract surface),新建 `cadenza-pr-gate` crate 与该纪律有张力。故**做成 `cadenza-cli` 的子命令**(`cadenza-cli pr-gate`):cli 已是 operator entrypoint(`doctor`/`workspace-key`/`workspace-path` 都是这类工具命令),纯函数逻辑放 cli 内部模块并单测,零新增 workspace member。

- **改动文件**:`crates/cadenza-cli`(新增 `pr-gate` 子命令 + 纯函数模块 + 单测);新建 `.github/workflows/pr-metadata.yml`;新建一条 pr-gate ADR(`new-decision.sh` 取号)。
- **架构(隔离 IO 与纯逻辑,便于测试)**:
  - 纯函数模块:`evaluate(changed: &[ChangedFile], pr_body: &str) -> GateResult`。无 IO。`ChangedFile` 携带 `path` + 对 `tools/versions.toml` 额外携带「哪些 MVP-critical 键的**赋值右值**发生了变化」(由调用方预先算出,见下;复用 `cadenza_core::contracts::line_assigns_key` 取值比较,避免误判注释/格式改动)。
  - IO 层(子命令 `main`):从 `$GITHUB_EVENT_PATH` 的 JSON 读 `.pull_request.body`;取 changed files 与 versions.toml 键值变化(实现见下方"diff base"必须项);调 `evaluate`;失败 `exit(1)` 并打印可读理由。
- **diff base(必须项,否则门禁静默失效)**:`actions/checkout@v4` 默认 `fetch-depth: 1`,`origin/main` 在 runner 上不存在,`git diff origin/main...HEAD` 会报错或放空 → fail-open。故 `pr-metadata.yml` **必须** `fetch-depth: 0`(或用 `${{ github.event.pull_request.base.sha }}` 作 diff base),且门禁在 base ref 不可达 / diff 失败时 **fail-closed**(`exit 1`),绝不放过。
- **path classifier(两档,避免误报反噬)**:
  - **硬门禁路径(契约文件本身 + 门禁自身,改了几乎必然有契约影响,零误报)**:命中即强制勾对应 box + 配 ADR,否则 `exit 1`。
    | 类别 | 硬门禁路径模式 |
    |---|---|
    | Codex schema / cli_version | `schemas/codex/`、`ci/expected/codex-schema.sha256`、`tools/versions.toml` 的 `cli_version` 键**右值变化** |
    | WIT ABI | `wit/`、`abi/expected/` |
    | Pinned versions | `tools/versions.toml` 任意 **MVP-critical 键右值变化**(非"该行任意字节变化"——改注释/格式不触发) |
    | **门禁自我保护** | `.github/workflows/pr-metadata.yml`、`crates/cadenza-cli` 的 pr-gate 模块(改门禁本身 → 强制声明 + ADR) |
  - **软门禁路径(行为 crate,可能是契约语义改动也可能是无关内部重构)**:命中时**要求 PR body 显式声明**——勾对应 box(并按硬规则配 ADR),**或**写一行 `no <area> semantics change`;两者皆无 → `exit 1`。这比纯日志有牙齿(强制作者表态),又不需要 AST 判断语义。**诚实标注**:软门禁是"强制声明",不是"机械判定语义";真伪由 reviewer 兜底。各行为面映射到**独立**分类结果,与 PR 模板对应 box 对齐:
    | Box | 软门禁路径模式 |
    |---|---|
    | Secret/redaction/log field | host-secrets 相关路径、`crates/cadenza-obs/` 的 redaction 模块 |
    | Observability field names | `crates/cadenza-obs/` 的字段常量模块 |
    | Workspace path safety | `crates/cadenza-workspace/` |
    | Orchestrator state machine | `crates/cadenza-orchestrator/` |
  - 取舍依据(写进 pr-gate ADR):aiops 对行为代码只看"新增语义"信号而非任何改动,正是为了避免"一个你总是超的预算是减速带不是预算"。Rust 里"新增语义"不像 Go 能用"新增非测试文件"简单判定,故对行为 crate 退而用"强制声明"而非硬拦语义;契约文件则零误报可硬拦。
- **门禁规则**:
  1. PR body 必须含**恰好一个** `Closes #<n>` closing reference(不止存在性——拒绝多个 `Closes #`、占位 `Closes #`、重复关键词),兑现 one PR = one issue。
  2. 命中**硬门禁路径** → 对应 box 必须勾选;否则 `exit 1`("改了 X 却没声明/谎称无影响")。
  3. 命中硬门禁路径(或对应 box 勾选)→ changed files 必须含 `decisions/` 下新增/修改的 ADR 文件(**路径级判定**,不读 ADR 内容——内容由 reviewer 判,与 aiops `touchesDeviations` 一致);否则 `exit 1`。
  4. 命中**软门禁路径** → PR body 必须勾对应 box(并按规则 3 配 ADR)或含 `no <area> semantics change` 声明;否则 `exit 1`。
  5. **接受偏离逃生口**(兑现对 PR-B 的依赖):若改动是显式接受的偏离,合规方式 = changed files 含 `DEVIATIONS.md` 的新增行 + 对应 ADR;门禁承认 `DEVIATIONS.md` 改动为合规信号之一。
- **反作弊用例(cargo test,均用内联 fixture 字符串,不 `include_str!` 真模板)**:
  - 断言一段等同 PR 模板的空 body **不能满足门禁**(否则作者照抄模板就过)。**用内联 fixture**——避免 PR-E 改 `.github/pull_request_template.md` 时震红本 test(消除 PR-C↔PR-E 的耦合)。
  - 断言命中硬路径但 changed files 无 `decisions/` 改动 → 拒。
  - 断言嵌套子路径(如 `crates/cadenza-orchestrator/sub/x.rs`)与 `git mv` 进敏感目录都会触发(对应 aiops P2 加固)。
  - 断言多个 `Closes #` → 拒;`tools/versions.toml` 仅改注释 → 不触发 pinned-version 硬门。
- **workflow 触发器**:用 `pull_request`(**不是 `pull_request_target`**——后者 check 报告在 base SHA,分支保护按 head SHA 评估,导致 required check 永久 pending)。**必须显式列触发类型**:`types: [opened, reopened, synchronize, edited, ready_for_review]`——否则贡献者最后一次 push 后编辑 PR body,门禁不重跑、状态过期。本仓只接受可信维护者/agent 的同仓分支 PR,跑 PR 自身的 validator 可接受(且门禁自我保护已把门禁代码列入硬路径)。
- **验收标准**:`cargo test -p cadenza-cli` 覆盖上述规则与反作弊用例(全内联 fixture);`pr-metadata.yml` 用 `fetch-depth: 0`、显式 trigger types、`pull_request` 事件;门禁逻辑纯函数无 IO;base ref 不可达时 fail-closed;**workflow `name:` 与 job `name:` 在本 PR 钉死确切字符串**(供 §7 配 required check,避免猜错);pr-gate ADR 记录机制、author-time vs audit-time 理由、行为 crate 用软门禁的取舍。
- **ADR**:pr-gate ADR。

#### PR-D · toolchain 漂移门禁
- **改动文件**:`crates/cadenza-core/src/contracts.rs`(加纯函数 + test,沿用现有风格);`.github/workflows/ci.yml`(rust job 版本改为从 ledger 读)。
- **机制**:
  - 加 `pub fn` 从 `tools/versions.toml` 文本提取 `toolchain_version`、从 `rust-toolchain.toml` 文本提取 `channel`(均纯文本解析,无 TOML parser,与现有 `line_assigns_key` 风格一致)。
  - 加 `#[test]` 用 `include_str!("../../../tools/versions.toml")` 和 `include_str!("../../../rust-toolchain.toml")` 读真实文件,断言两者相等;不等则 test 红。
  - `ci.yml` rust job 当前硬编码 `toolchain: "1.95.0"`(第三处重复)。改为前置步骤 `awk` 从 `tools/versions.toml` 读出 `toolchain_version` 注入 `$GITHUB_ENV`,再传给 toolchain action。**已核实**:`dtolnay/rust-toolchain` 不自动读 `rust-toolchain.toml`,要用显式 `toolchain` input 须把 rev 固定为 `@master`(当前用的 `@stable` rev 会压过显式 input)。故 PR-D 改为 `dtolnay/rust-toolchain@master` + `toolchain: ${{ env.RUST_TOOLCHAIN }}`,消除硬编码常量。
- **验收标准**:改 `rust-toolchain.toml` 而不改 `versions.toml` 会让 `cargo test` 红;`ci.yml` 不再硬编码版本字面量、改用 `@master` + ledger 注入的显式 input;三处版本收敛到单一 ledger。
- **ADR**:不需要(强化既有 ADR 0004 的 `tools/versions.toml` 单一真相源,无新契约语义)。

### 批次 3 — 预防/打磨(用户决定三批全保留)

#### PR-E · 函数大小门
- **改动文件**:root `Cargo.toml` 加 `[workspace.lints.clippy]` + 各 crate `[lints] workspace = true`;新增 `clippy.toml`(配阈值)。
- **机制(已按 Rust 工具链核实)**:
  - `too_many_lines` 属 **pedantic 组,默认 `allow`**——光配 `clippy.toml` 的 `too-many-lines-threshold` **不会启用 lint**。必须显式启用:root `Cargo.toml` 加 `[workspace.lints.clippy] too_many_lines = "warn"`,各 crate `[lints] workspace = true` 继承(Cargo 1.74+ 特性,cadenza 用 1.95 满足);配合既有 `clippy ... -D warnings` 即升级为 error。`clippy.toml` 仅用于调阈值。**阈值取较宽值(如 150,而非默认 100)以减少早期 scaffold 摩擦**,后续可收紧。无需新 CI 步骤。
  - **不启用 `cognitive_complexity`**:它不是默认 lint(实测组别在 nursery / restriction 之间存在版本差异,且 clippy 官方明示其度量"不够好、易假阳性"),纳入 `-D warnings` 风险高、收益低。本 PR 只用 `too_many_lines`;`cognitive_complexity` 不采用(留作未来观察)。
- **验收标准**:超阈值函数会让 `clippy --workspace -- -D warnings` 失败(lint 被显式启用,而非依赖默认组);`cognitive_complexity` 未被启用/未被设为 deny。
- **ADR**:不需要。

#### PR-F · bootstrap 契约快照脚本
- **改动文件**:新建 `scripts/bootstrap-issue.sh`(`set -euo pipefail`);`CONTRIBUTING_AI.md` 的 "Required context" 段加指针。
- **机制**:fail-fast 脚本,接受 issue 号,打印:issue 全文(`gh issue view`)+ `wit/runtime.wit` + `tools/versions.toml` + `abi/expected/` 当前快照清单 + `git log -1 main`(冻结契约 + 基线)。逼 agent/operator 在改契约前先看冻结契约。
- **职责边界**(reviewer 提示):与现有 `scripts/bootstrap-dev.sh`(环境引导)区分——本脚本是**单 issue 上下文聚合**,主要服务"人类 operator / 新会话引导";agent 自身可直接 Read 这些文件。明确这是可选助手,行为约束的主力仍是 PR-A 的 `CONTRIBUTING_AI.md`。`gh issue view` 依赖网络/认证,失败时 fail-fast 报清原因。
- **验收标准**:脚本可运行、缺 issue 号时 fail-fast、打印上述全部内容;与 `bootstrap-dev.sh` 职责区分写清。
- **ADR**:不需要。

## 6. 分批、排序与依赖

| 批次 | PR | 依赖 | 风险 |
|---|---|---|---|
| 1 | PR-A(原则)| 无 | 零(纯文档)|
| 1 | PR-B(台账 + deviation-ledger ADR)| 无 | 零(纯文档)|
| 2 | PR-C(`cadenza-cli pr-gate` + pr-gate ADR)| PR-B(规则 5 的"接受偏离逃生口"需 `DEVIATIONS.md` 已存在作落点)| 中(cli 子命令 + CI workflow + 需 admin 配 required check)|
| 2 | PR-D(漂移门禁)| 无 | 低(沿用 contracts.rs 风格)|
| 3 | PR-E(函数大小门)| 无 | 低 |
| 3 | PR-F(bootstrap 脚本)| 无 | 零 |

排序原则:**先立规矩(批次 1)→ 再加牙齿(批次 2)→ 后打磨(批次 3)**。PR-C 依赖 PR-B 先落地(门禁规则 5 的"登记 deviation"要有台账可指)。PR-C 反作弊用内联 fixture 字符串、不依赖任何 PR 模板内容,故与其它 PR 无耦合。

## 7. 已知部署前置(必须点破)

PR-C 的门禁 workflow 加了之后,**必须由仓库 admin 在 GitHub branch protection 把 `pr-metadata` job 设为 required status check**,否则只是"显示红叉但能合并"的软门禁。aiops 血泪教训 + cadenza 适配:
1. required check 的 context 名要用 **Actions 实际发出的 job 名**(PR-C 已在本 PR 钉死 workflow `name:` 与 job `name:` 的确切字符串,admin 照抄),不是 PR UI 里的 `Workflow / Job` 形式;写错会让所有 PR 永久 pending、卡死全仓。
2. 不要用 `pull_request_target`(check 挂在 base SHA,required check 永远不出现)。
3. **merge queue 场景**:若将来启用 GitHub merge queue,PR 级 required check 不在 merge group 上报告,门禁会形同虚设——届时 `pr-metadata.yml` 须同时在 `merge_group` 事件上触发。cadenza 目前未用 merge queue,记为条件性注记。

此前置不在代码 PR 范围内,是部署动作,需在 PR-C 合并后由 admin 执行。

## 8. aiops → cadenza 改造对照

| aiops 机制 | cadenza 对应 |
|---|---|
| 引用 Elixir reference `file:line` | 引用 `wit/runtime.wit` 签名 / schema 字段(优先)/ SPEC §(配 `symphony_spec_sha`)|
| `validate-pr-metadata.mjs`(Node 独立脚本)| `cadenza-cli pr-gate` 子命令(Rust 纯函数 + cargo test)|
| `node --test`(门禁的门禁)| `cargo test -p cadenza-cli`(pr-gate 模块)|
| Dockerfile Go == go.mod 漂移门 | `rust-toolchain.toml` == `versions.toml` 漂移门(PR-D)|
| AGENTS.md(Claude+Codex 共读)| `CONTRIBUTING_AI.md`(已知局限:Codex 默认不读;留作未来可选项)|
| 删除越界 worker gate | 预防式条款(cadenza 还没实现这些 gate)|

## 9. 风险与权衡

- **软门禁≠机械判定语义**:对 `orchestrator`/`workspace`/`obs` 等行为面,门禁只能强制作者**声明**(勾 box 或 `no <area> semantics change`),不能机械判定是否真有语义变更——真伪靠 reviewer 兜底。这是相对 §1 立论(机械 > 人工)的诚实退让:契约文件零误报可硬拦,行为代码退而求"强制表态"以避免误报减速带。验收标准已明确软门禁为"强制声明、非语义门"。
- **Codex 读不到新规则**:本次不引入 AGENTS.md,Codex CLI 默认不读 `CONTRIBUTING_AI.md`。权衡:避免一次大重构;记为已知局限。若将来 Codex 侧偏离增多,再做 AGENTS.md 单一真相源迁移。
- **门禁基于路径分类、不做 AST 解析**:契约文件用键值/路径精确判定(零误报);行为 crate 用强制声明。可能仍有"声明了无变更但实际改了语义"的假阴性,由 reviewer 兜底。
- **同仓分支 PR 信任模型**:`pull_request` 触发跑 PR 自身的 validator;门禁自我保护(门禁代码列入硬路径)+ reviewer 审 `.github/` 改动共同防篡改。前提是本仓只接受可信维护者/agent 的同仓分支 PR;若将来接受 fork PR,需升级触发模型。
- **空台账的价值**:DEVIATIONS.md 初期是空表。权衡:它是预防框架 + 门禁规则 5 的落点,趁早建立结构成本最低。

## 10. 后续

每个 PR 各自独立、各带自己的验收标准。批次 1 两个 PR 可直接进入 writing-plans 产出实现计划;批次 2/3 在批次 1 落地后按序推进。本 spec 作为总纲,各 PR 不再重复总体动机,只引用本文件对应小节。

## 11. 修订记录

- **v1(2026-06-03)**:初稿,6 PR / 3 批骨架,经用户批准。
- **v2(2026-06-03)**:经 Claude + Codex 双独立 reviewer 对抗式审查后修订,吸收以下成立的 findings:
  - **Critical**:去掉硬编码 ADR 编号(与 `new-decision.sh` 自动取号冲突),改符号引用;`versions.toml` classifier 改判"MVP-critical 键右值变化"而非整行/整文件,避免误封注释改动。
  - **High**:补 `fetch-depth: 0` / base.sha + fail-closed(否则浅克隆下 diff 失效、门禁静默放过);软门禁路径从"纯日志"升级为"强制声明",并与现有 ADR 要求对齐、拆分 secret/redaction 与 obs field 两类。
  - **触发器/绕过**:`Closes #` 要求恰好一个;trigger 加 `edited` 等类型;补 `merge_group` 注记。
  - **贴合度**:门禁载体从新建 crate 改为 `cadenza-cli` 子命令(crate-per-boundary);门禁自我保护(门禁代码列入硬路径);反作弊用内联 fixture(与 PR-E 解耦);门禁规则 5 兑现 PR-C→PR-B 依赖;ADR 判定降为路径级(纯函数 API 可执行)。
  - **工具链**:砍 `cognitive_complexity`(组别版本差异 + 易假阳性),PR-E 只留 `too_many_lines`(阈值放宽到 150)。
  - **用户决定**:批次 3 三批全保留(reviewer 建议砍 PR size 声明/bootstrap,用户保留)。
- **v3(2026-06-03)**:用户复核批次 3——**砍掉 PR size 声明**(纯人工声明、无机械强制、cadenza 已有 scope 纪律、早期 PR 天然小),**保留 PR-F bootstrap 脚本**(契约冷启动入口,服务人类贡献者/新会话)。PR-E 收敛为纯函数大小门,不再改 PR 模板,与 PR-C 彻底解耦。
