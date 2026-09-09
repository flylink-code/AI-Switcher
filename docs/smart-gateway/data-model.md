# 数据模型（Schema 29）

前向迁移 `28 → 29`，禁止跳版本，禁止把 `user_version` 改回 28。

## 新表

### `upstreams`

稳定 ID、URL、协议、`provider_kind`、keyring 引用（沿用 `api_key` 列形态）、启用、模型相关缓存字段。旧 `providers` 行 **1:1** 映射；`gateway_id_map.old_provider_id` 保留。同名但凭据/端点/协议不同视为不同资源，不按名称合并。

Agent 专有字段（默认模型、hidden、failover 链、Claude 角色映射）进连接或档案，不提升为全局。1.5.0 仍双写 `providers` 行以便现有 apply/配额代码工作；`upstreams` 是共享上游镜像。

### `upstream_models`

每上游已声明/发现的模型 id、`verified_status`、`visible`。

### `gateway_profiles`

档案：默认/规划/执行/辅助模型、允许上游列表、`role_routing_enabled`、`explicit_fallback_enabled`、`fallback_mode`、`fallback_models`、角色备用链、`hide_official`、`entry_token`。

### `agent_connections`

每 Agent：`external`（指向 `upstream_id`）或 `gateway`（指向 `profile_id`），`is_current`。

### `gateway_id_map`

`old_provider_id → upstream_id`。

## `proxy_request_logs` 扩展

`profile_id`、`route_reason`、`attempt_index`、`requested_model`、`upstream_id`。用量按 attempt 一行；网关日志 `data_source=proxy`，不要与 AG 内建 `antigravity` 行双计同一语义。

## 引用检查

- 删除仍被 **当前网关档案允许列表** 使用的上游必须阻断。
- 网关连接禁止把本机网关端口（15821–15827）选为上游。
- 删除档案前须无当前 `gateway` 连接指向它（1.5.0 默认每 Agent 一个档案，不提供删除 UI）。
