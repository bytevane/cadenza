# 命名决策：Cadenza

## 选定名称

项目名称：**Cadenza**

中文可译为：**华彩**。

## 命名寓意

Cadenza 是协奏曲中独奏者展示能力的华彩乐段。这个名字适合当前项目的三个核心特征：

1. **呼应 Symphony**：音乐语义与 Symphony 一致，但不是直接复制或派生官方项目名。
2. **强调可控发挥**：Codex、Claude Code、Wasm 插件像独奏者，有很强生成和执行能力，但仍然由 Rust host 控制节奏与边界。
3. **适合扩展生态**：未来可以自然形成 `cadenza-host`、`cadenza-wasm`、`cadenza-linear`、`cadenza-safe-hook` 等模块名。

## 推荐 tagline

```text
Cadenza: a Rust + WebAssembly orchestration runtime for Symphony-style Codex workflows.
```

中文描述：

```text
Cadenza 是一个基于 Rust 与 WebAssembly 组件模型的 Codex 工作流编排运行时，面向 Symphony 风格的长期任务调度、安全工具扩展与沙箱化执行。
```

## 模块命名建议

```text
cadenza-core
cadenza-host
cadenza-workflow
cadenza-workspace
cadenza-orchestrator
cadenza-codex
cadenza-wasm
cadenza-wit
cadenza-linear
cadenza-safe-hook
cadenza-obs
```

## 曾考虑的候选

| 名称 | 优点 | 未选原因 |
|---|---|---|
| `Continuo` | 强调长期运行、持续伴奏 | 品牌感稍弱，不如 Cadenza 有表现力 |
| `Rondo` | 短、好记，贴合调度循环 | 更像调度循环名，平台感略弱 |
| `Conductor` | 语义直接 | 过于通用，重名概率高 |
| `Chord` | 短，适合组件组合 | 过于通用，搜索和包名冲突风险高 |
| `Wasmphony` | 直观体现 Wasm + Symphony | 过于拼接，像非正式 demo 名 |
| `Maestro` | 指挥感强 | 重名概率高，品牌不够独立 |
| `Citadel` | 安全感强 | 音乐语义弱，偏安全产品 |

## 结论

`Cadenza` 在音乐语义、品牌独立性、技术延展性和开源项目辨识度之间取得较好平衡，因此作为项目正式名称。
