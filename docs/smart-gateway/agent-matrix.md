# Agent 支持矩阵（1.5.0 必须逐项）

| Agent | external | gateway | 阶段信号 | 备注 |
| --- | --- | --- | --- | --- |
| Claude Code | 直连或现有协议代理 | 明确模型 + 辅助 / 长上下文 / 联网；默认 `ANTHROPIC_MODEL=claude.auto` | haiku / subagent | 无 opusplan；入口 token 鉴权；发现 id 必须含 claude/anthropic |
| Claude Desktop | 不变（Haiku 探测、429 原样） | 明确模型 + 辅助槽；默认模型 `claude.auto` | 无 opusplan | 入口 token 鉴权 |
| Codex | 不变 | 明确模型 + 子代理；默认 `auto` | 无 opusplan | 目录写档案 token，不写 `PROXY_MANAGED` |
| OpenCode | 写入全部供应商、无当前激活 | 配置里 **只出现一条** 受管入口 | 无 | 仅 gateway 时起 15824 |
| Pi | 同上（多供应商写入） | 一条受管入口 | 无 | 15825 |
| DSH | 同上 | 一条受管入口 | 无 | 仅 gateway 时起 15826 |
| Cline | 仍走 15827 Responses | 同样按档案入口 | 无 | 用量筛 `target_app=cline` |

无凭据的真实上游调用标为未验证，用模拟集成测试补齐。

## 写入规则

- gateway：把托管 Auto 卡设为当前（OpenCode/Pi/DSH 也可在模型列表里选它）。Agent 只拿到本机入口 URL + **共享档案** token，拿不到全部上游密钥。
- Claude Code：写出 `ANTHROPIC_AUTH_TOKEN` 与 `ANTHROPIC_API_KEY` = `gprof_shared.entry_token`（禁止 `local-proxy-code` / `kr://`），默认 `ANTHROPIC_MODEL=claude.auto`（Auto 卡选了明确目录 id 则写该 id）。不写 `model=opusplan`。
- Codex 目录模式：`auth.json` / `experimental_bearer_token` 写档案 token；独立供应商本地代理仍可写 `PROXY_MANAGED`（监听器不校验档案 token）。
- WSL 同步、MCP/权限字段合并、失败可回滚：沿用现有原子写。
- 失败文案区分：配置已保存 / Agent 已写入 / 请求已验证。
