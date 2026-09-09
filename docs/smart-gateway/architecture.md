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

与 Antigravity `BindAppsCard` 同构：绑定某 Agent 后写入普通供应商卡 `provider_kind=smart_gateway`，`base_url` 指向 `127.0.0.1:15828`（Codex/Cline/OpenCode 带 `/v1`，Claude 系用宿主根），`api_key` 为该 App 独立 `entry_token`。设为当前才走网关。

是否再经本地代理：Desktop / Cline 仍走本地代理；Code / Codex / OpenCode / Pi / DSH 直连 15828。

## 路由顺序

```
客户端点名的目录模型  →  规则
  →  image_gen  →  web_search  →  vision  →  long_context
  →  background  →  plan  →  think  →  edit  →  default
```

`plan` / `edit` 只看工具清单（Claude Code：`ExitPlanMode` / `Edit`/`Write`；Codex：`update_plan` / `apply_patch`/`shell`），不看正文关键词。这两个模式默认关闭。最近路由写「命中规划模式（依据：tools 含 ExitPlanMode）」，不写「检测到用户在做规划」。

## 服务可用性

照本地代理：`phase` / `last_error` + Tauri 事件、启动 8 次退避、更新后 `restore_after_relaunch`、`prepare_for_updater_exit` 优雅停机。端口保存前探测占用，并拒绝 15821–15827 与 15830。有绑定但网关异常时概览页提醒。
