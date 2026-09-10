# 智能网关架构

## 监听

本机三个独立服务，互不合并实现：

| 服务 | 端口 | 职责 |
| --- | --- | --- |
| 本地代理 | 15821–15827 | 协议转换 + 当前供应商故障切换 |
| 智能网关 | **15828** | 模式路由、推理挡位、模型范围、条件规则 |
| Antigravity 反代 | 15830 | 账号池 / 额度 / Cloud Code |

智能网关入口：`/health`、`/v1/models`、`/v1/messages`、`/v1/chat/completions`、`/v1/responses`、`/v1/images/generations`。转发复用 `ProxyManager` 的虚拟 `SmartGateway` 槽，不另写一套流式栈。

自引用防护：上游池禁止 15821–15828，仍允许 15830。

## 绑定

与 Antigravity `BindAppsCard` 同构：绑定某 Agent 后写入普通供应商卡 `provider_kind=smart_gateway`，`base_url` 指向 `127.0.0.1:15828`（Codex/Cline/OpenCode 带 `/v1`，Claude 系用宿主根），`api_key` 为该 App 独立 `entry_token`。

- **Claude Code / Desktop / Codex**：把 Auto 卡设为当前才走网关（与独立供应商互斥）。
- **OpenCode / Pi / DSH / Cline**：目录型多供应商，绑定只**追加**一条 Auto 入口，写出全部供应商，不设 `is_current`、不出现切换。Agent 配置里既能选直连卡，也能选 `auto` 走网关。

是否再经本地代理：Desktop 仍走 15822；Code / Codex / OpenCode / Pi / DSH / Cline（绑定 Auto）直连 15828。Cline 未绑定的独立卡仍可走 15827。

## 自定义 Agent

不必绑定。设置键 `smart_gateway_api_key`（首次读状态时生成 `sk-aisw-…`，可保存或轮换）。请求带 `Authorization: Bearer` 或 `x-api-key`。默认按 Claude Code 目录解析 `auto`；OpenAI 风格模型名加请求头 `x-ai-switcher-target: codex`。OpenAI SDK 的 Base URL 用 `http://127.0.0.1:15828/v1`，Anthropic SDK 用宿主根（不要再加 `/v1`）。仍只监听 `127.0.0.1`，不做局域网/公网。

## 路由顺序

```
客户端点名的目录模型  →  规则
  →  子代理/Haiku：只走 background（未开则 default）
  →  image_gen  →  web_search  →  vision  →  long_context
  →  plan  →  think  →  edit  →  default
```

Haiku / Explore / `x-cs-subagent` 不会被长上下文阈值或请求体里的 `thinking` 抢走。模式选出的后台模型在 `force_subagent` 归一化时保留，不再跟空的档案子代理槽或池里 `is_current` 默认。长上下文阈值 `<= 0` 不匹配；新行默认 **20000**（存量 `1` 会迁到 20000）。

`plan` 看工具清单里是否声明了 `ExitPlanMode` / `enterplanmode` / `update_plan`（Claude Code 只在 Plan 工作模式下才下发）。`edit` 看最近一轮真实调用：Anthropic 最后一条 assistant 的 `tool_use`，Codex 看 `input` 末尾一批 `function_call`（`Edit` / `Write` / `apply_patch` / `Bash` 等）。不看正文关键词，也不把 `body.tools` 目录当成改内容信号。这两个模式默认关闭。最近路由写「命中规划模式（依据：tools 含 ExitPlanMode）」或「命中改内容模式（依据：最近一轮调用了 Write）」，不写「检测到用户在做规划」。

## 服务可用性

照本地代理：`phase` / `last_error` + Tauri 事件、启动 8 次退避、更新后 `restore_after_relaunch`、`prepare_for_updater_exit` 优雅停机。端口保存前探测占用，并拒绝 15821–15827 与 15830。有绑定但网关异常时概览页提醒。
