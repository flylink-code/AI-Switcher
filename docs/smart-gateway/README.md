# 1.5 智能网关（设计冻结）

本目录冻结「独立智能网关服务 + 模式路由」的产品与运行语义。实施以这些文档为准；未写入的行为不得在代码里发明第三套模式。

| 文档 | 内容 |
| --- | --- |
| [architecture.md](architecture.md) | 监听布局、绑定、路由顺序、自引用防护、与本地代理 / Antigravity 的边界 |
| [data-model.md](data-model.md) | Schema 33 表、字段、用量去重、绑定令牌、多套档案 |
| [agent-matrix.md](agent-matrix.md) | 七个 Agent 的绑定写入、目录刷新与阶段信号 |
| [migration.md](migration.md) | 30→31、32→33、旧 Auto 卡改写、`agent_connections` 删除 |
| [acceptance.md](acceptance.md) | 验收用例：绑定、9 模式、去重、刷新、服务可用性 |

**不做：** 嵌入 CCR Node、合并 Antigravity 内部实现、JS 脚本路由、关键词/LLM 分类器、局域网/公网监听、把 15821–15828 列入上游。

档案全局共享，每个 Agent 的 Auto 卡选一套（Schema 33：`gateway_bindings.profile_id`）。`gprof_shared` 不可删。未绑定 / 自定义 Key 走默认档案。

CCR（MIT）只借鉴配置驱动的 `RouteDecision` 与 `off | retry | model_chain` 备用语义。
