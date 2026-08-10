# AI-Switcher UI Refactor — Phase 5 Execution

## Dashboard / Overview + Cross-Domain Read Model

> 前置条件：Phase 0 ~ Phase 4 已完成。
>
> Phase 1：Design Tokens + Primitive UI  
> Phase 2：App Shell + Navigation Model  
> Phase 3：Providers Control Center  
> Phase 4：Proxy Control Center + Resilience
>
> 本阶段开始建设 Dashboard，但 Dashboard 必须是现有业务状态的“聚合观察层”，不是新的业务服务层。

---

# 0. Phase 5 核心原则

Phase 5 是：

> **Dashboard Read Model + Overview Experience**

不是：

> **Dashboard Business Service / New Polling Layer**

必须保持：

```text
Dashboard UI
   ↓
Existing Shared Hooks / React Query Cache / Zustand
   ↓
Existing Services
   ↓
Existing IPC
```

禁止形成：

```text
Dashboard
├── dashboardService.ts
├── new proxy polling
├── new provider polling
├── new usage polling
└── new quota polling
```

---

# 1. 总目标

建立 AI-Switcher 的默认 Overview 页面，让用户打开应用后快速回答：

```text
当前管理哪个 Client？
Proxy 是否正在运行？
当前 Provider 是谁？
Provider 是否正常？
最近用了多少？
当前额度情况如何？
下一步最常用操作在哪里？
```

推荐结构：

```text
Dashboard
├── Runtime Overview
├── Current Client
├── Provider Snapshot
├── Usage Snapshot
├── Account / Quota Snapshot
└── Quick Actions
```

Dashboard 应：

- 高信息密度
- 低操作复杂度
- 数据来源可信
- 支持部分数据不可用
- 不阻塞整个页面
- 不创建新的业务状态源
- 不重复 Phase 3 / Phase 4 完整页面

---

# 2. Phase 5 Preflight

修改前读取：

```text
docs/ui-refactor/phase-2-result.md
docs/ui-refactor/phase-3-result.md
docs/ui-refactor/phase-4-preflight.md
docs/ui-refactor/phase-4-result.md
```

并检查真实：

```text
Dashboard / Home page
pageRegistry
App Shell
ContextHeader
ClientSwitcher
StatusBar

Provider hooks/store/query
Proxy hooks/query
Usage hooks/query
Antigravity hooks/query/store

ProviderCard
ProxyRuntimeCard
Metric
StatusBadge
Surface
```

创建：

```text
docs/ui-refactor/phase-5-preflight.md
```

完成后继续实施，不等待确认。

---

# 3. Dashboard Data Source Map

Preflight 必须输出：

| Domain | Data | Existing Source | Cache Key / Store | Polling | Dashboard Strategy |
|---|---|---|---|---|---|
| Client | Active Client | ... | ... | ... | reuse |
| Proxy | Runtime | ... | ... | ... | reuse |
| Provider | Current Provider | ... | ... | ... | reuse |
| Provider | Health / Latency | ... | ... | ... | reuse / unavailable |
| Usage | Requests | ... | ... | ... | reuse |
| Usage | Tokens | ... | ... | ... | reuse |
| Usage | Cost | ... | ... | ... | reuse |
| Account | Quota | ... | ... | ... | reuse / unavailable |

必须以真实代码为准。

---

# 4. Zero New Polling Rule

这是 Phase 5 的硬约束。

Phase 4 已确认：

```text
ProxyPage
+
StatusBar
```

共享：

```text
["proxy-status", target]
```

Dashboard 必须继续消费同一 Query Cache / Hook。

禁止为了 Dashboard 创建：

```text
["dashboard-proxy-status"]
["dashboard-provider-health"]
setInterval(...)
new Tauri listener
```

如果现有数据刷新频率不足：

先记录 Deferred。

不要因为 Dashboard 增加第二套 polling。

---

# 5. Dashboard 是 Read Model

Dashboard 的主要职责：

```text
Read
Summarize
Navigate
```

而不是：

```text
Configure
Mutate complex settings
Manage failover
Edit provider
Edit account
```

允许少量安全 Quick Actions。

复杂操作必须导航到对应 Domain 页面。

---

# 6. Dashboard 页面结构

推荐：

```text
ContextHeader
└── Dashboard

Content
├── Runtime Overview
├── Primary Snapshots
│   ├── Provider
│   └── Usage
├── Secondary Snapshots
│   └── Account / Quota
└── Quick Actions
```

不要再建立重复 Client Tabs。

Current Client 继续由 Phase 2 ClientSwitcher 单一状态源驱动。

---

# 7. ContextHeader

推荐：

```text
Dashboard                    [Claude Code ▾]
Overview of your current AI-Switcher environment
```

Dashboard 默认不需要大量 Header Actions。

避免：

```text
Add Provider
Start Proxy
Import
Export
Settings
```

全部堆到 Header。

---

# 8. Runtime Overview

Dashboard 顶部应该优先回答：

> 系统现在是否可用？

推荐紧凑 Surface：

```text
Runtime

● Proxy Running
Port 15821

Active Provider
OpenRouter

Endpoint
http://127.0.0.1:15821
```

根据真实数据调整。

---

# 9. 不直接复用完整 Proxy Hero

Phase 4 已建立：

```text
ProxyRuntimeCard
```

它属于 Proxy Control Center。

Dashboard 不建议直接复制完整 Hero Card。

应该：

```text
复用状态来源
复用 StatusBadge
复用视觉语言
```

但建立更轻量 Snapshot。

例如：

```text
RuntimeSnapshot
```

只有在真正复用价值明确时才创建组件。

---

# 10. Runtime Actions

Dashboard 可以提供有限：

```text
Open Proxy
```

是否提供：

```text
Start / Stop
```

必须谨慎。

默认推荐：

> Dashboard 只显示状态 + “Open Proxy”。

原因：

Proxy Start / Stop 属于 Runtime Control Center 的核心操作。

如果现有产品明确需要一键启动，可保留，但必须复用 Phase 4 handler，不得实现第二套。

---

# 11. Current Client

Client Context 已由 ContextHeader 表达。

Dashboard Content 不需要再放大型：

```text
Current Client: Claude Code
```

Card。

如果对 Snapshot 解释有帮助，可以使用小型 contextual label。

避免重复信息。

---

# 12. Provider Snapshot

推荐展示：

```text
Current Provider
OpenRouter

Model
anthropic/claude-sonnet-4

Health
Healthy

Latency
430 ms

[Manage Providers]
```

字段只在真实数据存在时显示。

---

# 13. Provider Health / Latency

Phase 3 已出现：

```text
health
latency
```

Phase 5 必须复用相同数据来源。

禁止：

```text
Dashboard 自己 ping provider
Dashboard 新建 health query
Dashboard 新建 latency timer
```

如果当前 Client 没有健康数据：

显示：

```text
Unknown
```

或不显示。

不要伪造 Healthy。

---

# 14. Provider Snapshot 不等于 ProviderCard

不要直接把完整：

```text
ProviderCard
```

塞进 Dashboard。

ProviderCard 是管理组件。

Dashboard 需要的是摘要。

推荐：

```text
ProviderSnapshot
```

或者使用：

```text
Surface + Metric + StatusBadge
```

组合即可。

---

# 15. Usage Snapshot

Usage 是 Dashboard 的高价值摘要。

根据现有数据能力考虑：

```text
Requests
Input Tokens
Output Tokens
Total Tokens
Estimated Cost
```

但 Dashboard 不应复制完整 Usage Analytics。

推荐只展示 3~4 个核心 Metric。

---

# 16. Usage Time Range

必须先确认 Usage 当前真实统计周期。

Dashboard 不允许模糊显示：

```text
Requests 128
```

却不说明周期。

应该明确：

```text
Today
Last 24h
Current Session
This Month
```

具体依据真实 Usage 数据。

禁止为了 Dashboard 自己计算一个与 Usage 页面不同的周期。

---

# 17. Usage Metric

优先使用 Phase 1：

```text
Metric
```

例如：

```text
Requests
128

Tokens
1.42M

Estimated Cost
$12.84
```

不要使用巨型 KPI Typography。

这是 Desktop Tool，不是 Investor Dashboard。

---

# 18. Usage Chart

Dashboard 可以有非常轻量趋势图，但不是 Phase 5 必需项。

只有满足：

```text
已有 Usage 时间序列
无需新 Query
无需新 Aggregation Service
```

时才允许加入。

否则：

只使用 Metric。

不要为了首页漂亮而创建新的 Usage 数据管道。

---

# 19. Usage Navigation

Usage Snapshot 提供：

```text
View Usage
```

跳转现有 Usage 一级导航。

必须继续使用 Phase 2 Navigation Model。

不要直接操纵不存在的 Router URL。

---

# 20. Account / Quota Snapshot

如果当前 Client / Account 有真实 Quota 数据：

展示轻量：

```text
Account
user@example.com

Quota
72%

Reset
2h 14m

[Manage Accounts]
```

实际字段以 Antigravity 数据为准。

---

# 21. Antigravity 强制保护

禁止修改：

```text
Account Pool
Account Rotation
Quota Refresh
45s first refresh
5min event polling
OAuth
Token behavior
```

Dashboard 只能消费已有状态。

绝对禁止为了 Dashboard 新增：

```text
quota interval
account refresh timer
OAuth refresh
```

---

# 22. Quota Visual

Quota 可以使用：

```text
Progress
+
numeric value
```

颜色必须克制。

例如：

```text
Normal
Warning
Critical
```

只有已有业务阈值时才使用语义状态。

禁止自行定义：

```text
< 20% = red
```

然后暗示这是业务错误。

若无正式阈值：

使用 neutral progress + numeric value。

---

# 23. 无 Account / Quota 数据

Dashboard 必须允许：

```text
No account
Unsupported client
Quota unavailable
Loading
Error
```

不要让 Account 模块错误导致整个 Dashboard Error Boundary。

---

# 24. Partial Failure Architecture

Dashboard 聚合多个 Domain。

必须避免：

```text
Usage API error
→ 整个 Dashboard 白屏
```

每个 Snapshot 应独立处理：

```text
Loading
Ready
Unavailable
Error
```

例如：

```text
Runtime ✓
Provider ✓
Usage Error
Quota N/A
```

页面仍然可用。

---

# 25. Dashboard Loading

禁止全页面等待所有 Query 完成后才显示。

应该：

```text
Shell immediately
Header immediately
Each Snapshot independently loading
```

可使用：

```text
Skeleton
Metric skeleton
Surface placeholder
```

---

# 26. Dashboard Error

错误应该 Domain scoped。

例如：

```text
Usage data unavailable
[Open Usage]
```

不要展示 raw stack trace。

不要把敏感 Provider / Token 信息写进错误 UI。

---

# 27. Quick Actions

推荐：

```text
Manage Providers
Open Proxy
View Usage
Manage Accounts
```

如果 Workspace 已可访问，也可：

```text
Open Workspace
```

不要做 10+ 个 Quick Actions。

---

# 28. Quick Action 原则

Quick Action 主要负责：

```text
Navigate
```

而不是执行复杂 mutation。

优先：

```text
Manage Providers
```

而不是：

```text
Delete Provider
```

优先：

```text
Open Proxy
```

而不是：

```text
Reset Failover
```

---

# 29. Navigation

所有 Quick Actions 必须使用现有：

```text
activeKey
pageRegistry
Phase 2 navigation mapping
```

禁止引入 Router。

如果已有统一 navigation helper：

复用。

不要在 Dashboard 创建第二套 navigation map。

---

# 30. Layout

推荐 Desktop Layout：

```text
┌──────────────── Runtime ────────────────┐
│                                        │
└────────────────────────────────────────┘

┌──────── Provider ───────┐ ┌── Usage ──┐
│                         │ │           │
└─────────────────────────┘ └───────────┘

┌──────── Quota ──────────┐ ┌─ Actions ─┐
│                         │ │           │
└─────────────────────────┘ └───────────┘
```

具体根据真实数据量调整。

---

# 31. Compact Window

窗口较窄：

```text
Runtime
Provider
Usage
Quota
Actions
```

依次单列。

必须避免：

```text
horizontal overflow
tiny cards
metric text clipping
```

---

# 32. Large Window

不要因为屏幕变宽无限拉伸 Card。

可以使用：

```text
max-width
grid
controlled columns
```

保持可读性。

---

# 33. Dashboard Domain Components

按真实复用需求考虑：

```text
src/components/dashboard/
  RuntimeSnapshot.tsx
  ProviderSnapshot.tsx
  UsageSnapshot.tsx
  QuotaSnapshot.tsx
  QuickActions.tsx
```

不要机械创建所有文件。

简单区域可以直接由 Dashboard Page 使用 Phase 1 Primitives 组合。

---

# 34. 不建立 Universal Dashboard Card

禁止：

```tsx
<DashboardCard
  type="provider"
  variant="usage"
  quotaMode
  runtimeMode
  ...
/>
```

优先：

```text
Primitive
+
Small Domain Snapshot
```

---

# 35. Phase 1 Foundation Reuse

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

不要创建 Dashboard 专用：

```text
spacing system
color system
status system
metric system
```

---

# 36. Phase 2 Foundation Reuse

必须复用：

```text
AppShell
ContextHeader
ClientSwitcher
StatusBar
Navigation Model
```

不要重新设计 Shell。

---

# 37. Phase 3 / Phase 4 Pattern Reuse

复用：

```text
Provider Identity
Provider status language
Proxy runtime status language
```

但不强制复用完整管理组件。

Dashboard 是摘要，不是管理页面嵌套。

---

# 38. Visual Direction

目标：

> **Operational Overview**

不是：

> Analytics Marketing Dashboard

避免：

- 巨型数字
- 彩色渐变 KPI
- 饼图堆叠
- 每个模块不同颜色
- 大量装饰图标
- Hero Banner

优先：

```text
neutral surfaces
clear typography
compact metrics
subtle status
consistent borders
```

---

# 39. Dark Mode

必须验证：

```text
Runtime Snapshot
Provider Snapshot
Usage Metrics
Quota
Quick Actions
Loading
Error
Unavailable
```

全部使用 Semantic Tokens / antd Theme。

---

# 40. Accessibility

要求：

- Status 文字 + 色彩
- Quick Actions keyboard accessible
- Metric label 清晰
- Progress 有 accessible value
- Error 文本可读
- Icon-only action 有 aria-label

---

# 41. i18n

Dashboard 新文案进入现有：

```text
zh-CN.json
en-US.json
```

建议 namespace：

```text
dashboard
```

不要在 JSX 大量硬编码双语字符串。

---

# 42. Performance

Dashboard 聚合多个 Domain 时检查：

```text
unnecessary rerender
duplicate query
duplicate polling
expensive transformation
large usage dataset
```

不要为了显示三个 Metric：

把完整数万条 Usage Record 每次 render 全量 reduce。

如果已有统计结果：

优先复用。

如果只能前端计算：

使用已有数据 + 合理 memoization。

不要新增 backend aggregation，除非后续独立 Phase 决定。

---

# 43. Sensitive Data

Dashboard 不应展示：

```text
Full API Key
Bearer Token
OAuth Token
Sensitive Headers
```

Provider Endpoint 可以显示。

Account Email 是否显示：

遵循现有产品行为。

---

# 44. Phase 5 文件修改范围

允许重点修改：

```text
Dashboard/Home page
components/dashboard
Dashboard CSS
navigation metadata（必要时）
i18n
docs/ui-refactor
```

允许轻微修改：

```text
shared hooks
query selector
UI-only helper
```

前提是：

只为了安全复用现有数据，不改变业务行为。

---

# 45. 禁止修改范围

原则上禁止：

```text
src/services/ipc.ts
src/services/proxy.ts
src/services/providers.ts
src/services/antigravity.ts
Rust backend
Tauri commands
Proxy engine
Failover
Provider persistence
Account rotation
OAuth
```

不要新建：

```text
dashboardService.ts
dashboardPolling.ts
dashboardStore.ts
```

除非只是纯 UI preference；业务数据禁止复制。

---

# 46. 不要顺便重构其他页面

Phase 5 不要重构：

```text
Providers
Proxy
Usage
Accounts
Workspace
Settings
App Shell
```

只允许修复由 Dashboard 接入暴露出的明确 regression。

其他问题进入：

```markdown
## Deferred Issues
```

---

# 47. 推荐执行顺序

## Step 1
创建 `phase-5-preflight.md`。

## Step 2
完成 Dashboard Data Source Map。

## Step 3
完成 Query / Polling / Event Listener 审计。

## Step 4
建立 Dashboard 页面骨架。

## Step 5
接入 Runtime Snapshot。

## Step 6
接入 Provider Snapshot。

## Step 7
接入 Usage Snapshot。

## Step 8
接入 Account / Quota Snapshot。

## Step 9
加入 Quick Actions。

## Step 10
完成 Partial Loading / Error / N/A。

## Step 11
验证 Compact / Normal / Large Window。

## Step 12
执行 TypeScript / Build / Regression。

---

# 48. Dashboard Regression Matrix

至少验证：

| Scenario | Expected |
|---|---|
| Proxy Running | Runtime 正确 |
| Proxy Stopped | Runtime 正确 |
| Provider changed | Snapshot 跟随现有状态 |
| Client changed | Dashboard 上下文正确 |
| Usage loading | 仅 Usage 区域 loading |
| Usage error | 其他模块仍可用 |
| No quota | Quota 显示 N/A/Empty |
| Quota loading | 不阻塞其他区域 |
| Dark Mode | 全部可读 |
| Compact Window | 单列且无横向溢出 |
| Quick Actions | 导航到正确 Legacy Page |

---

# 49. Polling Audit

完成后必须记录：

```text
Proxy query:
Provider health query:
Latency query:
Usage query:
Quota refresh:
Tauri listeners:
New polling added by Phase 5:
```

目标：

```text
New polling added by Phase 5: 0
```

如果不是 0：

必须解释为什么现有共享机制无法满足。

---

# 50. Build Verification

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

# 51. Manual Visual Verification

至少验证：

```text
Light
Dark

Claude Code
Claude Desktop
Codex
OpenCode

Proxy Running
Proxy Stopped

Provider available
Provider unavailable/unknown（如果真实支持）

Usage ready
Usage loading
Usage error

Quota ready
Quota unavailable

Compact Window
Normal Window
Large Window
```

无法安全模拟的状态记录：

```text
Not manually simulated
```

---

# 52. Phase 5 Deliverables

完成后创建：

```text
docs/ui-refactor/phase-5-result.md
```

必须包含：

## A. Changed Files

```text
File
Purpose
Risk
```

## B. Dashboard Architecture

```text
DashboardPage
Runtime Snapshot
Provider Snapshot
Usage Snapshot
Quota Snapshot
Quick Actions
```

## C. Data Sources

明确每个 Snapshot 使用：

```text
Hook
Query Key
Store
Selector
```

## D. Zero Polling Audit

记录：

```text
New polling:
New interval:
New event listener:
Duplicate query:
```

## E. Partial Failure

说明每个 Domain 如何独立处理：

```text
Loading
Error
Unavailable
Ready
```

## F. Verification

```text
TypeScript:
Build:
Lint:
Tests:
Light:
Dark:
Client switching:
Proxy state:
Provider state:
Usage:
Quota:
Quick Actions:
Compact Window:
```

## G. Deferred Issues

记录 Phase 5 不应解决的问题。

---

# 53. Phase 6 Usage Analytics Readiness

Phase 5 完成后，对 Usage 页面做只读审计。

回答：

1. 当前 Usage 页面由哪些文件组成？
2. Usage 原始数据来自哪里？
3. React Query Key 是什么？
4. 是否存在本地数据库 / IPC 查询？
5. 当前统计周期有哪些？
6. Request / Token / Cost 如何计算？
7. Cost 是否为后端值还是前端估算？
8. Recharts 当前有哪些图表？
9. Request Detail/Table 数据源是什么？
10. Filter / Search / Date Range 当前如何实现？
11. 是否存在大数据量 render 性能问题？
12. Dashboard Usage Snapshot 是否复用了同一数据源？
13. Phase 6 哪些 UI 可以安全重构？
14. 哪些 Usage 计算逻辑绝对不能修改？
15. Phase 6 推荐修改哪些文件？

只做 Readiness。

不要在 Phase 5 提前重构 Usage Analytics。

---

# 54. Git / Diff Discipline

保持 Phase 5 Diff 聚焦。

禁止：

```text
全项目格式化
无关文件重命名
重构 services
新增 Dashboard backend
新增 polling
重构 Usage
重构 Accounts
修改 Rust
升级 dependencies
```

无关问题记录到：

```markdown
## Deferred Issues
```

---

# 55. Phase 5 成功标准

Dashboard 完成后，用户打开 AI-Switcher 应能在几秒内知道：

```text
Which client?
→ ContextHeader

Is proxy running?
→ Runtime Snapshot

Which provider?
→ Provider Snapshot

How much have I used?
→ Usage Snapshot

How much quota remains?
→ Quota Snapshot

Where do I manage things?
→ Quick Actions
```

技术层必须保持：

```text
Dashboard
   ↓
Shared Read Models / Existing Hooks
   ↓
Existing Query Cache / Zustand
   ↓
Existing Services / IPC
```

而不是：

```text
Dashboard
├── new service
├── new polling
├── duplicated provider state
├── duplicated proxy state
├── duplicated quota state
└── duplicated usage aggregation
```

---

# 56. 最终执行指令

现在执行 Phase 5。

必须遵守：

1. 先创建 `docs/ui-refactor/phase-5-preflight.md`
2. 建立完整 Dashboard Data Source Map
3. 完成 Query / Polling / Event Listener 审计
4. Preflight 完成后继续实施，不等待确认
5. Dashboard 必须作为 Read Model
6. Phase 5 新增 polling 数量目标为 0
7. Proxy 必须复用 `["proxy-status", target]` 或项目真实共享来源
8. Provider health / latency 必须复用 Phase 3 现有来源
9. Usage 必须复用现有 Usage 数据源
10. Quota 必须复用现有 Antigravity refresh 机制
11. 不建立 `dashboardService`
12. 不建立第二套业务 Store
13. 不修改 Proxy / Provider / Antigravity 核心业务
14. Dashboard 各模块必须支持 Partial Failure
15. Quick Actions 优先导航，不执行复杂 mutation
16. 优先消费 Phase 1 Primitives
17. 优先消费 Phase 2 App Shell / Navigation
18. 复用 Phase 3 / 4 的状态语言与视觉模式
19. 完成后创建 `docs/ui-refactor/phase-5-result.md`
20. 输出 Phase 6 Usage Analytics Readiness

**最高优先级：建立一个快速、可信、低噪音的 Operational Overview，让 Dashboard 聚合现有系统事实，而不是创造一套新的系统事实。**
