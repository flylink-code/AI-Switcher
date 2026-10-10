# Agent 矩阵

绑定后各 Agent 拿模型列表的方式不同：

| Agent | 目录来源 | 绑定后刷新 | 默认模型 |
| --- | --- | --- | --- |
| Claude Code | 运行时 `/v1/models`（已选模型或目录第一项） | **需重启** | 已选模型，否则目录第一项 |
| Claude Desktop | 运行时 `/v1/models` | **需重启** | 已选模型，否则目录第一项 |
| Codex | `/v1/models` + `ai-switcher-model-catalog.json` | 目录 JSON 立即重建；CLI 仍可能要重启 | 已选模型，否则目录第一项 |
| OpenCode | 嵌入式列表，每模型 `limit.context` / `limit.output`；网关只写所选目录模型，直连只写所选全局上游 | **立即重写托管项** | 已选模型，否则目录第一项 |
| Pi | 嵌入式列表；网关写所选目录模型，默认供应商跟随入口；直连只写所选上游 | **立即重写托管项** | 已选模型，否则目录第一项 |
| DSH | 嵌入式列表；绑定后写所选目录模型 | **立即重写全部** | 已选模型，否则目录第一项 |
| Cline | 嵌入式列表（含 `filter_hidden_models`）；sidecar 只写所选目录模型或所选直连上游，网关直连 15828 | **立即重写托管项** | 已选模型，否则目录第一项 |
| 自定义 Agent | 运行时 `/v1/models`（公开 API Key；OpenAI 风格加 `x-ai-switcher-target: codex`） | 立刻（拉目录即可） | 客户端自填聚合目录 id |

上游池可见性变更后回调重写所有已绑定 App 的嵌入式配置。自定义 Agent 不写供应商卡，用服务卡上的访问地址 + 对外 API Key，模型填聚合目录 id。
