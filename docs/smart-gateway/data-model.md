# 数据模型（Schema 30）

前向迁移 `29 → 30` 只加列；禁止跳版本。Schema 29 引入 `upstreams` / `gateway_profiles` / `agent_connections`。

## 新表

### `upstreams`

稳定 ID、URL、协议、`provider_kind`、keyring 引用（沿用 `api_key` 列形态）、启用、模型相关缓存字段。迁移时旧 `providers` 行曾 1:1 镜像进池；`gateway_id_map.old_provider_id` 保留。同名但凭据/端点/协议不同视为不同资源，不按名称合并。

Agent 专有字段（默认模型、hidden、failover 链、Claude 角色映射）留在供应商卡或档案。**上游池与供应商卡不再双写**：编辑/删除供应商不影响池。从 Agent 显式导入时复制为新 `up_*` id；相同 `base_url + protocol` 默认跳过。旧同 id 镜像行保留为独立快照。

### `upstream_models`

每上游已声明/发现的模型 id、`verified_status`、`visible`。网关目录只收录 `visible=1`；刷新模型写入本表并同步 `upstreams.hidden_models_json`。默认模型不可关。

### `gateway_profiles`

档案：默认/辅助模型、允许上游列表、`explicit_fallback_enabled`、`fallback_mode`、`fallback_models`、辅助备用链、`hide_official`、`entry_token`、`long_context_model`、`long_context_tokens`、`web_search_model`（空=关闭该槽）。**产品只有一份档案 `gprof_shared`**；所有 Agent 的 `gateway` 连接 `profile_id` 指向它。`target_app` 仅占位。规划/执行列与 `role_routing_enabled` 留空不用（不升 Schema）。

托管 Auto 卡：`providers.provider_kind=smart_gateway`，每 Agent 至多一张，**不得**写入 `upstreams`。供应商「新增 → 智能网关」只 `ensure` + 选用，不打开 Key 表单。网关页 CRUD `upstreams`（含 AG `:15830`），并支持从 Agent 供应商导入、批量刷新模型与按模型开关。

用量：`proxy_request_logs` 带 `route_reason` / `requested_model` / `upstream_id` / `profile_id`；明细可筛「经智能网关」。记账仍按发出请求的 Agent + **路由后的上游**，不要 `target_app=smart_gateway`。

### `agent_connections`

每 Agent：`external`（指向 `upstream_id`）或 `gateway`（指向 `profile_id`），`is_current`。

### `gateway_id_map`

`old_provider_id → upstream_id`。

## `proxy_request_logs` 扩展

`profile_id`、`route_reason`、`attempt_index`、`requested_model`、`upstream_id`。用量按 attempt 一行；网关日志 `data_source=proxy`，不要与 AG 内建 `antigravity` 行双计同一语义。

## 引用检查

- 删除仍被 **当前网关档案允许列表** 使用的上游必须阻断。
- 网关连接禁止把本机网关端口（15821–15827）选为上游。
- 删除档案前须无当前 `gateway` 连接指向它（1.5.0 只保留 `gprof_shared`，不提供删除 UI）。
