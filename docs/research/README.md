# Cadenza Research Notes

本目录保存 Cadenza 项目前期研究、架构判断和实施准备资料。

Cadenza 的定位是：**基于 Symphony-style 工作流的 Rust + WebAssembly 编排运行时**。推荐架构是 Rust 原生主控服务负责调度、状态机、workspace、Codex app-server 客户端与可观测性；WebAssembly 组件负责受限扩展、安全工具调用、`linear_graphql`、safe hook 和未来多语言插件。

## 推荐阅读顺序

1. `01-feasibility-study.md`：为什么选择 Rust 主控 + Wasm 扩展层。
2. `02-implementation-readiness.md`：开工前必须冻结和准备的事项。
3. `03-initial-architecture.md`：初始架构、crate 划分和运行流程。
4. `04-project-naming.md`：项目名 Cadenza 的命名决策。
5. `05-source-manifest.md`：权威资料清单与用途。
6. `06-risk-register.md`：主要风险、触发条件和缓解措施。
7. `07-ai-collaboration-playbook.md`：Claude Code / Codex 协作开发规则。
8. `08-first-milestones.md`：8 周 MVP 推进计划。

## 外部资料策略

本目录不保存外部官方文档全文，只保存链接、摘要和工程用途。真正的权威来源包括：

- `openai/symphony` 的 `SPEC.md`；
- OpenAI Codex app-server 官方文档；
- Rust `wasm32-wasip2` 官方目标文档；
- Bytecode Alliance Component Model / WIT / Wasmtime 文档；
- `nearai/ironclaw` 中关于 Wasm 沙箱和 capability-based tools 的公开实现与接口参考；
- Claude Code CLI / Agent SDK 文档；
- GitHub Actions secrets / OIDC 安全文档。

项目实现时应以 `REFERENCE_SOURCES.md`、`tools/versions.toml`、`schemas/codex/current/`、`wit/` 和 `abi/expected/` 共同构成可审计事实基线。
