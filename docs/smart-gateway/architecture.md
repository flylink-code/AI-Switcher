# 智能网关架构

## 监听

本机三个独立服务，互不合并实现：

| 服务 | 端口 | 职责 |
| --- | --- | --- |
| 本地代理 | 15821–15827 | 协议转换 + 当前供应商故障切换 |
| 智能网关 | **15828** | 显式目录 id → 上游、限额、健康检查、故障转移 |
| Antigravity 反代 | 15830 | 账号池 / 额度 / Cloud Code |

智能网关入口：`/health`、`/v1/models`、`/v1/messages`、`/v1/chat/completions`、`/v1/responses`、`/v1/images/generations`。转发复用 `ProxyManager` 的虚拟 `SmartGateway` 槽，不另写一套流式栈。

自引用防护：上游池禁止 15821–15828，仍允许 15830。

## 绑定

与 Antigravity `BindAppsCard` 同构：绑定某 Agent 后写入普通供应商卡 `provider_kind=smart_gateway`，`base_url` 指向 `127.0.0.1:15828`（Codex/Cline/OpenCode 带 `/v1`，Claude 系用宿主根），`api_key` 为该 App 独立 `entry_token`。

- **Claude Code / Desktop / Codex**：绑定 `mode=gateway` 且 Auto 卡为当前才走网关；`mode=direct` 不允许 token 入站。
- **OpenCode / Pi / Cline**：网关模式只写 Auto，直连只写所选全局上游，官方清理托管入口；保留用户自有配置。
- **DSH**：产品面隐藏，后端保留旧多供应商目录与追加 Auto 行为。

是否再经本地代理：Desktop 仍走 15822；Code / Codex / OpenCode / Pi / DSH / Cline（网关模式）直连 15828。新直连不启动协议转换代理；旧 T1 转换链路迁移时保留。

## 自定义 Agent

不必绑定。设置键 `smart_gateway_api_key`（首次读状态时生成 `sk-aisw-…`，可保存或轮换）。请求带 `Authorization: Bearer` 或 `x-api-key`。默认按 Claude Code 目录解析 `auto`；OpenAI 风格模型名加请求头 `x-ai-switcher-target: codex`。OpenAI SDK 的 Base URL 用 `http://127.0.0.1:15828/v1`，Anthropic SDK 用宿主根（不要再加 `/v1`）。仍只监听 `127.0.0.1`，不做局域网/公网。

## 路由顺序

```
客户端点名的目录模型  →  规则
  →  子代理/Haiku：只走 background（未开则 default）
  →  image_gen  →  web_search  →  vision
  →  plan  →  think  →  edit  →  long_context  →  default
```

Haiku / Explore / `x-cs-subagent` 不会被长上下文阈值或请求体里的 `thinking` 抢走。模式选出的后台模型在 `force_subagent` 归一化时保留，不再跟空的档案子代理槽或池里 `is_current` 默认。长上下文阈值 `<= 0` 不匹配；新行默认 **60000**，不改写存量 20000（存量 `1` 会修复到默认值）。

`plan` 看工具清单里是否声明了 `ExitPlanMode` / `enterplanmode` / `update_plan`（Claude Code 只在 Plan 工作模式下才下发）。`edit` 看最近一轮真实调用：Anthropic 最后一条 assistant 的 `tool_use`，Codex 看 `input` 末尾一批 `function_call`（`Edit` / `Write` / `apply_patch` / `Bash` 等）。不看正文关键词，也不把 `body.tools` 目录当成改内容信号。这两个模式默认关闭。最近路由写「命中规划模式（依据：tools 含 ExitPlanMode）」或「命中改内容模式（依据：最近一轮调用了 Write）」，不写「检测到用户在做规划」。

## 备用与健康门禁

- 模式 `fallback_models` 是显式备用链，不受档案 `off` / `retry` 开关抑制；档案级模型链仍要求 `model_chain`。公开目录 ID 必须解析成实际上游和模型 slug，保留档案 allowlist。显式 pin 禁止切换；模式链去重后最多主模型加两个备用。
- Code/Codex 共用出站观察器和内存健康表。认证失败冷却 30 分钟，额度不足 1 小时，明确模型不存在只冷却对应上游的该模型 1 小时；普通 404 不冷却。连续瞬时失败触发熔断，到期只放行一个半开请求，取消时释放探针。
- 429 优先遵守 `Retry-After`（秒、小数或 HTTP 日期）；缺省从 5 秒指数退避至 300 秒，不截短服务器声明的长等待。短冷却最多等待 10 秒，长冷却不出网，返回 429 与剩余等待时间或交给可用备用。
- 目录探测 401/403 显示凭据失效；404/405 只代表地址可达、能力未知。目录探测成功不能解除真实请求的熔断；手动对话测试成功可恢复对应模型和上游。
- 默认通用重试码为 408、429、500–599，不覆盖用户存量配置；模型不存在可走显式链。AG 429/504 仅允许用户配置的显式备用跨供应商，Kiro 429/504 仍只在其账号池处理。反代网络失败不冷却整个账号池入口。
- 仅在向客户端提交响应前故障切换。重试等待不提前发送 SSE keepalive；中途断流继续沿用原有错误收尾，不启动备用。健康错误字段只保留安全分类描述，不保存原始响应正文或凭据。
- 档案编辑按档案 ID 保存，不隐式绑定 Agent，也不把 direct 连接切回 Auto。后续 UI 和请求尝试日志见 [优化路线图](roadmap.md)。

## 路由试跑（Simulate）

- **真实一致性**：试跑复用完整请求信号解析、日预算限额门禁、档案白名单（allowlist）、条件规则与模式推导，以及执行计划构造（`RouteExecutionPlan`）。
- **健康快照与备用**：只读检查当前内存健康快照（`health::is_available`、`min_cooldown_remaining_secs` 与 `lookup`）。首选上游正常时选中首选并将备用候选置为待命；首选处于冷却、熔断或认证失败时，自动沿显式备用链尝试下一个健康候选并解释切换原因。显式锁定（`explicit_pinned`）时抑制备用切换；全部候选冷却时解释无可用上游。
- **只读与安全性保证**：试跑严格只读，不产生网络出网，不调用 `acquire_permit`（不抢占半开探测 permit），不写入代理用量日志或请求记录。API Key、请求正文及敏感错误原文不进入诊断步骤与候选信息。

## 上游准入与首输出（第四批，验收中）

- 每个实际上游的策略保存为 `settings.smart_gateway_upstream_policy:<id>` JSON，与资料库备份一同保存；不是供应商额度，也不写入 Agent 配置。默认并发/RPM 为 0（不限），队列容量 16、等待 8000ms、首输出截止 0（关闭）。
- `Database` 持有共享限流实例，Code/Codex 按上游 ID 共用并发与严格 60 秒滑动 RPM。FIFO 准入先原子预留，健康门禁通过后才提交实际出站计数；取消排队/未出站释放预留。配置重载保留在途计数，删除唤醒等待者。
- 入口许可持有至客户端响应体结束，出站许可持有至对应上游正文释放。响应体取消通过异步任务写 `cancelled`，不修改健康；流式终态只写一次，已知错误不被 `complete` 覆盖。响应头前取消会带上当时的供应商、路由和尝试链。Anthropic `message_stop` 与 Chat 完成帧同样只写一次。
- 子代理上游继承默认关闭，设置键 `smart_gateway_subagent_inherit_upstream`。只在已识别子代理且带 `x-cs-parent-session-id` 时，于显式目录和规则锁定之后，在已选模型的合法候选里偏好父会话最近成功的上游。成功请求写入最终上游；子代理不回写父槽。该头不转发给外部供应商。试跑不创建、不刷新这条记录。
- 首输出截止覆盖实际出站后的 headers 与 SSE 预读，不包含排队；只认文本、thinking/reasoning、工具调用/参数或正常空完成。预读解析上限 256KiB，完整帧增量检测并按原字节回放；超限、协议错误、异常 EOF 与超时分别分类。
- 截止前不向客户端提交成功响应；一旦响应提交，不再切换。取消本地读取不保证撤销供应商计算、计费或远程工具副作用。AG/Kiro 的既有备用保护不变。
- 试跑展示只读准入压力快照，不排队、不预留 RPM，不保证执行时仍能准入。

## 日志刷新与终态展示

复用 `usage-log-recorded` 合并通知；各页面共享一个事件监听器，保留轮询兜底。隐藏页面不触发查询，恢复可见补查；在途期间保留尾随事件，外部查询在途时等待后补查。日志页自动刷新开关同时控制事件与轮询，详情离页显示快照提示。拓扑仅在新日志或终态指纹变化时短暂动画，不模拟实时传输，遵循 reduced-motion。

## 服务可用性

照本地代理：`phase` / `last_error` + Tauri 事件、启动 8 次退避、更新后 `restore_after_relaunch`、`prepare_for_updater_exit` 优雅停机。端口保存前探测占用，并拒绝 15821–15827 与 15830。有绑定但网关异常时概览页提醒。
