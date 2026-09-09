# 验收

文案三态必须可区分：**配置已保存**（DB/档案）、**Agent 已写入**（settings.json / config.toml / opencode.json 等）、**请求已验证**（真实或模拟 HTTP）。

## 路由

| ID | 给定 | 期望 |
| --- | --- | --- |
| R1 | 明确目录 public id | `route_reason=explicit_model`，不被规划槽改写 |
| R2 | Code 请求 `opus` / `sonnet` / `opusplan` | 不 remap；不写 `opusplan` |
| R3 | 残留 plan/execute 列 | 不 remap；不写 `opusplan` |
| R4 | 流式已出正文后 429 | 不切模型拼接 |
| R5 | 档案 `fallback_mode=off` | 即使全局旧 failover 开着也不跨供应商扩大 |
| R6 | 上游 URL 为本机 15821–15827 | 拒绝保存 |
| R7 | AG `127.0.0.1:15830` | 允许 |
| R8 | 改供应商 URL 后 | 上游池行不变；需再导入才更新 |

## Agent（每项至少一条连接测试）

| ID | Agent | 检查 |
| --- | --- | --- |
| A1 | Code | external 写当前供应商；gateway 写入口 + 档案 token + `ANTHROPIC_MODEL=auto`（或 Auto 卡明确 id）；settings 无 `opusplan` |
| A2 | Desktop | gateway 无 opusplan 信号不假装规划/执行；429 原样 |
| A3 | Codex | 无 opusplan；子代理头改写；备用默认不扩大 |
| A4 | OpenCode | gateway 配置仅一条受管入口 |
| A5 | Pi | 同上 |
| A6 | DSH | 同上；仅 gateway 时起 listener |
| A7 | Cline | sidecar 指向 15827 + 档案 token；用量 `target_app=cline` |

无真实凭据标 **未验证**，用内存/模拟补齐。

## UI

- 供应商页无连接类型开关；有且仅有一张托管 Auto 卡；「新增供应商」含快捷「智能网关」（无 Key 表单）；Claude Code 工作模式仍可改；**无 Opus Plan 入口**。
- 设为 Auto 当前后，Agent 只拿到本机入口，拿不到上游 Key；默认请求 `auto`；`/model` 能看到 `auto` 和目录模型。
- 智能网关是主导航第 8 项（路由 key 仍为 `proxy`）；设置旧入口跳转到该页；网关页可「启用此 Agent 的智能网关」。顶栏 Agent 只影响接入/监听/最近路由，**不换档案**。
- 网关页：上游池（导入 / 批量刷新模型 / 按模型开关）、**全局** Auto 档案槽（默认/辅助/长上下文/联网）、最近路由。改 Codex 槽后 Code 接入同一套即生效。
- 用量明细供应商旁「经智能网关」标签；可筛「仅网关」；仪表盘仍按 Agent + 实际上游聚合。
