# Cadenza 8 周 MVP 计划

本计划假设 2 名工程师，以 Claude Code 和 Codex 作为主要代码生成与审查辅助工具。

## Week 1：冻结边界

交付物：

- `SPEC_SHA`；
- Codex CLI/app-server 精确版本；
- Codex schema artifacts 和 hash；
- Rust / Wasmtime / wasm-tools 版本；
- `ARCHITECTURE.md`；
- `CONTRIBUTING_AI.md`；
- 初始 ADR。

验收：schema generation 脚本可运行；CI 能检查 hash placeholder 或实际 hash。

## Week 2：冻结 ABI

交付物：

- `wit/runtime.wit`；
- `abi/expected/runtime.wit`；
- Wasm hello-world plugin；
- Wasmtime host skeleton；
- ABI diff CI job。

验收：`wasm-tools validate` 和 `wasm-tools component wit` 能在本地或 CI 中跑通。

## Week 3：Workflow + Workspace

交付物：

- `WORKFLOW.md` loader；
- YAML front matter parser；
- strict prompt rendering；
- hot reload skeleton；
- workspace path sanitizer；
- containment tests。

验收：无效 workflow 不会污染 last-known-good；路径越界用例失败。

## Week 4：Codex app-server client

交付物：

- `stdio` launcher；
- app-server handshake；
- event stream parser；
- replay fixtures；
- process timeout/kill。

验收：本地 mock + 可选真实 app-server smoke test。

## Week 5：Wasm extension path

交付物：

- Wasmtime component loader；
- host functions：`log`、`workspace-read`、`secret-exists`；
- resource limits；
- `linear-graphql` plugin stub。

验收：插件能通过 WIT 调用 host-mediated capability，不能读取 raw secret。

## Week 6：Orchestrator state machine

交付物：

- candidate selection；
- claimed/running/retry queue；
- continuation；
- backoff；
- stall detection；
- reconcile。

验收：fault injection 能覆盖 normal exit、failure exit、stalled turn、terminal issue cleanup。

## Week 7：Security + CI 完整化

交付物：

- schema gate；
- WIT gate；
- secret scrubber；
- logs redaction；
- GitHub Actions OIDC/secrets notes；
- container hardening notes。

验收：敏感字段不会出现在 logs；ABI/schema drift 会阻断 PR。

## Week 8：MVP 封板

交付物：

- minimal conformance suite；
- real integration smoke profile；
- `README` quickstart；
- release notes；
- rollback drill；
- MVP demo script。

验收：可以从 Linear mock issue 到 Codex mock/real app-server 跑通最小闭环，并通过 Wasm 插件执行一个受控工具调用。
