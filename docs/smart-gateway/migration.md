# Schema 30 → 31

前向迁移，不改回旧版本号。1.5.0 尚未发版，但本地开发库已是 30。

1. 建 `gateway_bindings` / `route_modes` / `route_rules`。
2. `proxy_request_logs` 加 `correlation_id` / `hop`。
3. `upstream_models` 补元数据列；自动拉模型只填缺失。
4. 把 `gateway_profiles` 的默认 / 辅助 / 长上下文 / 联网槽搬进 `route_modes`；`plan` / `think` / `edit` / `vision` / `image_gen` 默认关闭。
5. `gprof_shared.entry_token` 拆成各 App 绑定 token。
6. 旧 `sgw_{target}` Auto 卡：`base_url` 改到 `:15828`，`api_key` 换成绑定 token，`is_current` 保持。
7. 删除 `agent_connections`。

网关配置本次不做 ZIP / WebDAV 同步（绑定 token 是凭据）。迁移资料库后按本机重新绑定。

# Schema 32 → 33

1. `gateway_bindings` 增加 `profile_id TEXT NOT NULL DEFAULT 'gprof_shared'`。
2. 允许多行 `gateway_profiles`；`gprof_shared` 仍为不可删默认档案。
3. 新档案 `target_app` 写 `shared`，不再按 Agent 建档。
4. 删除非默认档案时，绑了该档的 Agent 回落到 `gprof_shared`。

# Schema 33 → 34（统一供应商）

1. `gateway_bindings` 增加 `mode` / `direct_upstream_id`；`upstreams` 增加 `model_mapping_json`。
2. 迁移前用 SQLite backup API 备份实际资料库路径（含 WAL 已提交数据）为 `app.db.v33.bak`。既有备份不覆盖；校验失败则停止升级。
3. 将 T1/T2 旧供应商合并到全局上游。身份包含规范化端点、协议、实际 Key 哈希、认证类型和 OAuth 账号；URL path 大小写保留。跳过 Auto、自指端口与 T3 专用卡。非空 keyring 引用缺失凭据、上游写入或模型合并失败时停止迁移，事务回滚，不将缺失凭据视为空 Key 合并。
4. Code / Codex 当前独立卡转直连，Auto 保持网关，官方不绑定；旧库升级时 T2 保留已有绑定，未绑定默认建网关入口。全新库（初始 `user_version=0`）不创建这些绑定，首次启动不接管 Agent。迁移事务不写 Agent 配置，旧供应商行暂保留。
5. 迁移表保存原绑定完整凭据、档案及当前卡快照，并有完成标记防止重跑覆盖。回滚后可以重新迁移。

# Schema 34 → 35（请求尝试明细）

1. `proxy_request_logs` 增加 `attempts_json TEXT NOT NULL DEFAULT '[]'`。
2. 记录单次请求在故障降级（failover）或协议兼容重试过程中的实际尝试链，包含每跳的 `attemptIndex`、`upstreamId`、`providerName`、`model`、`statusCode`、`durationMs`、`errorCategory`、`diagnostic` 及 `success`。
3. 严格脱敏：请求正文、响应正文与认证凭据不进入数据库，错误诊断文本经 `log_redact::redact_secrets` 统一过滤敏感信息。
4. 沿用已有 `correlation_id` / `hop` 去重规则：单请求的多跳尝试结构化保存在该请求日志行的 `attempts_json` 字段内，不增加用量日志记录数，避免重复计费。
5. 幂等迁移与旧库兼容：检查 `pragma_table_info` 避免重复添加；`Database::export_rollback_v34` 继续支持从 Schema 35 / 36 资料库导出 Schema 33 降级副本。

# Schema 35 → 36（去掉动态路由表）

1. `DROP TABLE IF EXISTS route_rules`，再 `DROP TABLE IF EXISTS route_modes`。
2. 新库不再创建这两张表。`gateway_profiles` 冻结为 `gprof_shared`。`gateway_bindings.profile_id` 与日志里的 `route_mode` / `profile_id` 列保留，新写入不再填充。
3. 回滚导出仍只生成 Schema 33 副本，不降级运行库。Schema 36 的运行库可以导出。

## 降级恢复

旧版不能打开 Schema 34。`rollback_v34(destination)` IPC 只导出经过完整性校验的 Schema 33 副本，要求绝对路径且禁止覆盖；**不改变运行中的库，也不修改 Agent live 配置**。库中 Key 仍是本机 keyring 引用，不能把导出库当跨设备凭据备份。

恢复前退出新版（包括托盘进程），另存当前资料库与 Agent 配置，再选择：

- 使用迁移前的 `app.db.v33.bak`，准确恢复迁移前数据库；或
- 使用回滚导出副本，恢复旧绑定与当前卡、保留可兼容的后续数据库内容。

将副本作为旧版的 `app.db` 使用；不要在任何应用持有 SQLite 连接时替换文件，不要让旧 `-wal` / `-shm` 跟随新文件。启动旧版前须核对 Agent live 配置（降级导出不回退迁移后手动切换的配置）。新版的常规备份导入会再次迁移到 Schema 34，不能用它完成降级。
