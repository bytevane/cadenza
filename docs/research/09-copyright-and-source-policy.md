# 外部资料与版权策略

Cadenza 仓库可以保存我们自己的研究结论、摘要、ADR 和工程判断，但不建议直接保存外部官方文档、文章或第三方仓库文件全文。

## 原则

1. **链接优先**：外部资料以 URL、访问日期、用途和摘要形式记录。
2. **摘要不替代原文**：实现时仍应回到官方原文核对。
3. **不复制第三方全文**：除非许可证明确允许且确有必要。
4. **版本化外部事实**：对关键依赖记录版本、commit、schema hash 或 ABI snapshot。
5. **生成物可提交**：由项目工具生成的 schema artifacts、ABI snapshot、lockfile 可以提交，但必须说明来源和生成命令。
6. **敏感信息永不提交**：token、真实 secrets、完整环境变量、认证缓存都不得进入仓库。

## 可以放入仓库的内容

- 研究报告；
- source manifest；
- ADR；
- risk register；
- AI collaboration rules；
- implementation checklist；
- 自己编写的图表和流程；
- 可复现生成脚本；
- 空 placeholder 文件。

## 不建议放入仓库的内容

- 官方文档全文；
- 第三方 README 全文；
- 网页抓取副本；
- 未经许可的长篇摘录；
- token 或私有配置；
- CLI 本地认证缓存；
- 包含真实 Linear/OpenAI/Anthropic 响应中敏感内容的 fixtures。

## 引用格式建议

在研究文档中使用：

```markdown
Source: <URL>
Purpose: <why it matters>
Checked: <YYYY-MM-DD>
```

对于关键工程事实，还应在 ADR 中记录：

```markdown
Decision:
Rationale:
Source:
Impact:
Upgrade process:
```
