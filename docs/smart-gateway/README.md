# 1.5 智能网关（设计冻结）

本目录冻结「统一目录模式」替换为「托管 Auto 供应商 + 智能网关页」的产品与运行语义。实施以这些文档为准；未写入的行为不得在代码里发明第三套模式。

| 文档 | 内容 |
| --- | --- |
| [architecture.md](architecture.md) | 监听布局、路由顺序、自引用防护、与 CCR / Antigravity 的边界 |
| [data-model.md](data-model.md) | Schema 30 表、字段、导入池、删除阻断 |
| [agent-matrix.md](agent-matrix.md) | 七个 Agent 的 external / gateway 写入与信号能力 |
| [migration.md](migration.md) | 旧 settings 键、1:1 上游映射、兼容期限 |
| [acceptance.md](acceptance.md) | 验收用例与「配置已保存 / Agent 已写入 / 请求已验证」 |

**不做：** 嵌入 CCR Node、合并 Antigravity 内部实现、脚本路由、关键词/LLM 分类器、把网关自身列入上游。

CCR（MIT）只借鉴 `RouteDecision` / `RouteExecutionPlan` 与 `off | retry | model-chain` 备用语义。若将来有实质源码复用，须保留 MIT 版权声明。
