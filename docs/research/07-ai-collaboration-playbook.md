# AI 协作开发规则

Cadenza 会使用 Claude Code 和 Codex 辅助实现。为了避免“高产但不可控”的 patch 流，所有 AI 任务必须遵守本规则。

## 工具定位

| 工具 | 角色 | 推荐任务 |
|---|---|---|
| Codex | Runtime/protocol 实现助手 | app-server client、schema-bound types、event replay tests、stdio process harness |
| Claude Code | 工程化与文档助手 | crate skeleton、docs、CI、test fixtures、refactor、ADR、review checklist |

Claude Code 不进入 Cadenza 生产 runtime 链路。Codex app-server 是 runtime 依赖，但开发期 Codex CLI 也可作为代码生成工具。

## 任务输入模板

每次给 AI 的任务应包含：

```text
Context:
- Project: Cadenza
- Architecture: Rust host + Wasmtime/WASI Preview 2 extension layer
- SPEC baseline: <SPEC_SHA>
- Codex schema hash: <HASH or TODO>
- WIT package: bytevane:cadenza-runtime@0.1.x

Task:
- <specific implementation request>

Constraints:
- Do not invent app-server protocol fields.
- Do not change WIT without updating abi/expected.
- Do not expose secrets to Wasm guest.
- Add or update tests.
- Keep changes scoped to <crates/...>.

Output:
- Summary
- Files changed
- Tests added
- Risks / follow-up
```

## PR 模板要求

每个 AI-assisted PR 必须回答：

- 是否改动 Codex schema artifacts？
- 是否改动 WIT / ABI？
- 是否改动 secret surface？
- 是否改动 workspace path handling？
- 是否改动 retry/reconcile semantics？
- 新增或更新了哪些测试？
- 使用了哪些 AI 工具和 prompts？

## 禁止事项

- 禁止让 AI 自行推断 Codex app-server 协议 shape；
- 禁止新增 `get_secret()` 或向 Wasm guest 暴露原始 token；
- 禁止在 logs/tests/fixtures 中写入真实 token；
- 禁止绕过 `workspace.root` containment；
- 禁止直接把外部官方文档全文复制到仓库；
- 禁止在同一个 PR 同时改 schema、WIT 和 orchestrator state machine。

## 推荐分工

### Codex 适合生成

- JSON-RPC request/response wrapper；
- app-server replay fixtures；
- event stream parser tests；
- `tokio::process` 启动器；
- schema validation helpers。

### Claude Code 适合生成

- Rust crate boilerplate；
- test fixture builders；
- markdown docs；
- CI workflow；
- ADR；
- refactor patches；
- WIT comments 和 host function documentation。

## Review 清单

Reviewer 应重点检查：

1. 有没有引入未冻结的协议假设；
2. 有没有放宽 workspace containment；
3. 有没有泄露 secret 的路径；
4. Wasm host functions 是否最小能力；
5. 是否有 conformance / replay / unit tests；
6. 是否修改了 schema/ABI 但未更新快照；
7. 是否新增了观察字段但缺少脱敏。
