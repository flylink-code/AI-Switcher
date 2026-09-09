# 验收

文案三态必须可区分：**配置已保存**（DB/档案）、**Agent 已写入**（settings.json / config.toml / opencode.json 等）、**请求已验证**（真实或模拟 HTTP）。

## 路由

| ID | 给定 | 期望 |
| --- | --- | --- |
| R1 | 明确目录 public id | `route_reason=explicit_model`，不被规划槽改写 |
| R2 | Code + 分工开 + `opus` | `role_plan` |
| R3 | 分工关 + 残留 plan/execute | 不 remap；不写 `opusplan` |
| R4 | 流式已出正文后 429 | 不切模型拼接 |
| R5 | 档案 `fallback_mode=off` | 即使全局旧 failover 开着也不跨供应商扩大 |
| R6 | 上游 URL 为本机 15821–15827 | 拒绝保存 |
| R7 | AG `127.0.0.1:15830` | 允许 |

## Agent（每项至少一条连接测试）

| ID | Agent | 检查 |
| --- | --- | --- |
| A1 | Code | external 写当前供应商；gateway 写入口 + token；分工开才 `opusplan` |
| A2 | Desktop | gateway 无 opusplan 信号不假装规划/执行；429 原样 |
| A3 | Codex | 无 opusplan；子代理头改写；备用默认不扩大 |
| A4 | OpenCode | gateway 配置仅一条受管入口 |
| A5 | Pi | 同上 |
| A6 | DSH | 同上；仅 gateway 时起 listener |
| A7 | Cline | sidecar 指向 15827 + 档案 token；用量 `target_app=cline` |

无真实凭据标 **未验证**，用内存/模拟补齐。

## UI

- 供应商页无「独立/统一」第三模式；连接选择覆盖七个 Agent。
- 设置入口文案为智能网关；路由 key 仍为 `proxy`，不新增第八主导航。
- sidebar / top / 窄窗；中英 i18n。
- 网关服务页可看最近路由：请求、档案、原始模型、命中原因、上游、尝试序号、结果。
