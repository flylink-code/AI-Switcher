# Phase 5 Preflight 预检日志 — Dashboard 概览模块

## 1. Dashboard Data Source Map (数据源映射表)

| Domain | 业务数据 (Data) | Existing Source (底层服务/API) | Cache Key / Store | Polling 策略 | Dashboard 接入策略 |
|---|---|---|---|---|---|
| **Client** | Active Client Target | `usePagePreferencesStore` | `workspaceTarget` | 无 | 完全复用 |
| **Proxy** | Proxy Running/Port | `getProxyStatus(target)` | `["proxy-status", target]` | 依赖 App Query / Cache | 完全复用（零新增 Polling） |
| **Provider** | Current Provider & Model | `useProvidersStore` / `listProviders` | `["providers", target]` | 按需 Refetch | 完全复用 |
| **Provider** | Health / Latency | `provider.healthLatencyMs` | Providers Store / Events | 事件驱动 | 完全复用 |
| **Usage** | Requests / Tokens / Cost | `getUsageDashboard("24h")` | `["usage-dashboard", "24h", source]` | 静态缓存 (StaleTime) | 完全复用 |
| **Account** | Antigravity Quota | `getAntigravityGatewayStatus()` / Store | `["antigravity-gateway"]` | 现有 5s/5min 事件 | 完全复用 |

---

## 2. Zero New Polling Rule 审计

- **原则**: 严禁在 Dashboard Page 中引入任何新的 `setInterval`、`setTimeout` 轮询或新建如 `["dashboard-proxy-status"]` 的 Query Key。
- **审计结论**: Dashboard 所需数据已 100% 被 `proxyStatusOptions`, `providerListOptions`, `usageDashboardOptions`, `antigravity-gateway` 覆盖。全量复用上述 Query Keys 即可，新增轮询数量为 **0**。

---

## 3. Partial Failure 架构设计 (局部失效保护)

Dashboard 充当跨域名的 Read Model。如果某个模块（如 Usage DB 加载超时或 Antigravity 离线）出现故障，不能导致整个 Dashboard 白屏。设计每个 Snapshot 的隔离状态：

- **Runtime Snapshot**: 支持 Ready / Loading / Error
- **Provider Snapshot**: 支持 Ready / Loading / Empty / Error
- **Usage Snapshot**: 支持 Ready / Loading / Error (局部展示 Alert，不遮蔽其他模块)
- **Account Snapshot**: 支持 Ready / N/A (未开启或无账号时不显示或显示空态)

---
*预检完成，下一步：构建 Dashboard Domain Snapshots 与 WorkbenchPage/DashboardPage 重构。*
