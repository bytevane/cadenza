# 方案一开工前准备清单

本文件列出 Cadenza 开工前必须准备的工程治理事项。目标是让 Claude Code 和 Codex 可以高产写代码，同时不破坏架构边界和协议契约。

## 三条并行准备线

| 准备线 | 目标 | 交付物 |
|---|---|---|
| 运行时冻结线 | 固定外部协议和工具链 | `SPEC_SHA`、Codex version、schema hash、Rust/Wasmtime/wasm-tools versions |
| 扩展 ABI 线 | 固定 Wasm 插件契约 | `wit/runtime.wit`、`abi/expected/`、ABI diff CI |
| AI 生产线 | 约束 Claude Code / Codex 输出 | `CONTRIBUTING_AI.md`、prompt templates、PR template、review checklist |

## 必须冻结的版本

| 项目 | 冻结方式 | 原因 |
|---|---|---|
| Symphony SPEC | 记录 upstream commit SHA | SPEC 是实现基准，不能只引用 `main` |
| Codex CLI/app-server | 精确版本 | app-server 是外部协议依赖，可能变化 |
| Codex schema | 生成 JSON Schema / TS artifacts + hash | 协议 shape 必须机器可校验 |
| Rust toolchain | `rust-toolchain.toml` | 保持 host/guest 构建一致 |
| Wasmtime crate family | 同一精确版本族 | 避免 `wasmtime` / `wasmtime-wasi` / `wasmtime-wasi-http` 混搭 |
| `wasm-tools` | 精确 CLI 版本 | ABI diff 依赖工具输出稳定性 |
| WIT package version | `bytevane:cadenza-runtime@0.1.x` | 插件兼容性基线 |
| Claude Code CLI/SDK | 开发环境记录版本 | 避免 AI 生成/审查自动化脚本漂移 |

## 开工前必须落库的文件

```text
rust-toolchain.toml
Cargo.toml
REFERENCE_SOURCES.md
tools/versions.toml
schemas/codex/current/.gitkeep
ci/expected/.gitkeep
wit/runtime.wit
abi/expected/runtime.wit
CONTRIBUTING_AI.md
prompts/claude-dev.md
prompts/codex-runtime.md
.github/pull_request_template.md
```

这些文件不是装饰，而是 AI 协作和 CI 判断的事实基线。

## Codex app-server 准备

Codex app-server 必须被看作外部协议依赖，而不是普通 CLI。

建议策略：

1. 一阶段只使用 `stdio://`；
2. 不把 experimental WebSocket 作为生产路径；
3. 通过脚本生成 schema artifacts；
4. CI 中校验 schema hash；
5. app-server client 只依赖 schema 派生类型和集中映射层；
6. 任何 schema 变化必须走专门 PR。

推荐命令：

```bash
codex --version
codex app-server generate-ts --out schemas/codex/current
codex app-server generate-json-schema --out schemas/codex/current
find schemas/codex/current -type f -print0 | sort -z | xargs -0 cat | shasum -a 256
```

## WIT / ABI 准备

Cadenza 的 Wasm 扩展必须先有 WIT world，再让 AI 写插件。

最小 world 应包含：

- `host-log`；
- `host-time`；
- `host-workspace`；
- `host-secrets`；
- `host-http`；
- `host-linear`；
- `host-tools`；
- plugin `run` export。

建议在 0.x 阶段采用保守规则：**minor 版本变化也视为 breaking**。原因是早期接口语义会快速变化，宁可 fail closed，也不要让旧插件静默兼容。

## Secrets 策略

必须遵守：

- Wasm 组件不能直接获取 secret 值；
- 只提供 `secret-exists(name) -> bool`；
- `linear-graphql`、`http-request` 等能力由 host 注入凭据；
- 插件不得设置或透传 `Authorization` / `Cookie`，除非 host policy 显式允许；
- 所有日志、stderr、panic、hook output、tool output 都经过 scrubber；
- CI 中避免打印完整环境变量或 GitHub contexts。

## Claude Code / Codex 作为开发工具的边界

| 工具 | 建议用途 | 不建议用途 |
|---|---|---|
| Codex | app-server client、JSON-RPC/schema 类型、回放测试、runtime 协议贴身实现 | 自行发明协议 shape |
| Claude Code | 文档、CI、Rust 样板、测试数据、refactor、ADR、review checklist | 未冻结 WIT/Schema 前生成最终接口代码 |

每个 AI 生成 PR 应说明：

- 使用了哪个 prompt；
- 依赖哪个 SPEC/Codex/WIT 版本；
- 是否改动 schema/ABI/security surface；
- 新增了哪些测试；
- 是否可能影响 secrets/logging。

## 最小 CI gates

| Gate | 必须度 |
|---|---|
| `cargo fmt --check` | 必须 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 必须 |
| `cargo test --workspace` | 必须 |
| `cargo build --target wasm32-wasip2` | 必须 |
| `wasm-tools validate` | 必须 |
| `wasm-tools component wit` diff | 必须 |
| Codex schema hash diff | 必须 |
| conformance seed tests | 必须 |
| real integration smoke | 有凭据时运行 |

## 进入开发的 Go / No-Go 标准

只有满足以下条件才开始大规模让 AI 写业务代码：

- `tools/versions.toml` 中无关键 `TODO`；
- Codex schema 可以生成并有 hash；
- WIT world 已有 ABI 快照；
- CI 能阻断 schema/ABI drift；
- `CONTRIBUTING_AI.md` 和 PR template 已落库；
- secret policy 已写入 `SECURITY.md`；
- 至少有 workspace containment 测试和 workflow parser 测试。
