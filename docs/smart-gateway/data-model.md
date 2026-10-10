# 智能网关数据模型（Schema 36）

档案冻结为默认 `gprof_shared`（allowlist / token 仍在其上），界面不再编辑档案。未绑定与自定义 Key（`sk-aisw-…`）回落 `gprof_shared`。`agent_connections` 已删除。Schema 36 删除 `route_modes` / `route_rules`；`gateway_bindings.profile_id` 与 `proxy_request_logs.route_mode` / `profile_id` 列保留给历史和回滚，新写入不再填充。

## 新表

- `gateway_bindings(target_app PK, entry_token, provider_id, created_at, profile_id, mode, direct_upstream_id)` — Schema 33 增加 `profile_id`，Schema 34 增加连接模式与直连上游；直连 token 拒绝网关入站
- `upstreams.model_mapping_json` — 全局上游保存角色模型映射
- `upstream_migration_v34` — 保留旧卡映射与原连接状态，详见 [统一供应商](unified-providers.md)
`route_modes` / `route_rules` 只存在于 Schema 36 之前的库，升级时删除。备用模型在 `gateway_profiles.fallback_models_json`，且 `fallback_mode` 为 `model_chain` 时才进入尝试链。

## 增列

- `proxy_request_logs.correlation_id` / `hop`（`agent_proxy` / `smart_gateway` / `antigravity`）
- `proxy_request_logs.attempts_json`（Schema 35：存储请求实际尝试明细，含上游、模型、耗时、状态码、脱敏失败分类与最终结果，默认 `'[]'`）
- `upstream_models.display_name` / `context_window` / `max_output_tokens` / `reasoning_levels_json` / `capabilities_json`

## 用量去重

第一跳生成 `x-aisw-request-id` / `x-aisw-target-app` 并向下游传播。同一 `correlation_id` 只把最内层跳计入 token 与花费；外层行仍可见，标为中转。无 `correlation_id` 的历史行走原逻辑。

## 解析

请求模型是聚合目录里的公开 id 时，转到该 id 所属上游，出网仍用上游 slug。`auto` / `claude.auto` / 空模型落到目录第一项。显式 pin 不切换上游。
