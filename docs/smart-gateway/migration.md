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
