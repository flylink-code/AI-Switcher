# Phase 6 Preflight 预检日志 — Usage Analytics 模块

## 1. Usage Data Architecture Map (数据与公式声明)

| 核心指标 / 数据 | 数据来源 (Data Source) | 计算公式 / 规则 (Protection Rule) | 过滤作用域 | 时区规则 |
|---|---|---|---|---|
| **Requests** | `summary.requestCount` | 后端 DB / session 汇总请求总数 | `period` + `logTargetApp` | 本地/UTC 一致映射 |
| **Success Rate** | `summary.successfulRequestCount` | `(successfulCount / requestCount) * 100` (%) | `period` + `logTargetApp` | - |
| **Total Tokens** | `summary` 中的 Tokens 字段 | `inputTokens` + `cacheRead` + `cacheCreation` + `outputTokens` | `period` + `logTargetApp` | - |
| **Estimated Cost** | `summary.estimatedCost` | 模型基准单价与 Token 估算 | `period` + `logTargetApp` | 标注 "Estimated Cost" |
| **Trend Series** | `dashboard.trend` | 日 / 小时粒度聚合数组 | `period` + `logTargetApp` | `trendBucketLabel` |
| **Request Logs** | `logsQuery.data` (分页) | 原始 Proxy 请求日志 / Session 导入日志 | `period` + `logPage` + `logTargetApp` | `createdAt` 转换为本地字符串 |

---

## 2. `["usage-dashboard"]` 专项审计

- **共享性对比**:
  - Phase 5 `Dashboard` 消费 `usageDashboardOptions("24h", heatmapSource)`。
  - Phase 6 `UsagePage` 消费 `usageDashboardOptions(period, logTargetApp)`。
- **结论**: 当用户在 Usage 页面选取 `period="24h"` 且 `logTargetApp` 与 Dashboard 筛选源相同时，两者完全命中相同的 Query Key `["usage-dashboard", "24h", target]`，无任何额外开销；当选择其他 period (如 7d, 30d) 时，按需拉取特定窗口数据，关系清晰互不干扰。

---

## 3. 大数据量与前端 Aggregation 性能审查

- **渲染优化**:
  - `UsageTrendChart` 内部使用 `useMemo` 对趋势与多币种汇总进行 Memoization。
  - 请求日志 `Table` 使用后端分页（`logsQuery` 按 `logPage` 分页获取 20 条/页），不将数万条历史日志一次性装载进 DOM，保证极佳渲染性能。

---
*预检完成，下一步：重构 Usage Analytics 界面与 Domain Components。*
