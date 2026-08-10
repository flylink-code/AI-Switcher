# AI-Switcher UI Refactor — Phase 6 Execution

## Usage Analytics + Request Explorer

> 前置条件：Phase 0 ~ Phase 5 已完成。
>
> Phase 1：Design Tokens + Primitive UI  
> Phase 2：App Shell + Navigation Model  
> Phase 3：Providers Control Center  
> Phase 4：Proxy Control Center  
> Phase 5：Dashboard / Operational Overview
>
> Phase 5 已确认 Dashboard 新增 Polling = 0，并复用 `["proxy-status", target]`、`["providers", target]`、`["usage-dashboard"]` 等共享 Query Cache。
>
> 本阶段重构 Usage 的展示、分析与请求浏览体验，但**绝对不能改变 Usage 的统计口径与数据事实**。

---

# 0. Phase 6 核心原则

Phase 6 是：

> **Usage Analytics UI / UX Refactor**

不是：

> **Usage Accounting / Pricing / Aggregation Rewrite**

必须保持：

```text
New Usage UI
      ↓
Existing Usage Hooks / Query / State
      ↓
Existing Usage Services / IPC
      ↓
Existing Usage Storage / Backend
```

禁止为了新版图表创建第二套统计系统。

---

# 1. 总目标

将 Usage 页面重构为高信息密度的开发者分析工作台：

```text
Usage Analytics
├── Time Range / Filters
├── KPI Summary
│   ├── Requests
│   ├── Tokens
│   ├── Estimated Cost
│   └── Success Rate
├── Usage Trend
├── Token Breakdown
├── Provider / Model Breakdown
├── Request Records
└── Request Detail
```

目标：

- KPI 统计口径明确
- 时间范围明确
- 图表层级清晰
- Provider / Model 分布可理解
- Request Records 高效浏览
- Request Detail 易于诊断
- 大数据量下保持性能
- Dashboard 与 Usage 的数字不会互相矛盾
- 不增加无必要 polling
- 不修改 Cost / Token / Success 的计算规则

---

# 2. Phase 6 Preflight

修改前读取：

```text
docs/ui-refactor/phase-5-preflight.md
docs/ui-refactor/phase-5-result.md
```

以及实际：

```text
Usage Page
Usage components
Usage hooks
Usage React Query
Usage Zustand state（如果有）
Usage service
Usage IPC
Usage database/storage access
Recharts components
Dashboard UsageSnapshot
["usage-dashboard"] query source
```

创建：

```text
docs/ui-refactor/phase-6-preflight.md
```

完成后继续执行，不等待确认。

---

# 3. Usage Data Architecture Map

Preflight 必须记录：

```text
Usage Page:
Raw request source:
Summary source:
Query keys:
Refresh strategy:
Polling / refetchInterval:
Date range source:
Filter state:
Request table source:
Request detail source:
Token calculation source:
Cost calculation source:
Success-rate calculation source:
Provider grouping source:
Model grouping source:
```

必须标记：

```text
Backend-calculated
Frontend-calculated
Persisted
Derived
UI-only
```

---

# 4. `["usage-dashboard"]` 专项审计

Phase 5 已存在：

```text
["usage-dashboard"]
```

Phase 6 必须确认它究竟属于：

```text
A. 通用 Usage Summary Query
B. Dashboard 专属轻量 Query
C. Usage 主页面 Query 的共享结果
D. 与 Usage 主页面职责不同的合理独立 Query
```

不要机械合并 Query。

判断原则：

如果两个 Query：

- 数据范围不同
- 聚合粒度不同
- payload 大小明显不同
- refresh 策略合理不同

则可以保留。

如果完全请求相同数据却只是 Key 不同：

记录重复并在不改变业务语义的前提下安全去重。

---

# 5. 统计口径强制保护

以下指标必须首先找到真实现有定义：

```text
Requests
Input Tokens
Output Tokens
Cache Tokens
Total Tokens
Estimated Cost
Success Rate
Latency
```

禁止为了新版 UI：

- 改 Total Tokens 公式
- 改成功请求定义
- 改失败请求定义
- 改 Cost pricing table
- 改 currency
- 改 rounding
- 改时间边界
- 改时区语义
- 改 Provider / Model attribution

如果发现现有指标存在疑问：

记录 Deferred。

不要在 UI Phase 修 accounting。

---

# 6. Estimated Cost

Phase 5 已明确：

```text
Estimated Cost
```

带免责声明 Tooltip。

Phase 6 必须继续保持“Estimated”语义，除非后端明确提供真实 billing cost。

禁止将：

```text
Estimated Cost
```

改成：

```text
Cost
Actual Cost
Bill
```

造成错误产品承诺。

如果当前 Cost 由前端估算：

必须记录真实计算位置。

---

# 7. Time Range

Usage 页面必须清楚显示当前统计周期。

优先复用已有范围，例如：

```text
24h
7d
30d
Custom
```

实际选项以代码为准。

不要为了新版 UI 凭空增加后端不支持的范围。

---

# 8. Timezone

Preflight 必须确认：

```text
Backend timestamp timezone
Frontend parsing timezone
Chart timezone
Date filter timezone
```

禁止 UI 重构时无意改变：

```text
UTC
→ Local
```

或反向改变统计边界。

如果当前 UI 已明确使用 Local Time：

保持。

---

# 9. Filter Bar

推荐：

```text
Time Range
Provider
Model
Status
Search
Reset
```

只实现现有数据能够支持的过滤。

过滤状态属于 UI state 时，可以本地管理。

不要把 UI Filter 复制进业务 Store，除非已有跨页面持久化需求。

---

# 10. Filter Semantics

必须明确 Filter 是：

```text
Server-side
Frontend
Hybrid
```

不要出现：

> KPI 使用全量数据，而 Table 使用过滤数据，但 UI 却让用户以为 KPI 也已过滤。

如果 KPI 与 Table 过滤范围不同：

必须明确表达。

优先让同一页面的 Analytics 与 Records 使用一致 filter context，但不能因此重写后端。

---

# 11. KPI Summary

推荐顶部 4 个核心 Metric：

```text
Requests
Total Tokens
Estimated Cost
Success Rate
```

可以根据真实数据调整。

使用 Phase 1：

```text
Metric
Surface
```

不要重新建立 KPI Component System。

---

# 12. KPI Typography

保持 Desktop Tool 密度。

禁止：

```text
72px 数字
大型彩色 KPI Hero
每个 Metric 一种颜色
```

推荐：

```text
Label
Medium-large Value
Small contextual hint
```

例如：

```text
Total Tokens
1.42M
Last 24 hours
```

---

# 13. Success Rate

必须找到真实计算规则。

例如：

```text
success requests / total requests
```

但不能假设。

如果 4xx 是否算失败、cancelled 是否计入，必须沿用现有逻辑。

UI 不得自行重算另一套 Success Rate。

---

# 14. Usage Trend

使用现有：

```text
recharts
```

禁止为了 Phase 6 引入第二套 Chart Library。

趋势图可以根据真实数据展示：

```text
Requests
Tokens
Cost
```

但不要默认把所有指标塞在一个 Chart。

---

# 15. Chart Hierarchy

推荐：

```text
Primary Trend
→ Requests / Tokens over time

Secondary Breakdown
→ Token types / Provider / Model
```

避免：

```text
6 张同权图表
饼图墙
彩虹配色
```

每张图必须回答明确问题。

---

# 16. Chart Color

使用 Semantic / Chart Tokens。

建议建立少量：

```text
--chart-primary
--chart-secondary
--chart-success
--chart-danger
--chart-muted
```

如果 Phase 1 已有对应 token：

直接复用。

禁止在多个 Chart 文件中散落：

```text
#1677ff
#52c41a
#faad14
```

---

# 17. Chart Tooltip

Tooltip 应显示：

```text
Timestamp
Metric label
Formatted value
```

Cost：

```text
Estimated Cost
```

Token：

使用统一 compact formatter。

不要 Tooltip 信息过载。

---

# 18. Empty Chart

没有数据时：

不要渲染空坐标轴。

显示：

```text
No usage data for this period.
```

如果 Filter 导致为空：

显示：

```text
No usage matches the current filters.
```

---

# 19. Token Breakdown

只有真实数据区分时才展示：

```text
Input
Output
Cache Read
Cache Write
Reasoning
```

具体以现有字段为准。

不要推导不存在的 Token 类型。

---

# 20. Provider Breakdown

如果已有 Provider attribution：

可以展示：

```text
Provider
Requests
Tokens
Estimated Cost
```

推荐：

```text
compact horizontal bars
或
ranked list
```

不强制 Pie Chart。

---

# 21. Model Breakdown

如果已有 Model attribution：

可以展示：

```text
Model
Requests
Tokens
Cost
```

长 Model Name 必须：

```text
ellipsis
+
tooltip
```

---

# 22. Provider / Model Drilldown

Phase 6 不要求建立复杂 BI Drilldown Engine。

允许：

```text
点击 Provider → 设置 Filter
点击 Model → 设置 Filter
```

前提是当前过滤架构支持。

不要建立新的路由层级。

---

# 23. Request Records

Usage Analytics 下方建立 Request Explorer。

目标：

```text
Timestamp
Provider
Model
Status
Tokens
Latency
Estimated Cost
```

实际列以真实数据为准。

不要为了表格完整创建不存在字段。

---

# 24. Table Density

这是 Developer Tool。

推荐：

```text
small / compact row height
sticky header（安全时）
clear numeric alignment
monospace for IDs / endpoints when useful
```

不要使用 SaaS 大行高。

---

# 25. Column Priority

高优先级：

```text
Time
Provider / Model
Status
Tokens
Latency
Cost
```

低优先级信息可以：

```text
Request Detail
```

中展示。

不要让表格横向扩展到十几列。

---

# 26. Status

Request Status 必须使用真实状态。

可通过：

```text
StatusBadge
```

表达：

```text
Success
Failed
Cancelled
```

只有真实数据支持时使用。

HTTP Status Code 可以单独显示。

不要仅通过红绿颜色表示。

---

# 27. Request Detail

点击 Request Row 可以打开：

```text
Drawer
```

推荐：

```text
Request Summary
Timing
Token Usage
Provider / Model
Error
Metadata
```

具体以真实数据为准。

---

# 28. Request Detail 安全边界

请求详情可能包含敏感信息。

必须审计：

```text
Authorization
API Key
Bearer Token
Headers
Prompt
Response
Account identifiers
```

禁止因为“诊断方便”自动展示：

```text
Authorization header
完整 token
secret headers
```

如果现有数据已经脱敏：

保持。

如果现有数据根本没有保存：

不要新增保存机制。

---

# 29. Prompt / Response Content

如果 Usage Record 中已有 Prompt / Response：

是否展示必须遵循现有产品行为。

不要为了 Request Detail：

- 新增请求正文持久化
- 新增响应正文持久化
- 改变日志隐私策略

Phase 6 只展示已经存在且允许展示的数据。

---

# 30. Error Detail

失败请求可以展示安全：

```text
HTTP Status
Error Type
Error Message
Timestamp
Provider
```

不要直接展示 raw internal stack trace，除非现有开发模式明确如此。

---

# 31. Pagination / Virtualization

Preflight 必须检查 Usage Record 数据量。

如果数据量可能很大：

优先复用已有：

```text
pagination
backend pagination
virtual list
```

不要一次渲染数万行。

---

# 32. 不要过早引入 Virtualization Library

如果现有数据量较小或已有分页：

不要新增 virtualization dependency。

只有明确性能证据时才考虑。

本阶段原则上不升级依赖。

---

# 33. Frontend Aggregation Performance

如果图表/Metric 由前端对 records 聚合：

必须检查：

```text
reduce
groupBy
sort
filter
```

是否每次 render 重算大数组。

可以使用：

```text
useMemo
selector
existing derived query
```

优化。

但禁止改变统计结果。

---

# 34. Data Formatting

建立统一格式策略：

```text
Tokens:
1,234
12.4K
1.42M

Cost:
$0.0042
$12.84

Latency:
430 ms
1.24 s

Success:
98.4%
```

不要不同组件各自使用不同 rounding。

如果项目已有 formatter：

优先复用。

---

# 35. Cost Precision

小成本值可能需要：

```text
$0.0004
```

不要统一 `toFixed(2)` 导致显示 `$0.00`。

必须沿用或改善展示精度，但不能改变内部 Cost 数值。

---

# 36. Loading

Usage 页面各区域可以独立：

```text
KPI loading
Chart loading
Records loading
```

如果它们来自同一 Query，也可以统一 Skeleton。

不要因为 Records 较慢导致 Shell / Header 消失。

---

# 37. Partial Failure

如果 Summary 与 Records 是不同 Query：

必须允许：

```text
Summary ready
Records error
```

页面仍可用。

如果只有单一 Query：

不要为了 Partial Failure 人为拆 Query。

---

# 38. Error State

错误必须说明：

```text
Usage data unavailable
```

并在已有 refetch 能力时提供：

```text
Retry
```

禁止显示 secret / raw IPC payload。

---

# 39. Empty State

真正无 Usage：

```text
No usage recorded yet.

Usage will appear here after requests pass through AI-Switcher.
```

Filtered Empty：

```text
No requests match the current filters.
[Clear filters]
```

两者必须区分。

---

# 40. Refresh

如果当前 Usage 有手动 Refresh：

保留。

如果没有：

不要为了 UI 增加 polling。

可以使用现有 React Query：

```text
refetch
```

前提是已有 Query。

---

# 41. Dashboard Consistency

Phase 5 Dashboard 已展示：

```text
24h Requests
Total Tokens
Estimated Cost
Success Rate
```

Phase 6 必须验证：

当 Usage 页面选择相同时间范围时：

```text
Dashboard Snapshot
≈
Usage Analytics KPI
```

如果不同：

必须找到原因。

不要用 UI hack 把数字强行改成一致。

---

# 42. Dashboard Query Relationship

最终在 `phase-6-result.md` 明确：

```text
["usage-dashboard"]
```

与 Usage 主页面 Query 的关系：

```text
Shared
Derived
Independent but justified
Duplicate and removed
```

---

# 43. Domain Components

根据实际需要建立：

```text
src/components/usage/
  UsageToolbar.tsx
  UsageMetrics.tsx
  UsageTrendChart.tsx
  TokenBreakdown.tsx
  ProviderBreakdown.tsx
  RequestTable.tsx
  RequestDetailDrawer.tsx
```

不要机械创建所有组件。

---

# 44. Component Boundary

推荐：

```text
UsagePage
  orchestration

UsageToolbar
  filter UI

UsageMetrics
  KPI presentation

UsageTrendChart
  chart presentation

RequestTable
  records presentation

RequestDetailDrawer
  selected request diagnostics
```

组件不得直接绕过现有 service/hook 调 IPC。

---

# 45. Phase 1 Foundation Reuse

必须优先使用：

```text
Surface
Stack
Inline
StatusBadge
Metric
IconButton
Semantic Tokens
```

不要再建立 Usage 专用：

```text
Metric system
Status system
Spacing system
Card system
```

---

# 46. Phase 2 Foundation Reuse

继续使用：

```text
AppShell
ContextHeader
ClientSwitcher（仅页面确实 client-scoped 时）
Navigation Model
StatusBar
```

不要重新设计 Shell。

---

# 47. ContextHeader

推荐：

```text
Usage
Request and token analytics
```

如果 Usage 数据按 Client Scope：

显示现有：

```text
ClientSwitcher
```

如果 Usage 是全局聚合：

不要为了视觉一致强行显示 ClientSwitcher。

必须根据真实数据语义决定。

---

# 48. Visual Direction

目标：

> **Developer Analytics Workbench**

而不是：

> Marketing BI Dashboard

避免：

- 巨型 KPI
- 3D Chart
- Donut Chart 墙
- 彩虹图表
- 大面积 Gradient
- 装饰性数据

优先：

```text
clear metrics
useful trends
dense table
diagnostic detail
```

---

# 49. Dark Mode

必须验证：

```text
Toolbar
Date Range
Metrics
Charts
Axis
Grid
Tooltip
Legend
Table
Status
Drawer
Error
Empty
```

Chart 颜色必须在 Dark Theme 下可读。

---

# 50. Accessibility

要求：

- Chart 不作为唯一数据表达
- KPI 有文本 label
- Table keyboard 可访问
- Status 文字 + 色彩
- Drawer focus 正常
- Filter 有 label
- IconButton 有 aria-label
- Tooltip 不承载唯一关键信息

---

# 51. i18n

新增文案进入：

```text
zh-CN.json
en-US.json
```

建议：

```text
usage
```

namespace。

不要在 JSX 中大量硬编码。

---

# 52. Phase 6 文件修改范围

允许重点修改：

```text
Usage Page
Usage presentation components
components/usage
Usage CSS
Usage UI formatter
i18n
docs/ui-refactor
```

允许谨慎修改：

```text
existing Usage hooks
query selectors
pure derived helpers
```

仅用于复用、性能或 presentation。

不得改变统计结果。

---

# 53. 禁止修改范围

原则上禁止：

```text
src/services/ipc.ts
Proxy engine
Provider persistence
Antigravity logic
Rust backend
Tauri command contracts
Usage database schema
Usage accounting rules
Pricing rules
Token accounting
```

如果 UI 需要业务层改变：

记录：

```markdown
## Deferred Business Dependency
```

---

# 54. 不要顺便重构其他模块

Phase 6 不要重构：

```text
Dashboard
Providers
Proxy
Accounts
Workspace
Settings
App Shell
```

只允许修复由 Usage 接入导致的明确 regression。

---

# 55. 推荐执行顺序

## Step 1
创建 `phase-6-preflight.md`。

## Step 2
完成 Usage Data Architecture Map。

## Step 3
专项审计 `["usage-dashboard"]`。

## Step 4
锁定统计公式 / timezone / range semantics。

## Step 5
重构 Usage Page Skeleton。

## Step 6
实现 Toolbar + KPI。

## Step 7
重构 Trend / Breakdown。

## Step 8
重构 Request Records。

## Step 9
实现 Request Detail。

## Step 10
完成 Loading / Empty / Filtered Empty / Error。

## Step 11
执行大数据量性能检查。

## Step 12
验证 Dashboard 一致性。

## Step 13
执行 Build / Theme / Accessibility / Regression。

---

# 56. Usage Regression Matrix

至少验证：

| Scenario | Expected |
|---|---|
| Default range | 与旧统计语义一致 |
| Change range | KPI / Chart / Records 正确 |
| Provider filter | 过滤语义一致 |
| Model filter | 过滤语义一致 |
| Status filter | 过滤语义一致 |
| Search | 不改变原始数据 |
| No records | 正确 Empty |
| Filter no result | 正确 Filtered Empty |
| Request success | Status 正确 |
| Request failure | Status/Error 正确 |
| Cost | 保持 Estimated 语义 |
| Dashboard same range | 数值逻辑一致 |
| Dark Mode | Chart/Table 可读 |
| Large dataset | 无明显 UI 卡顿 |

---

# 57. Data Accuracy Verification

必须抽样至少若干真实 Records，核对：

```text
Request count
Input token
Output token
Total token
Cost
Status
Latency
Provider
Model
Timestamp
```

新版 UI 展示必须与现有原始数据一致。

不要只看截图判断正确。

---

# 58. Performance Verification

记录：

```text
Records loaded:
Pagination:
Virtualization:
Aggregation location:
Memoization:
Largest chart dataset:
Duplicate query:
New polling:
```

目标：

```text
New polling: 0
```

除非现有 Usage 本身已有合理 polling，Phase 6 可以继续复用。

---

# 59. Build Verification

执行项目实际存在的：

```bash
npm run typecheck
npm run build
```

若存在：

```bash
npm run lint
npm run test
```

也执行。

不得伪造结果。

---

# 60. Manual Visual Verification

至少验证：

```text
Light
Dark

No Usage
Small Dataset
Large Dataset

Default Range
Other Existing Ranges

Provider Filter
Model Filter
Status Filter
Search

Long Model Name
Long Provider Name

Success Request
Failed Request

Request Detail
Cost Precision
Chart Tooltip

Compact Window
Normal Window
Large Window
```

无法安全模拟：

```text
Not manually simulated
```

---

# 61. Phase 6 Deliverables

完成后创建：

```text
docs/ui-refactor/phase-6-result.md
```

必须包含：

## A. Changed Files

```text
File
Purpose
Risk
```

## B. Usage Architecture

```text
UsagePage
Toolbar
Metrics
Charts
Records
Request Detail
Data Sources
```

## C. Accounting Protection

明确：

```text
Request calculation:
Token calculation:
Cost calculation:
Success calculation:
Timezone:
Range semantics:
```

以及是否发生变化。

期望：

```text
Behavior changed: No
```

## D. Query Architecture

说明：

```text
Usage main query:
Dashboard usage query:
Relationship:
Polling:
Duplicate query:
```

## E. Performance

```text
Records:
Pagination:
Aggregation:
Memoization:
New dependency:
New polling:
```

## F. Security

说明 Request Detail 对：

```text
API keys
Authorization
Headers
Prompt/Response
Error
```

的处理。

## G. Verification

```text
TypeScript:
Build:
Lint:
Tests:
Light:
Dark:
Filters:
Charts:
Records:
Request Detail:
Dashboard consistency:
Large dataset:
```

## H. Deferred Issues

记录所有 Phase 6 不应解决的问题。

---

# 62. Phase 7 Accounts / Antigravity Readiness

Phase 6 完成后，对 Accounts / Antigravity 做只读审计。

回答：

1. 当前 Antigravity / Accounts 页面由哪些文件组成？
2. Account Pool 数据来源是什么？
3. Active Account 来源是什么？
4. Quota 数据来源是什么？
5. 45 秒首次刷新具体在哪里实现？
6. 5 分钟事件轮询具体在哪里实现？
7. 是否还有其他 account/quota polling？
8. OAuth 登录流程由哪些 UI / Service / IPC 文件组成？
9. Account Rotation 的业务逻辑在哪里？
10. Account 切换 / 删除 / 新增分别走哪些 handler？
11. Quota reset / refresh 信息如何表达？
12. Dashboard 是否已经复用相同 quota 数据源？
13. 哪些 Account UI 可以安全重构？
14. 哪些 Antigravity 逻辑绝对不能修改？
15. Phase 7 推荐修改哪些文件？
16. 是否存在重复 refresh / event listener 风险？

只做 Readiness。

不要在 Phase 6 提前重构 Accounts。

---

# 63. Git / Diff Discipline

保持 Phase 6 Diff 聚焦。

禁止：

```text
全项目格式化
无关文件重命名
重写 Usage backend
修改 database schema
修改 pricing
修改 token accounting
重构 Accounts
修改 Proxy
升级 dependencies
```

无关问题记录：

```markdown
## Deferred Issues
```

---

# 64. Phase 6 成功标准

用户应该能快速回答：

```text
这段时间请求了多少次？
→ Requests

用了多少 Token？
→ Token Metrics

大约花了多少？
→ Estimated Cost

成功率如何？
→ Success Rate

什么时候使用最多？
→ Usage Trend

主要用了哪个 Provider / Model？
→ Breakdown

哪一个请求失败了？
→ Request Records

为什么失败？
→ Request Detail
```

技术层必须保持：

```text
New Usage UI
      ↓
Existing Usage Facts
      ↓
Existing Query / Hooks
      ↓
Existing Services / Storage
```

而不是：

```text
New Usage UI
├── New Accounting
├── New Pricing
├── New Success Formula
├── New Polling
└── Existing Usage
```

---

# 65. 最终执行指令

现在执行 Phase 6。

必须遵守：

1. 先创建 `docs/ui-refactor/phase-6-preflight.md`
2. 完成 Usage Data Architecture Map
3. 专项审计 `["usage-dashboard"]` 与 Usage 主 Query 的关系
4. 锁定 Request / Token / Cost / Success / Timezone / Range 统计语义
5. Preflight 完成后继续实施，不等待确认
6. Usage UI 可以明显重构
7. Usage accounting / pricing / token 计算禁止重写
8. 不创建第二套 Usage 事实源
9. 新增 polling 目标为 0
10. 继续使用 Recharts，不引入第二套 Chart Library
11. Request Detail 不得扩大敏感数据采集
12. 不新增 Prompt / Response 持久化
13. Dashboard 与 Usage 相同周期的数字必须进行一致性验证
14. 优先消费 Phase 1 Primitives
15. 优先消费 Phase 2 App Shell
16. 不顺便重构 Dashboard / Providers / Proxy / Accounts
17. 完成大数据量性能检查
18. 完成后创建 `docs/ui-refactor/phase-6-result.md`
19. 输出 Phase 7 Accounts / Antigravity Readiness

**最高优先级：让 Usage 成为可信、紧凑、高效的 Developer Analytics Workbench，同时保证新版 UI 展示的是既有 Usage 事实，而不是重新定义 Usage 事实。**
