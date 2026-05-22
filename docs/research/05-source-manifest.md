# 权威资料清单

本文件记录 Cadenza 前期研究中依赖的权威资料和用途。外部资料不直接复制进仓库，应通过链接和版本记录追踪。

## Symphony / Codex

| 来源 | URL | 用途 |
|---|---|---|
| Symphony SPEC | https://github.com/openai/symphony/blob/main/SPEC.md | 实现基准：服务边界、workflow、workspace、agent runner、tracker、state machine、observability、test matrix |
| Symphony repo | https://github.com/openai/symphony | 上游仓库状态、README、原型实现说明 |
| OpenAI Symphony article | https://openai.com/index/open-source-codex-orchestration-symphony/ | 设计目标背景：issue tracker as control plane |
| Codex app-server docs | https://developers.openai.com/codex/app-server | app-server transport、JSON-RPC、schema generation、experimental status |
| Codex CLI reference | https://developers.openai.com/codex/cli/reference | `codex app-server`、`codex exec`、login、debug 命令 |
| OpenAI Codex repo | https://github.com/openai/codex | app-server implementation、README、issues、schema drift 参考 |

## Rust / WebAssembly / Component Model

| 来源 | URL | 用途 |
|---|---|---|
| Rust `wasm32-wasip2` target | https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip2.html | Rust guest component 编译目标依据 |
| WASI official site | https://wasi.dev/ | WASI 的安全标准接口定位 |
| Component Model guide | https://component-model.bytecodealliance.org/ | 组件模型基础 |
| WIT design | https://component-model.bytecodealliance.org/design/wit.html | WIT interface/world/package 设计依据 |
| Component worlds | https://component-model.bytecodealliance.org/design/worlds.html | world 组织方式 |
| Component packages | https://component-model.bytecodealliance.org/design/packages.html | package versioning 依据 |
| Rust component guide | https://component-model.bytecodealliance.org/language-support/building-a-simple-component/rust.html | Rust 原生 `wasm32-wasip2` 组件构建参考 |

## Wasmtime / Tooling

| 来源 | URL | 用途 |
|---|---|---|
| Wasmtime docs | https://docs.wasmtime.dev/ | runtime、examples、security |
| Wasmtime security | https://docs.wasmtime.dev/security.html | 沙箱安全边界和 threat model |
| Wasmtime component API | https://docs.wasmtime.dev/api/wasmtime/component/index.html | Rust host component API |
| Wasmtime WASI API | https://docs.wasmtime.dev/api/wasmtime_wasi/index.html | WASI integration |
| ResourceLimiter | https://docs.wasmtime.dev/api/wasmtime/trait.ResourceLimiter.html | Wasm resource limits |
| Config epoch/fuel | https://docs.wasmtime.dev/api/wasmtime/struct.Config.html | CPU interruption / fuel / epoch 配置参考 |
| `wasm-tools` | https://github.com/bytecodealliance/wasm-tools | `validate`、`component wit`、ABI introspection |
| `wit-bindgen` | https://github.com/bytecodealliance/wit-bindgen | guest binding generation |
| `cargo-component` | https://github.com/bytecodealliance/cargo-component | 过渡工具；因 experimental，不作为一阶段主路径 |

## IronClaw 参考

| 来源 | URL | 用途 |
|---|---|---|
| IronClaw repo | https://github.com/nearai/ironclaw | 安全 agent runtime、Wasm sandbox、credential injection、endpoint allowlist 参考 |
| IronClaw WIT / host functions | https://github.com/nearai/ironclaw/blob/staging/wit/tool.wit | `log`、`workspace-read`、`http-request`、`tool-invoke`、`secret-exists` 等能力启发 |
| IronClaw WIT mismatch issue | https://github.com/nearai/ironclaw/issues/840 | WIT 版本不兼容导致拒载的风险参考 |
| IronClaw host function issue | https://github.com/nearai/ironclaw/issues/1741 | host functions 列表参考 |

## Claude Code / AI Collaboration

| 来源 | URL | 用途 |
|---|---|---|
| Claude Code overview | https://docs.anthropic.com/en/docs/agents-and-tools/claude-code/overview | Claude Code 定位 |
| Claude Code CLI reference | https://docs.anthropic.com/en/docs/claude-code/cli-reference | `claude -p`、structured output、`--json-schema`、`--max-turns` |
| Claude Code SDK | https://docs.anthropic.com/en/docs/claude-code/sdk | Agent SDK capabilities |
| Claude Code TypeScript SDK | https://docs.anthropic.com/en/docs/claude-code/sdk/sdk-typescript | 远程/容器运行能力参考 |
| Claude Code IAM | https://docs.anthropic.com/en/docs/claude-code/iam | `setup-token`、CI/无浏览器环境 auth |
| Claude Code hooks | https://docs.anthropic.com/en/docs/claude-code/hooks | 开发期 hook 治理参考 |

## GitHub Actions / Security

| 来源 | URL | 用途 |
|---|---|---|
| GitHub Actions secrets | https://docs.github.com/actions/security-guides/using-secrets-in-github-actions | secrets 范围和使用建议 |
| Secure use reference | https://docs.github.com/en/actions/reference/security/secure-use | OIDC、最小权限、长凭据规避 |
| Contexts reference | https://docs.github.com/en/actions/reference/workflows-and-actions/contexts | 避免打印敏感 contexts |
| Workflow commands | https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands | masking / log hygiene |

## 使用规则

1. 每次升级外部依赖都应更新 `tools/versions.toml`。
2. Codex app-server 升级必须重新生成 schema artifacts 和 hash。
3. WIT 修改必须更新 `abi/expected/` 并在 PR 中说明 breaking/non-breaking 判断。
4. 外部文档 URL 可变时，在 ADR 或 release notes 中记录访问日期和版本。
5. 不要把外部文档全文复制进仓库，除非许可证明确允许且确有必要。
