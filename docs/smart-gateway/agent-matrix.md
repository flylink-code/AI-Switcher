# Agent 矩阵

绑定后各 Agent 拿模型列表的方式不同：

| Agent | 目录来源 | 绑定后刷新 | plan / edit 信号 |
| --- | --- | --- | --- |
| Claude Code | 运行时 `/v1/models`（`claude.auto`） | **需重启** | `ExitPlanMode` / `Edit` `Write` `NotebookEdit` |
| Claude Desktop | 运行时 `/v1/models` | **需重启** | 无阶段工具 → 不触发 plan/edit |
| Codex | `/v1/models` + `ai-switcher-model-catalog.json` | 目录 JSON 立即重建；CLI 仍可能要重启 | `update_plan` / `apply_patch` `shell` |
| OpenCode | 嵌入式列表，每模型 `limit.context` / `limit.output`；绑定后 **追加** Auto 入口，其它直连卡仍在 | **立即重写全部** | 无信号 |
| Pi | 嵌入式列表；绑定后追加 Auto，不切换默认供应商 | **立即重写全部** | 无信号 |
| DSH | 嵌入式列表；绑定后追加 Auto | **立即重写全部** | 无信号 |
| Cline | 嵌入式列表（含 `filter_hidden_models`）；绑定后追加 Auto，仍走本地代理 15827 | **立即重写全部** | 无信号 |
| 自定义 Agent | 运行时 `/v1/models`（公开 API Key；OpenAI 风格加 `x-ai-switcher-target: codex`） | 立刻（拉目录即可） | 无绑定；看客户端是否带阶段工具 |

上游池可见性、模式表、规则表任一变更后回调重写所有已绑定 App 的嵌入式配置。自定义 Agent 不写供应商卡，用服务卡上的访问地址 + 对外 API Key。
