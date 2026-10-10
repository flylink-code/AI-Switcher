# 统一供应商与 Agent 连接（Schema 34）

供应商页维护唯一的全局 `upstreams`，不再按 Agent 重复配置 Key。智能网关 `127.0.0.1:15828` 按聚合目录里的公开模型 id 转到对应上游，不再做动态路由模式或 `claude.auto`。

## 连接模式

- **智能网关（推荐）**：Agent 指向 `127.0.0.1:15828`，使用自己的入口 token 和所选档案；真实供应商凭据只由网关读取。
- **直连（高级）**：选择一个全局上游，运行时投影成该 Agent 的配置，不在 `providers` 里复制供应商。Code 只允许原生 Anthropic，Codex 只允许 OpenAI Chat / Responses。需要协议转换或 Codex OAuth 的上游应走智能网关。新直连不启动 15821 / 15823。
- **官方**：恢复原生配置，删除该 Agent 的连接绑定，清理托管目录；不会移除用户自有配置项。

档案选择只更新已有 `mode=gateway` 的绑定，直连或未绑定时禁用。编辑路由、档案、目录不得把直连 Agent 写回 15828。

T2（OpenCode / Pi / Cline）在网关模式只写 Auto 一个托管入口，直连只写所选上游。切换时清理原有托管项，保留用户自有项。Pi 默认供应商跟随选用入口。OpenCode 仍写 `limit.context` 和 `limit.output`。T3（Desktop / DSH）保留后端与旧资料，UI 不展示。

## 数据结构

- `gateway_bindings.mode`：`gateway` 或 `direct`。
- `gateway_bindings.direct_upstream_id`：直连上游 ID，网关模式为空。
- `upstreams.model_mapping_json`：保留 Claude 角色模型映射。
- `upstream_migration_v34`：记录旧卡、目标 Agent、映射上游、原当前卡状态与原绑定，供数据回滚核查。

直连 token 不允许作为网关入站凭据。`provider_from_upstream` 返回内存投影，ID 仍是上游 ID，凭据通过 keyring 引用读取。API Key 不明文落 SQLite，不随 IPC/JSON 导出。

## 升级与兼容

迁移前备份实际数据库路径，保留旧 `providers` 行。迁移只改数据库，不写 Agent 配置：

- 当前独立 Code / Codex 卡映射为直连；当前 Auto 保持网关；官方登录不创建连接。
- T2 已绑定的保持绑定，未绑定的建立网关入口。
- 跳过 Auto、自指 15821–15828、T3 专用卡。
- 同端点不同 Key 或不同 OAuth 账号不能合并；每张成功迁移旧卡都有 `gateway_id_map`。

升级前依赖本地代理转换协议的旧 T1 卡保留当前行与原配置，以维持已有链路。新显式直连不再建立这种链路；需要转换时手动切换智能网关。

## 导入与导出

JSON 导出为不含 API Key / keyring 引用 / OAuth 账号标识的元数据，并过滤名称含认证、Key、token、cookie 等敏感标记的自定义请求头；用户填写的名称、URL、备注仍需自行检查。JSON/live 导入可刷新已绑定网关的目录，不改变直连或官方连接。Code / Codex / OpenCode 可从当前配置读取上游，自动跳过托管网关地址；Pi / Cline 暂不支持磁盘读取导入。

降级只导出副本，不降级运行库。详见 [迁移与降级恢复](migration.md)。

## 验证

- L0：Schema 34 迁移、去重、token 模式、只读查询、删除引用保护与备份。
- L1：Code / Codex 直连写出、官方恢复、失败回滚、路由不夺取直连、T2 Auto 唯一入口。
- L2：`SG-providers-pool-crud`、`SG-agent-connection-card`、`SG-p0-catalog-bind-appends-auto`。连接卡场景目前验证 IPC 状态机，不等同 DOM 交互测试。

实际验证结果以执行日志为准，不以场景存在或命令已启动视为通过。
