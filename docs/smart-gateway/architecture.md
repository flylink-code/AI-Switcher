# 架构

## 目标形态

```
Agent → AgentConnection
  ├─ external：固定上游（直连或现有协议适配）
  └─ gateway：档案入口 token → 明确模型 → 角色（仅档案开启分工）→ 档案默认
       → RouteDecision + ExecutionPlan
       → 现有 proxy 适配器
       → 官方 / 第三方 / 反代 API（含内建 AG :15830）
       → 日志 / 用量 / 路由观测
```

删除「独立 / 统一」双模式。Agent 只选 **外部供应商连接** 或 **网关档案连接**。不新增第三套平行模式。规划/执行/辅助与备用链只属于档案。

## 监听布局

共享引擎（现 `ProxyManager`，语义上为 GatewayManager）+ **按 Agent 本机 loopback 端口**（不强制单端口）：

| Agent | 默认端口 | 协议面 |
| --- | ---: | --- |
| Claude Code | 15821 | Anthropic `/v1/messages`、`/v1/models` |
| Claude Desktop | 15822 | 同上（含 Desktop 前缀路径） |
| Codex | 15823 | Chat + Responses + `/v1/models` |
| OpenCode | 15824 | Chat + Responses + `/v1/messages`（仅 gateway 连接时启动） |
| Pi | 15825 | Anthropic `/v1/messages` |
| DSH | 15826 | Chat + Responses + `/v1/messages`（仅 gateway 连接时启动） |
| Cline | 15827 | Responses（已有 sidecar） |

兼容期内继续认旧 `proxy_port_*`。OpenCode / DSH **仅当该 Agent 绑了网关连接**才起 listener，并在其原生配置里只写 **一条** 受管入口（本机 URL + 档案 token），不得把全部上游密钥写进 Agent 配置。

档案识别用 **每档案入口 token**（写入该 Agent 的 API key），不靠 User-Agent。

Antigravity 内建反代仍是 **15830 普通上游**。「添加到网关上游」只写 `http://127.0.0.1:15830` 类连接，不联动启停 AG。网关端口 15821–15827 禁止自引用；15830 允许。

## 路由顺序（比 CCR 窄）

1. 鉴权档案（入口 token）
2. 明确目录 public id（且落在档案允许上游内）→ 记 `explicit_model`
3. 可验证角色（**仅** `role_routing_enabled`）→ `role_plan` / `role_execute` / `role_subagent`
4. 档案默认 → `profile_default`

明确模型是否允许备用是档案开关（`explicit_fallback_enabled`），默认关。规划/执行/辅助各有备用链，且不得越出允许上游。已输出流式正文后禁止切模型拼接。

记录「命中规划槽」不得写成「检测到 Plan 阶段」。关闭分工后残留槽位无效：不写 `model=opusplan`、运行时不 remap、界面不展示为生效。

## 备用（有界）

档案 `fallback_mode`：`off` | `retry` | `model_chain`。最多 3 跳。网关请求 **不** 再隐式使用全局 `proxy_failover_enabled` 去跨供应商扩大；仅当迁移时该旧值显式为 true 才把档案设为 `retry`。

## 保留为适配器

`proxy/convert.rs`、Codex Chat/Responses/Anthropic、Moonshot `$ref`、web_search/web_fetch、thinking 映射、现有超时/429 语义、AG `provider_kind=antigravity`。

## 删除的运行路径（P5）

以「统一目录开关」驱动的双轨 apply 作为产品模式消失；gateway 连接仍通过本机入口 + 当前供应商作为写盘载体。旧 `get/set_gateway_catalog_*` 仅作读旧键 / 写档案的兼容层，新 UI 走 `get/set_agent_connection` 与 `update_gateway_profile`。
