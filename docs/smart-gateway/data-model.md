# 智能网关数据模型（Schema 33）

一套或多套全局档案。默认档案 `gprof_shared` 不可删除。每个 Agent 通过 `gateway_bindings.profile_id` 选用其中一套；未绑定与自定义 Key（`sk-aisw-…`）回落 `gprof_shared`。不再按 Agent 建档。`agent_connections` 已删除。

## 新表

- `gateway_bindings(target_app PK, entry_token, provider_id, created_at, profile_id)` — Schema 33 增加 `profile_id TEXT NOT NULL DEFAULT 'gprof_shared'`
- `route_modes(id, profile_id, enabled, model, thinking_config_json, fallback_models_json, threshold, sort_index)` — `id` 为 9 个模式名
- `route_rules(id, profile_id, enabled, sort_index, rule_type, condition_json, pattern, target_model, thinking_config_json, rewrites_json)`

克隆档案会复制 `hide_official` / allowlist / 9 个模式 / 规则，**不**复制 `entry_token`（令牌仍在 binding 上）。删除非默认档案时，绑了该档的 Agent 回落到 `gprof_shared`。

## 增列

- `proxy_request_logs.correlation_id` / `hop`（`agent_proxy` / `smart_gateway` / `antigravity`）
- `upstream_models.display_name` / `context_window` / `max_output_tokens` / `reasoning_levels_json` / `capabilities_json`

## 用量去重

第一跳生成 `x-aisw-request-id` / `x-aisw-target-app` 并向下游传播。同一 `correlation_id` 只把最内层跳计入 token 与花费；外层行仍可见，标为中转。无 `correlation_id` 的历史行走原逻辑。

## 规则

`model-prefix` 与 `condition`（左值白名单：token 数 / thinking / web_search / vision / 工具名 / 绑定 App / 路径）。rewrite 只开白名单键，鉴权头一律禁改。不做 JS 脚本。
