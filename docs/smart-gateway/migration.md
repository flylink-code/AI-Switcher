# 迁移映射

## 上游

每个旧 `providers` 行 → 同 ID 的 `upstreams` 行 + `gateway_id_map`。不按显示名合并。

每个 `target_app`：

- 每条旧供应商 → `agent_connections` `external`（`aconn_ext_{provider_id}`）
- 一条 `gateway` 连接（`aconn_gw_{target}`）指向共享档案 `gprof_shared`（首次确保时从 `gprof_claude_code` 或第一份非空槽拷贝）
- `gateway_catalog_claude_code` / `gateway_catalog_codex` 为 `true` → 该 target 的当前连接为 `gateway`，否则 `external` 指向原 `is_current`

## 档案从旧 settings 拷贝

| 旧键 | 档案字段 |
| --- | --- |
| `gateway_catalog_claude_code` / `_codex` | 连接类型（一次性） |
| `gateway_catalog_*_subagent` | `subagent_model` |
| `gateway_catalog_hide_official_*` | `hide_official` |
| `gateway_catalog_claude_code_opusplan` | 忽略（固定关，不 remap） |
| `gateway_catalog_claude_code_plan` / `_execute` | 忽略 |
| `proxy_failover_enabled=true` | 共享档案 `fallback_mode=retry`（不得悄悄变宽：false 保持 `off`） |

允许上游 = 该 target 当时全部供应商 ID。入口 token 迁移时新生成（`gwt_` + uuid）。

## 幂等

`seed_from_legacy` / `migrate_v28_to_v29` 可重复执行：已存在的档案与 token 不覆盖。

## 兼容期限

- 1.5.x：可读旧 settings 键；槽位 IPC `get/set_gateway_catalog_*` 写档案并回写旧键，便于未改完的前端。
- 禁止把 Schema 改回 28。
- 旧 ZIP 无 `providerTargets` 仍整库替换 `app.db`（导入行为不变）；导入后走 v29 迁移。
- `clear_gateway_catalog` / 恢复官方：退出 gateway 连接，回到 external 或官方。
