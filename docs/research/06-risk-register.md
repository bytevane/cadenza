# Cadenza 风险台账

## 高优先级风险

| 风险 | 触发条件 | 影响 | 缓解措施 | Owner |
|---|---|---|---|---|
| Codex app-server 协议漂移 | CLI 升级、schema shape 变化 | handshake 失败、event parsing 失败 | 精确 pin 版本；schema artifacts 落库；CI hash gate；专门升级 PR | Runtime |
| WIT ABI 漂移 | world/package 变化、生成器升级 | 插件拒载或静默错配 | ABI 快照；`wasm-tools component wit` diff；0.x minor 视为 breaking | Wasm |
| Secret 泄漏 | log、hook output、plugin response、agent output | 凭据暴露 | host-mediated secrets；scrubber；禁止 `get-secret`；CI 不打印 contexts | Security |
| Workspace 越界 | `../`、symlink、absolute path、插件路径输入 | 跨 issue 污染、宿主文件泄露 | normalize + containment；统一 path API；负例测试 | Workspace |
| Shell hook 逃逸 | trusted hook 执行任意命令 | OS 级风险 | 专用用户；timeout；日志截断；二期 safe-hook Wasm 化 | Runtime |
| Wasm DoS | 插件死循环、内存增长、请求泛洪 | host 卡顿或崩溃 | ResourceLimiter；epoch/fuel；调用预算；rate limiting | Wasm |
| AI 生成代码分叉 | Claude/Codex 使用不同假设 | 架构漂移、PR 冲突 | `CONTRIBUTING_AI.md`；prompt templates；PR 模板；schema/WIT 版本声明 | Eng |
| Linear GraphQL schema/query 漂移 | Linear API 变化 | tracker fetch 失败 | query 集中在 adapter；contract tests；mock fixtures | Tracker |

## 中优先级风险

| 风险 | 触发条件 | 影响 | 缓解措施 |
|---|---|---|---|
| Hot reload 失败处理不当 | 无效 `WORKFLOW.md` | 服务崩溃或错误策略生效 | last-known-good；reload validation；reload error metrics |
| Retry/backoff 状态混乱 | normal exit / failure / continuation 语义混淆 | 重复 dispatch 或停止推进 | 明确 state machine；回放测试；fault injection |
| Observability 成为状态源 | API 修改 runtime state | correctness 分叉 | HTTP API 只读投影；refresh 只触发安全队列事件 |
| Cargo dependency drift | 未 pin 或间接升级 | 构建/ABI 行为变化 | `Cargo.lock` 提交；`cargo deny`；版本升级 PR |
| Container 权限过大 | root 用户、宽泛 volume | hook 或 Codex 子进程扩大影响 | non-root；read-only rootfs；最小 mount |

## 风险处理原则

- 对协议/ABI/secret 类风险采用 fail-closed；
- 对 observability/debug 类风险采用 degrade gracefully；
- 对外部依赖变化采用 explicit upgrade PR；
- 对 AI 生成代码采用 human review + conformance gate；
- 对安全边界变化要求 ADR。
