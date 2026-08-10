# AI-Switcher UI Refactor — Phase 7 Execution

## Accounts / Antigravity Account Pool + Quota Experience

> 前置条件：Phase 0 ~ Phase 6 已完成。
>
> Phase 1：Design Tokens + Primitive UI  
> Phase 2：App Shell + Navigation Model  
> Phase 3：Providers Control Center  
> Phase 4：Proxy Control Center  
> Phase 5：Dashboard / Operational Overview  
> Phase 6：Usage Analytics / Request Explorer
>
> 本阶段重构 Accounts / Antigravity 的账户池、额度与账号状态体验。
>
> **UI 可以明显变化，但 Account Rotation、OAuth、Token、Quota Refresh、定时刷新与事件监听机制必须保持现有业务语义。**

---

# 0. Phase 7 核心原则

Phase 7 是：

> **Accounts / Antigravity UI / UX Refactor**

不是：

> **Account Pool Runtime / OAuth / Rotation Rewrite**

必须保持：

```text
New Accounts UI
        ↓
Existing Account Hooks / Query / Store
        ↓
Existing Antigravity Services
        ↓
Existing IPC
        ↓
Existing Backend Account Pool
```

禁止形成：

```text
New Accounts UI
├── New Account Pool
├── New Rotation Logic
├── New Quota Polling
├── New OAuth State Machine
└── Existing Antigravity Runtime
```

---

# 1. 总目标

将 Accounts / Antigravity 页面重构为清晰的 Account Pool Control Center：

```text
Accounts
├── Pool Overview
│   ├── Active Account
│   ├── Available Accounts
│   ├── Pool Status
│   └── Quota Summary
│
├── Account Pool
│   └── Account Card / Row
│       ├── Identity
│       ├── Status
│       ├── Quota
│       ├── Reset Time
│       └── Actions
│
├── Rotation
│   ├── Rotation Status
│   └── Existing Rotation Controls
│
├── Account Actions
│   ├── Add / OAuth Login
│   ├── Switch
│   ├── Refresh Quota
│   └── Remove
│
└── States
    ├── Loading
    ├── Empty
    ├── Expired
    ├── Disabled
    ├── Error
    └── Quota Unavailable
```

---

# 2. Phase 7 Preflight

开始修改前读取：

```text
docs/ui-refactor/phase-5-result.md
docs/ui-refactor/phase-6-preflight.md
docs/ui-refactor/phase-6-result.md
```

以及 Phase 6 输出的：

```text
Phase 7 Accounts / Antigravity Readiness
```

检查实际：

```text
Accounts / Antigravity Page
Account components
Account hooks
Account Zustand stores
Account React Query queries
Antigravity service
OAuth service / handlers
Quota refresh logic
Rotation logic
Dashboard quota/account snapshot
Tauri event listeners
Timers / intervals
```

创建：

```text
docs/ui-refactor/phase-7-preflight.md
```

完成 Preflight 后继续实施，不等待确认。

---

# 3. Account Architecture Map

Preflight 必须记录：

```text
Accounts Page:
Account list source:
Active account source:
Quota source:
Quota reset source:
Rotation state source:
Rotation configuration source:
OAuth entry:
OAuth callback handling:
Add account handler:
Remove account handler:
Switch account handler:
Manual refresh handler:
Automatic refresh mechanism:
45s initial refresh:
5min event polling:
Tauri event listeners:
```

每项标记：

```text
UI-only
React Query
Zustand
Service
IPC
Backend
Runtime-critical
```

---

# 4. Refresh / Polling / Event 专项审计

这是 Phase 7 的最高优先级 Preflight 项之一。

必须搜索并记录：

```text
setInterval
setTimeout
refetchInterval
invalidateQueries
Tauri listen
event listener
quota refresh
account refresh
```

尤其确认现有：

```text
45s 首次刷新
5min 事件轮询
```

真实实现位置和生命周期。

禁止仅根据之前文档猜测。

---

# 5. 45s 首刷保护

如果真实代码确认存在：

```text
45s initial quota refresh
```

必须保持：

- 启动时机
- 触发条件
- Query invalidation
- Account scope
- cleanup
- error behavior

UI 重构不得：

```text
45s → 30s
45s → immediate
45s → every 45s
```

除非现有业务本身就是如此。

---

# 6. 5min 事件轮询保护

如果真实代码确认存在：

```text
5min event polling
```

必须保持其真实语义。

需要明确它究竟是：

```text
quota polling
account event polling
backend event polling
query refresh
```

禁止把所有“5 分钟刷新”概念混为一谈。

---

# 7. Zero Duplicate Refresh

Dashboard 可能已经展示 Account / Quota Snapshot。

Phase 7 必须确认：

```text
Dashboard
+
Accounts Page
```

是否消费相同 Account / Quota 状态源。

目标：

```text
Duplicate quota polling added by Phase 7: 0
```

禁止新增：

```text
accounts-page-quota-interval
dashboard-quota-interval
account-card-refresh-timer
```

多个独立 Timer。

---

# 8. Dashboard Quota Snapshot 专项审计

必须记录：

```text
Dashboard quota source:
Accounts page quota source:
Same query/store?
Same refresh mechanism?
Duplicate fetch?
```

如果 Dashboard 当前没有 Quota Snapshot：

只记录。

不要为了 Phase 7 顺便重构 Dashboard。

---

# 9. 强制业务保护区

原则上禁止修改：

```text
src/services/antigravity.ts
src/services/ipc.ts
```

以及实际承担以下职责的文件：

```text
OAuth protocol
Token refresh
Account rotation
Quota scheduler
Account persistence
```

除非完全不可避免，而且只能做无业务行为变化的 UI integration。

---

# 10. 禁止改变的业务行为

禁止修改：

- OAuth 登录协议
- OAuth callback
- Access Token
- Refresh Token
- Token persistence
- Account identity
- Account Pool persistence
- Active Account selection algorithm
- Auto Rotation algorithm
- Quota refresh timing
- Quota reset calculation
- Retry behavior
- Account failover
- Backend event protocol
- IPC contracts

---

# 11. Accounts 页面信息架构

推荐：

```text
ContextHeader
├── Accounts
├── Description
└── Add Account

Content
├── Pool Overview
├── Account Pool
└── Rotation / Advanced
```

不要重复大型 Page Header。

---

# 12. ContextHeader

推荐：

```text
Accounts                                  [+ Add Account]
Manage Antigravity accounts and quota
```

如果页面真实名称是：

```text
Antigravity
```

则遵循当前产品命名。

不要仅为了规划强制改名。

---

# 13. Pool Overview

页面顶部建立轻量 Overview：

```text
Account Pool

Active Account
user@example.com

Available
3 / 4

Quota
72%

Rotation
Automatic
```

实际指标以真实数据为准。

不要创建业务层不存在的：

```text
Pool Health Score
Account Reliability Score
```

---

# 14. Overview 不做巨型 Hero

Phase 7 不需要类似营销 Dashboard 的 Hero。

推荐：

```text
compact summary surface
+
small metrics
+
status
```

避免：

- 巨型 Account Avatar
- 巨型 72%
- 大面积 Gradient
- 彩色账户卡墙

---

# 15. Account Pool

Account Pool 是页面主体。

推荐：

```text
Active Account
──────────────
Account A

Other Accounts
──────────────
Account B
Account C
Account D
```

如果现有 Pool 有明确业务排序：

保持。

不要 UI 自动按 quota / latency / email 排序。

---

# 16. Account Card

建议建立：

```text
src/components/accounts/AccountCard.tsx
```

或者项目现有命名：

```text
src/components/antigravity/...
```

遵循真实目录结构。

推荐：

```text
┌───────────────────────────────────────────────┐
│ [Avatar] user@example.com          [ACTIVE] │
│          Account label / identity            │
│                                               │
│ Quota                    Reset                │
│ 72%                      2h 14m               │
│ █████████████░░░░                             │
│                                               │
│ Last updated 2m ago          Refresh   •••   │
└───────────────────────────────────────────────┘
```

只展示真实数据。

---

# 17. Account Identity

优先显示已有：

```text
Email
Display Name
Account ID（必要时）
```

不要暴露：

```text
Access Token
Refresh Token
Authorization Header
```

Account ID 如果很长：

可截断。

---

# 18. Active Account

Active Account 必须明确。

推荐：

```text
StatusBadge: Active
+
subtle brand border
```

不要整卡高饱和。

如果 Active Account 是 Rotation Runtime 自动决定：

必须使用真实 Runtime 状态。

禁止 UI 自己推导。

---

# 19. Account Status

只展示真实可判断状态。

可能：

```text
Active
Available
Disabled
Expired
Error
Quota Exhausted
Unknown
```

实际以代码为准。

禁止为了视觉完整凭空创建状态。

---

# 20. StatusBadge

优先使用 Phase 1：

```text
StatusBadge
```

颜色不能作为唯一表达。

例如：

```text
Expired
```

必须有文字。

---

# 21. Quota

Quota 是 Account Card 的核心信息。

根据真实数据展示：

```text
remaining
used
percentage
reset time
```

不要自行转换业务含义。

例如后端提供：

```text
remaining = 72%
```

不要误标成：

```text
Used 72%
```

---

# 22. Quota Progress

可以使用：

```text
antd Progress
```

或现有 Primitive。

必须配 numeric value。

不要只显示进度条。

---

# 23. Quota Threshold

禁止自行定义业务阈值，例如：

```text
<20% = Critical
```

除非现有业务已有明确 threshold。

如果没有：

使用 neutral progress。

如果已有状态：

直接消费已有：

```text
warning
critical
exhausted
```

---

# 24. Quota Reset

如果真实数据提供：

```text
resetAt
resetIn
```

显示：

```text
Resets in 2h 14m
```

或项目现有格式。

必须确认 timezone。

不要根据不可靠字段自行猜 reset 时间。

---

# 25. Last Updated

如果真实 Query / data 提供更新时间：

可以展示：

```text
Updated 2m ago
```

如果没有：

不要为了 UI 新增 account-level timer。

React Query `dataUpdatedAt` 若已有，可安全使用。

---

# 26. Manual Refresh

如果现有产品支持：

```text
Refresh Quota
```

保留。

要求：

- pending state
- prevent duplicate click
- success/error feedback
- 复用现有 handler

禁止点击 Refresh 时启动新的 interval。

---

# 27. Refresh All

只有现有业务支持批量刷新时才显示：

```text
Refresh All
```

不要通过前端循环逐个调用 account refresh 来伪造批量 API，除非旧实现本来如此。

---

# 28. Add Account

ContextHeader 提供：

```text
+ Add Account
```

如果真实流程是 OAuth：

按钮可表达：

```text
Add Account
```

进入 OAuth。

不要将其伪装成普通表单。

---

# 29. OAuth Flow

必须完全复用现有 OAuth 流程。

UI 可以改善：

```text
Idle
Opening browser
Waiting for authorization
Success
Error
Cancelled
```

但这些状态必须基于现有可观察状态。

禁止重写 OAuth State Machine。

---

# 30. OAuth Security

禁止：

- UI 显示 access token
- console.log token
- toast token
- query string token 暴露
- Drawer 保存 token 文本
- 将 secret 写入新的 Zustand UI store

OAuth Secret 继续由现有安全路径处理。

---

# 31. OAuth Window / Browser

如果现有流程使用：

```text
system browser
Tauri window
deep link
callback server
```

保持。

不要为了新版 UI 更换 OAuth transport。

---

# 32. OAuth Pending

授权等待期间：

UI 可以显示：

```text
Waiting for authorization…
```

并提供：

```text
Cancel
```

只有现有流程支持安全取消时才提供。

不要假取消但后台继续运行。

---

# 33. Account Switch

如果允许手动切换 Active Account：

使用现有 handler。

非 Active：

```text
Use Account
```

Active：

```text
Active
```

不要显示无意义的：

```text
Switch
```

按钮。

---

# 34. Auto Rotation

如果系统已有自动轮换：

建立独立：

```text
Rotation
```

区域。

推荐：

```text
Automatic Rotation        Enabled

Accounts are selected using the existing
Antigravity rotation strategy.
```

不要在 UI 文案里描述不存在的算法细节。

---

# 35. Rotation Algorithm 保护

禁止 UI 重构时修改：

```text
priority
quota selection
fallback order
cooldown
failure detection
account exclusion
retry
```

如果用户可以配置其中某些参数：

只展示真实已有配置。

---

# 36. Rotation Status

如果 Runtime 能提供：

```text
Current account
Last switch
Reason
Next candidate
```

可以展示。

如果没有：

不要推导。

尤其禁止：

```text
根据 quota 最低/最高猜 next account
```

---

# 37. Rotation Timeline

Phase 7 不要求新建 Account Rotation History。

如果已有历史数据：

可以轻量展示。

如果没有：

不要为了 UI 新增日志持久化。

---

# 38. Remove Account

Remove 属于 destructive action。

必须：

- secondary / overflow
- confirmation
- 显示 account identity
- danger style

例如：

```text
Remove user@example.com?
```

不要只显示：

```text
Are you sure?
```

---

# 39. Active Account Removal

如果删除 Active Account 有特殊规则：

完全遵循现有业务。

UI 不得自行：

```text
先切换再删除
```

除非现有 handler 就这么做。

---

# 40. Disabled / Expired Account

如果真实业务支持：

```text
Disabled
Expired
```

Account Card 应降低强调，但保持可读。

推荐：

```text
StatusBadge
subtle opacity / muted surface
```

不要隐藏账户导致用户无法管理。

---

# 41. Error State

Account-level Error 应尽量局部展示：

```text
Quota unavailable
OAuth expired
Refresh failed
```

前提是后端真实提供。

不要整个页面因为单个账户错误而白屏。

---

# 42. Partial Failure

Accounts 页面必须支持：

```text
Account A ready
Account B quota error
Account C expired
Account D loading
```

单一账户异常不应阻塞整个 Pool。

如果底层只有全局 Query：

不要为了 Partial Failure 强行拆业务 Query。

---

# 43. Empty State

无账户：

```text
No accounts connected.

Add an Antigravity account to enable account pooling
and quota-aware routing.

[Add Account]
```

文案根据真实产品功能调整。

---

# 44. Loading State

保持：

```text
AppShell
ContextHeader
```

立即出现。

Account Pool 使用：

```text
Skeleton
```

或现有 Loading。

不要全屏 Spin 覆盖整个 App。

---

# 45. Pool Layout

推荐 Desktop：

```text
2-column Account Cards
```

如果卡片字段较多：

```text
1-column compact list
```

也可以。

选择标准：

> 可读性优先于卡片数量。

---

# 46. Compact Window

窗口较窄：

```text
1 column
```

确保：

```text
email truncation
quota readable
actions accessible
no horizontal overflow
```

---

# 47. Account Actions

高频：

```text
Refresh
Use / Active
```

低频：

```text
Remove
Details
Advanced
```

不要把所有按钮横排。

---

# 48. Account Detail

Phase 7 不需要新建复杂 Account Detail Page。

如果已有较多 metadata：

使用：

```text
Drawer
```

展示。

不要为了 Detail 新增后台数据。

---

# 49. Domain Components

按真实复杂度考虑：

```text
src/components/accounts/
  AccountPoolOverview.tsx
  AccountCard.tsx
  AccountQuota.tsx
  RotationPanel.tsx
  AccountEmptyState.tsx
```

如果项目已经使用：

```text
components/antigravity
```

则遵循现有目录。

不要为了规划强制搬目录。

---

# 50. Component Boundary

推荐：

```text
AccountsPage
  orchestration

AccountPoolOverview
  summary presentation

AccountCard
  account presentation/actions

AccountQuota
  quota presentation

RotationPanel
  rotation presentation/config
```

Domain Components 不得直接绕过 hooks/services 调 IPC。

---

# 51. 不建立第二套 Account Store

禁止新增：

```text
accountsV2Store
accountPoolUIStore
dashboardAccountStore
```

保存业务事实。

UI-only 状态可以本地维护：

```text
selectedAccount
drawerOpen
filter
sort
```

业务事实必须来自现有源。

---

# 52. Phase 1 Foundation Reuse

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

不要重新建立：

```text
AccountCard primitive
Quota color system
Account status color system
```

---

# 53. Phase 2 Foundation Reuse

继续复用：

```text
AppShell
ContextHeader
StatusBar
Navigation Model
```

如果 Accounts 不属于 Claude/Codex Client Context：

不要强行显示 ClientSwitcher。

必须依据真实数据模型决定。

---

# 54. Phase 5 Dashboard Consistency

如果 Dashboard 已显示：

```text
Account
Quota
```

则 Phase 7 完成后必须验证：

```text
Dashboard
=
Accounts Page
```

对于：

```text
Active Account
Quota
Reset
```

相同数据语义必须一致。

---

# 55. Visual Direction

目标：

> **Account Pool Operations**

不是：

> Social Profile Manager

避免：

- 大型头像
- Profile cover
- 花哨账户颜色
- Gamified quota
- 大量装饰图标

优先：

```text
identity
quota
status
rotation
actions
```

---

# 56. Dark Mode

必须验证：

```text
Overview
Account Card
Active State
Disabled State
Expired State
Quota Progress
Rotation
OAuth Pending
Dropdown
Confirmation
Error
Empty
```

全部消费 Semantic Tokens / antd Theme。

---

# 57. Accessibility

要求：

- Account identity 有文本
- Status 不只依赖颜色
- Quota 有 numeric value
- IconButton 有 aria-label
- OAuth pending 可读
- Remove confirmation keyboard accessible
- Dropdown keyboard accessible
- Progress 提供 accessible value

---

# 58. i18n

新增文案进入：

```text
zh-CN.json
en-US.json
```

建议 namespace：

```text
accounts
```

或沿用：

```text
antigravity
```

根据现有项目命名。

---

# 59. Performance

Account Card 数量通常有限，但仍需检查：

```text
per-card query
per-card timer
per-card event listener
per-card quota fetch
```

这是 Phase 7 的重点。

禁止形成：

```text
10 accounts
=
10 independent intervals
```

除非现有架构本来如此且暂时不能安全改变。

若发现：

记录 Deferred。

---

# 60. Refresh Architecture 目标

理想：

```text
Existing Account/Quota Refresh Mechanism
            ↓
Shared Query / Store
            ↓
Accounts Page
            ↓
Account Cards

            ↓
Dashboard Snapshot
```

而不是：

```text
Account Card A → timer
Account Card B → timer
Account Card C → timer
Dashboard      → timer
```

---

# 61. Sensitive Information Audit

必须搜索 UI 是否可能暴露：

```text
access_token
refresh_token
authorization
bearer
cookie
secret
```

尤其检查：

```text
Account Detail
Error
Toast
Console
Tooltip
OAuth callback
```

Phase 7 不允许扩大 Secret 可见范围。

---

# 62. Phase 7 文件修改范围

允许重点修改：

```text
Accounts / Antigravity Page
Account presentation components
components/accounts or components/antigravity
Account CSS
i18n
docs/ui-refactor
```

允许谨慎修改：

```text
existing Account hooks
React Query selectors
UI-only helper
```

前提：

不改变 runtime 行为。

---

# 63. 禁止修改范围

原则上禁止：

```text
src/services/ipc.ts
src/services/antigravity.ts
Rust backend
Tauri commands
OAuth protocol
Token persistence
Account rotation algorithm
Quota scheduler
Account persistence
```

如果新版 UI 必须依赖业务变更：

停止该部分并记录：

```markdown
## Deferred Business Dependency
```

---

# 64. 不要顺便重构其他模块

Phase 7 不要重构：

```text
Dashboard
Usage
Providers
Proxy
Workspace
Settings
App Shell
```

只允许最小修复 Phase 7 引发的 regression。

---

# 65. 推荐实施顺序

## Step 1
创建 `phase-7-preflight.md`。

## Step 2
完成 Account Architecture Map。

## Step 3
完成 Refresh / Polling / Event Listener 审计。

## Step 4
确认 45s / 5min 真实生命周期。

## Step 5
确认 Dashboard Quota 数据源关系。

## Step 6
重构 Accounts Page Skeleton。

## Step 7
建立 Pool Overview。

## Step 8
建立 Account Card / List。

## Step 9
接入 Quota / Reset / Status。

## Step 10
接入 Add / OAuth。

## Step 11
接入 Switch / Refresh / Remove。

## Step 12
重构 Rotation Presentation。

## Step 13
完成 Loading / Empty / Error / Expired / Disabled。

## Step 14
验证 Dark / Accessibility / Compact Window。

## Step 15
执行 Account / OAuth / Rotation / Quota Regression。

---

# 66. Functional Regression Matrix

至少验证：

| Scenario | Expected |
|---|---|
| No accounts | Empty State 正确 |
| Add account | 使用现有 OAuth |
| OAuth success | Account 正常出现 |
| OAuth error | 安全错误反馈 |
| Active account | 状态正确 |
| Switch account | 使用现有 handler |
| Manual quota refresh | 现有行为保持 |
| Auto quota refresh | 生命周期保持 |
| Account expired | 状态正确 |
| Account disabled | 状态正确 |
| Remove account | 使用现有确认/handler |
| Remove active account | 遵循现有业务 |
| Auto rotation | 行为不变 |
| Dashboard quota | 与 Accounts 一致 |
| App restart | Account persistence 不变 |

---

# 67. OAuth Regression

至少验证现有支持范围：

```text
Start OAuth
Browser / Window Open
Callback
Success
Failure
Cancel（如果支持）
Duplicate Account（如果已有规则）
Token persistence
App restart
```

不要通过查看 UI 就声称 OAuth 正常。

必须使用现有可安全执行的验证方式。

---

# 68. Rotation Regression

至少验证：

```text
Rotation Enabled
Rotation Disabled（如果支持）
Single Account
Multiple Accounts
Quota Exhaustion
Unavailable Account
Manual Switch
Automatic Switch
```

不能为了测试破坏真实账户。

无法安全模拟：

```text
Not manually simulated
```

---

# 69. Refresh Verification

完成后记录：

```text
45s initial refresh:
5min event polling:
Manual refresh:
Per-account interval:
Dashboard quota refresh:
New interval added:
New listener added:
Duplicate refresh found:
```

目标：

```text
New interval added: 0
New duplicate refresh: 0
```

---

# 70. Build Verification

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

# 71. Manual Visual Verification

至少验证：

```text
Light
Dark

0 Account
1 Account
Multiple Accounts

Active Account
Non-active Account
Expired Account（可安全模拟时）
Disabled Account（如果支持）
Quota Available
Quota Unavailable

OAuth Idle
OAuth Pending
OAuth Error（可安全模拟时）

Rotation Enabled
Rotation Disabled（如果支持）

Long Email
Long Account Name

Compact Window
Normal Window
Large Window
```

---

# 72. Phase 7 Deliverables

完成后创建：

```text
docs/ui-refactor/phase-7-result.md
```

必须包含：

## A. Changed Files

```text
File
Purpose
Risk
```

## B. Account Architecture

```text
AccountsPage
Pool Overview
Account Card
Quota
OAuth
Rotation
State Sources
```

## C. Runtime Protection

明确：

```text
OAuth changed:
Token behavior changed:
Rotation algorithm changed:
Quota timing changed:
Persistence changed:
```

期望全部：

```text
No
```

## D. Refresh Audit

记录：

```text
45s source:
5min source:
Manual refresh:
Dashboard source:
Duplicate polling:
New interval:
New listener:
```

## E. Account States

说明实际支持：

```text
Active
Available
Disabled
Expired
Error
Quota Exhausted
Unknown
```

哪些是真实业务状态。

## F. Security

说明：

```text
Access Token
Refresh Token
Authorization
OAuth errors
Account identity
```

如何处理。

## G. Verification

```text
TypeScript:
Build:
Lint:
Tests:
Light:
Dark:
Add account:
OAuth:
Switch:
Refresh:
Remove:
Rotation:
Dashboard consistency:
Compact window:
```

## H. Deferred Issues

记录 Phase 7 不应解决的问题。

---

# 73. Phase 8 Workspace / Resources Readiness

Phase 7 完成后，对 Workspace / Resources 做只读审计。

回答：

1. 当前 Workspace / Resources 页面有哪些？
2. MCP 是否属于 Workspace / Resources 范围？
3. Environment / Config / Skills / MCP 等资源目前如何组织？
4. 每类资源的数据源是什么？
5. 哪些资源来自文件系统？
6. 哪些资源来自 Tauri IPC？
7. 哪些资源支持 Add / Edit / Delete / Enable / Disable？
8. 是否存在文件 watcher / polling / event listener？
9. Workspace 是否按 Client Context 隔离？
10. 当前 Resource Detail / Editor 如何实现？
11. 是否存在直接编辑用户配置文件的逻辑？
12. 哪些文件属于高风险业务边界？
13. 哪些 Workspace UI 可以安全重构？
14. 哪些资源操作必须完全保留？
15. Phase 8 推荐修改哪些文件？
16. 是否存在可以复用 Phase 3 Provider / Phase 7 Account 的列表与状态模式？

只做 Readiness。

不要在 Phase 7 提前重构 Workspace / MCP。

---

# 74. Git / Diff Discipline

保持 Phase 7 Diff 聚焦。

禁止：

```text
全项目格式化
无关文件重命名
重构 OAuth
重构 Account Runtime
重构 Rotation
修改 Quota scheduler
修改 Usage
修改 Proxy
升级 dependencies
```

无关问题：

```markdown
## Deferred Issues
```

---

# 75. Phase 7 成功标准

完成后用户应该能快速回答：

```text
现在有几个账户？
→ Pool Overview

当前正在使用哪个账户？
→ Active Account

每个账户还剩多少额度？
→ Account Quota

额度什么时候恢复？
→ Reset Time

自动轮换是否开启？
→ Rotation

如何新增账户？
→ Add Account / OAuth

如何手动刷新？
→ Refresh

哪个账户异常？
→ Account Status
```

技术层必须保持：

```text
New Accounts UI
        ↓
Existing Account Facts
        ↓
Existing Query / Store
        ↓
Existing Antigravity Service
        ↓
Existing IPC / Backend
```

而不是：

```text
New Accounts UI
├── New OAuth
├── New Rotation
├── New Token State
├── New Quota Timer
└── Existing Runtime
```

---

# 76. 最终执行指令

现在执行 Phase 7。

必须遵守：

1. 先创建 `docs/ui-refactor/phase-7-preflight.md`
2. 完成 Account Architecture Map
3. 专项审计所有 timer / polling / Tauri event listener
4. 明确验证 45s 首刷与 5min 事件轮询的真实实现
5. 审计 Dashboard Quota Snapshot 与 Accounts 页的数据源关系
6. Preflight 完成后继续实施，不等待确认
7. Accounts UI 可以明显重构
8. OAuth / Token / Rotation / Quota Runtime 禁止重写
9. 不创建第二套 Account Pool
10. 不创建第二套 Quota Store
11. 新增 interval 目标为 0
12. 新增重复 refresh 目标为 0
13. Account Card 不得各自建立 timer
14. 不扩大 Access Token / Refresh Token 可见范围
15. Active Account 必须来自真实 runtime state
16. Quota / Reset 必须使用真实数据语义
17. Dashboard 与 Accounts 相同 Quota 数据必须验证一致性
18. 优先消费 Phase 1 Primitives
19. 优先消费 Phase 2 App Shell / Navigation
20. 不顺便重构 Dashboard / Usage / Proxy / Workspace
21. 完成 OAuth / Rotation / Refresh 回归
22. 完成后创建 `docs/ui-refactor/phase-7-result.md`
23. 输出 Phase 8 Workspace / Resources Readiness

**最高优先级：把 Accounts / Antigravity 页面重构成可信、紧凑、高效的 Account Pool Control Center，同时确保新版 UI 只是既有账户池、额度与轮换 Runtime 的观察与控制层，而不是重新实现 Antigravity Runtime。**
