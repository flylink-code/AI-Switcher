# 智能网关数据模型（Schema 31）

一套全局档案 `gprof_shared`。不再使用 `agent_connections`。

## 新表

- `gateway_bindings(target_app PK, entry_token, provider_id, created_at)`
- `route_modes(id, profile_id, enabled, model, thinking_config_json, fallback_models_json, threshold, sort_index)` — `id` 为 9 个模式名
- `route_rules(id, profile_id, enabled, sort_index, rule_type, condition_json, pattern, target_model, thinking_config_json, rewrites_json)`

## 增列

- `proxy_request_logs.correlation_id` / `hop`（`agent_proxy` / `smart_gateway` / `antigravity`）
- `upstream_models.display_name` / `context_window` / `max_output_tokens` / `reasoning_levels_json` / `capabilities_json`

## 用量去重

第一跳生成 `x-aisw-request-id` / `x-aisw-target-app` 并向下游传播。同一 `correlation_id` 只把最内层跳计入 token 与花费；外层行仍可见，标为中转。无 `correlation_id` 的历史行走原逻辑。

## 规则

`model-prefix` 与 `condition`（左值白名单：token 数 / thinking / web_search / vision / 工具名 / 绑定 App / 路径）。rewrite 只开白名单键，鉴权头一律禁改。不做 JS 脚本。
