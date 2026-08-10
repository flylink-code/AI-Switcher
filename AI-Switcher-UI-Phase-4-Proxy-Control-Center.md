# AI-Switcher UI Refactor — Phase 4 Execution

## Proxy Control Center + Failover Experience

> 前置条件：Phase 0 ~ Phase 3 已完成。
>
> 当前架构继续保持：Tauri v2 + React 19 + Zustand + TanStack React Query + antd v6 + `activeKey + pageRegistry.ts`。
>
> Phase 1 已建立 Design Tokens / Primitive UI；Phase 2 已建立 App Shell / ContextHeader / StatusBar；Phase 3 已完成 Providers Domain UI 重构。
>
> 本阶段目标是重构 Proxy 页面体验，而不是重写 Proxy Engine。

---

# 0. Phase 4 核心原则

Phase 4 是：

> **Proxy Control Center UI / UX Refactor**

不是：

> **Proxy Runtime / Failover Algorithm Rewrite**

必须保持以下调用关系：

```text
New Proxy UI
      ↓
Existing Proxy Hooks / State
      ↓
Existing Proxy Service
      ↓
Existing IPC
      ↓
Existing Rust Proxy Engine
```

禁止建立第二套 Proxy Runtime。

---

# 1. 总目标

将 Proxy 页面重构为桌面级运行控制中心：

```text
Proxy
├── Runtime Overview
│   ├── Running / Stopped / Error
│   ├── Start / Stop
│   ├── Listen Port
│   └── Active Provider
│
├── Routing
│   ├── Primary Provider
│   └── Failover Chain
│
├── Resilience
│   ├── Retry
│   ├── Timeout
│   └── Retry HTTP Status Codes
│
└── Runtime Feedback
    ├── Loading
    ├── Transitioning
    ├── Warning
    └── Error
```

要求：

- Proxy 当前状态一眼可见
- Start / Stop 是页面最明确的运行操作
- Active Provider 与 Failover Chain 层级清晰
- 高级策略与日常控制分离
- 与 Phase 2 StatusBar 保持一致
- 不增加重复 polling
- 不修改 Proxy 算法

---

# 2. Phase 4 Preflight

开始修改前读取：

```text
docs/ui-refactor/phase-2-result.md
docs/ui-refactor/phase-3-preflight.md
docs/ui-refactor/phase-3-result.md
```

以及实际：

```text
Proxy Page
Proxy components
Proxy hooks
Proxy Zustand / React Query state
src/services/proxy.ts
src/services/ipc.ts
StatusBar.tsx
ProviderCard.tsx
Provider health / latency source
```

创建：

```text
docs/ui-refactor/phase-4-preflight.md
```

完成后继续执行，不等待确认。

---

# 3. Preflight 必须回答

记录真实文件和状态来源：

```text
Proxy Page:
Runtime state source:
Start handler:
Stop handler:
Restart handler (if any):
Port source:
Active Provider source:
Failover configuration source:
Retry source:
Timeout source:
Retry status code source:
Error source:
StatusBar proxy source:
```

同时标记：

```text
Presentation-only
UI state
Business state
IPC boundary
Rust/runtime boundary
```

---

# 4. 重复 Polling 审计

Phase 3 ProviderCard 已出现：

```text
Health
Latency
StatusBadge
```

Phase 4 开始前必须确认这些数据的真实来源。

检查：

```text
Provider health polling
Provider latency polling
Proxy runtime polling
StatusBar polling
React Query refetchInterval
setInterval
Tauri event listener
```

必须回答：

> Provider 页面、Proxy 页面、StatusBar 是否正在为同一运行状态建立重复 polling？

如果存在重复：

优先让多个 UI 消费已有共享 Query / Store / Event State。

禁止为了 Phase 4 再新增：

```text
setInterval
第二套 proxy status query
第二套 health query
第二套 event listener
```

除非现有架构确实没有共享来源。

不得为了去重而修改 Rust Proxy Engine。

---

# 5. 强制业务保护区

原则上禁止修改：

```text
src/services/ipc.ts
src/services/proxy.ts
```

禁止改变：

- Proxy startup
- Proxy shutdown
- lifecycle
- hot switching
- failover algorithm
- retry algorithm
- retry order
- timeout semantics
- HTTP retry status semantics
- streaming timeout behavior
- request routing
- provider selection
- port binding behavior
- Tauri command contracts
- Rust proxy implementation

若新版 UI 需要业务层变更才能实现：

停止该部分，记录：

```markdown
## Deferred Business Dependency
```

---

# 6. Proxy 页面信息架构

推荐页面：

```text
ContextHeader
├── Proxy
└── Local routing and failover control

Content
├── Runtime Control
├── Routing & Failover
└── Resilience Settings
```

不要重复显示第二个大型 `Proxy` Page Header。

---

# 7. Runtime Control

页面顶部建立最重要的 Runtime Surface。

推荐：

```text
┌──────────────────────────────────────────────┐
│ Proxy                               RUNNING │
│ Local API routing service                    │
│                                              │
│ ● Running     Port 15821                     │
│ Active Provider: OpenRouter                  │
│                                              │
│                                  [ Stop ]    │
└──────────────────────────────────────────────┘
```

Stopped：

```text
● Stopped
Port 15821
[ Start Proxy ]
```

Error：

```text
Proxy failed to start
<安全的错误摘要>
[ Retry ]
```

具体字段以真实状态为准。

---

# 8. Runtime State Machine

UI 必须正确表达已有生命周期。

至少审计是否存在：

```text
Stopped
Starting
Running
Stopping
Error
```

如果业务层只有：

```text
Running / Stopped
```

不要凭空创建新的业务状态。

允许 UI 本地通过 mutation pending 表达：

```text
Starting...
Stopping...
```

但不得将其伪装成新的后端事实状态。

---

# 9. Start / Stop

Start / Stop 是 Runtime Surface 的 Primary Action。

要求：

- Running → Stop
- Stopped → Start
- Starting → disabled + loading
- Stopping → disabled + loading
- Error → 根据现有能力 Retry / Start

禁止：

- 连续重复提交
- 同时显示 Start 和 Stop
- 点击后乐观伪造 Running 状态

必须以现有真实状态为最终依据。

---

# 10. Port

监听端口是高价值 Runtime 信息。

推荐：

```text
Port
15821
```

如果允许修改：

使用现有设置逻辑。

如果运行中不允许修改：

UI 应 disabled，并提供简短说明。

不要改变端口生效时机。

如果修改后必须 Restart：

只能根据现有业务行为提示，不要擅自实现新的自动 Restart。

---

# 11. Active Provider

Runtime Control 必须显示真实：

```text
Active Provider
```

数据来源应与：

```text
Providers Page
StatusBar
Proxy routing
```

一致。

禁止创建 Proxy 专用 `activeProviderV2`。

如果 Provider 在 Phase 3 被切换：

Proxy 页面必须通过现有状态机制反映变化。

---

# 12. StatusBar 一致性

Phase 2 已建立 StatusBar。

Phase 4 必须保证：

```text
Proxy Page Runtime State
=
StatusBar Runtime State
```

包括至少：

```text
Running / Stopped
Port
Active Provider
```

若 StatusBar 数据过时：

先定位状态共享问题。

不要简单增加 StatusBar polling。

---

# 13. Routing Section

建立：

```text
Routing
```

用于表达：

```text
Primary Provider
Failover Chain
```

不要把 Retry / Timeout 混在 Provider 顺序中。

Routing 回答：

> 请求优先走哪里，失败后走哪里？

Resilience 回答：

> 什么时候算失败、重试多少次、多久超时？

两个概念必须分开。

---

# 14. Primary Provider

Primary Provider 应明确。

例如：

```text
Primary
OpenRouter
anthropic/claude-sonnet-4
```

优先复用 Phase 3 已建立的 Provider Identity 表达。

不要直接复用完整 `ProviderCard` 如果它太重。

可以建立更轻量 Domain Component：

```text
ProviderRouteItem
```

---

# 15. Failover Chain

Failover 是 Phase 4 核心 UI。

推荐表达：

```text
Primary
OpenRouter

   ↓ on failure

Fallback 1
Anthropic

   ↓ on failure

Fallback 2
Custom API
```

或者紧凑：

```text
[1 OpenRouter] → [2 Anthropic] → [3 Custom]
```

根据窗口宽度选择。

优先 Desktop 可读性，而不是炫技。

---

# 16. Failover Chain 操作

如果现有业务支持调整顺序：

可以提供：

```text
Move Up
Move Down
Drag Handle
```

但优先选择最稳定的实现。

如果现有排序 mutation 已经成熟，可使用 Drag & Drop。

如果没有：

不要为了 Phase 4 引入大型 DnD Framework。

可以使用：

```text
↑
↓
```

完成排序。

---

# 17. Failover Chain 业务保护

UI 中的顺序必须严格映射现有业务顺序。

禁止：

- UI 自动排序
- 按 latency 自动重排
- 按 health 自动重排
- 改变 fallback priority
- 自动删除 unavailable provider
- 自动新增 fallback

除非这些行为已经存在于业务层。

---

# 18. Failover Empty State

没有 Failover：

```text
No fallback providers configured.

Requests will only use the primary provider.
[Add fallback]
```

只有现有业务支持添加时才显示 Add。

---

# 19. Failover Provider State

如果已有真实 health 状态：

可以显示：

```text
Healthy
Unavailable
Unknown
```

如果只有 latency：

只显示 latency。

不要根据：

```text
latency > 某数值
```

自行推导 Error，除非业务已有定义。

---

# 20. Retry Strategy

建立独立：

```text
Resilience
```

Surface。

根据真实配置展示：

```text
Retry Count
Retry Delay
Timeout
Streaming Timeout
Retry Status Codes
```

只展示实际存在的配置。

不要为了完整感创建不存在的字段。

---

# 21. Advanced Configuration

低频配置可以进入：

```text
Advanced
```

折叠区域。

例如：

```text
HTTP Status Codes
Streaming Timeout
Detailed Retry Parameters
```

高频：

```text
Retry Count
Timeout
```

可直接显示。

具体依据当前产品使用频率。

---

# 22. HTTP Retry Status Codes

如果现有 UI 支持配置，例如：

```text
429
500
502
503
504
```

新版 UI 应明确语义：

```text
Retry on HTTP status
[429] [500] [502] [503] [504]
```

不要让用户编辑一个难理解的裸字符串，除非当前业务只支持字符串。

UI 转换必须保证保存后的数据完全兼容原格式。

---

# 23. Timeout

必须区分真实存在的不同 timeout。

例如：

```text
Request Timeout
Streaming Timeout
Connect Timeout
```

禁止把多个不同语义 timeout 合并成一个 UI 字段。

如果业务只有一个 timeout：

只展示一个。

---

# 24. Settings Save Strategy

确认现有 Proxy Settings 是：

```text
Immediate Save
Save Button
Apply
Restart Required
```

新版 UI 必须保持相同业务语义。

禁止视觉重构时偷偷改成 auto-save。

如果 Restart Required：

只根据真实业务规则提示。

---

# 25. Runtime Feedback

Start / Stop / Save / Failover Change 都必须有合理反馈。

优先：

```text
inline pending
button loading
StatusBadge
toast
```

避免每个操作都弹 Modal。

---

# 26. Error Presentation

Proxy Error 应提供：

```text
What failed
Safe reason
Possible next action
```

例如：

```text
Proxy couldn't start
Port 15821 is unavailable.
```

前提是后端真实提供该信息。

不要：

- 展示 raw stack trace
- 展示 token
- 展示完整敏感 headers
- 猜测错误原因

---

# 27. Logs

如果现有 Proxy 页面已经有 Logs：

Phase 4 可以改善其视觉层级。

推荐放在：

```text
Diagnostics
```

或：

```text
Advanced
```

不要让 Logs 抢占 Runtime Control 主视觉。

如果没有 Logs：

不要在 Phase 4 新建日志系统。

---

# 28. Proxy Domain Components

可以按真实需要建立：

```text
src/components/proxy/
  ProxyRuntimeCard.tsx
  ProxyStatusBadge.tsx
  FailoverChain.tsx
  FailoverItem.tsx
  ResilienceSettings.tsx
```

不要机械创建。

其中 `ProxyStatusBadge` 只有在确有 Domain 映射价值时才建立。

否则直接使用 Phase 1：

```text
StatusBadge
```

---

# 29. Component Boundary

推荐：

```text
ProxyPage
  orchestration

ProxyRuntimeCard
  runtime presentation/actions

FailoverChain
  route priority presentation/actions

ResilienceSettings
  retry/timeout presentation
```

Domain Component 不得直接绕过现有业务层调用 Tauri IPC。

---

# 30. Phase 1 / 2 / 3 复用

必须优先消费：

```text
Surface
Stack
Inline
StatusBadge
Metric
IconButton

ContextHeader
StatusBar

Provider identity patterns
```

不要再建立第二套：

```text
Card system
Status colors
Spacing
Provider identity
Runtime badge
```

---

# 31. Visual Direction

Proxy 页面应该像：

> Desktop Runtime Control Panel

而不是：

> Settings Form Dump

视觉优先级：

```text
1. Proxy Running State
2. Start / Stop
3. Active Provider
4. Failover Chain
5. Retry / Timeout
6. Advanced Details
```

---

# 32. Density

保持紧凑。

避免：

- 巨型状态数字
- 大面积 Hero
- 过大 Running 图标
- 每项设置单独一张巨卡
- 大量无意义留白

推荐：

```text
Runtime → prominent Surface
Routing → medium Surface
Resilience → compact settings Surface
```

---

# 33. Dark Mode

必须验证：

```text
Runtime Card
Running / Stopped / Error
Start / Stop
Port
Provider Identity
Failover Chain
Retry Inputs
Advanced Section
Error State
StatusBar
```

不得通过硬编码 Light 色值实现新版 Proxy UI。

---

# 34. Accessibility

要求：

- Start / Stop keyboard accessible
- Icon actions 有 aria-label
- Status 不只依赖颜色
- Failover 顺序可读
- Drag-only 操作必须有替代方式
- Inputs 有 label
- Error 与对应字段关联

---

# 35. Responsive / Window Strategy

Desktop 优先。

推荐：

```text
Large
Runtime + Routing 可形成合理双栏

Normal
主要区域单栏或宽窄组合

Compact
全部单栏
```

不要为 Mobile 建复杂 breakpoint。

必须避免：

```text
horizontal overflow
double scrollbar
Failover chain clipping
```

---

# 36. Phase 4 文件修改范围

允许重点修改：

```text
Proxy Page
Proxy presentation components
Proxy page CSS
components/proxy
ContextHeader page actions（如必要）
StatusBar presentation（仅最小一致性修复）
Proxy UI i18n
docs/ui-refactor
```

谨慎修改：

```text
UI-only Zustand state
existing Proxy hooks
```

仅允许 presentation integration 所需的小调整。

---

# 37. 禁止修改范围

原则上禁止：

```text
src/services/ipc.ts
src/services/proxy.ts
src/services/providers.ts
src/services/antigravity.ts
Rust proxy engine
Tauri commands
Provider persistence
Failover algorithm
Retry algorithm
```

不要升级依赖。

不要引入新 Router。

不要引入新的状态管理库。

---

# 38. 不要顺便重构其他模块

Phase 4 不要顺便重构：

```text
Dashboard
Usage
Accounts
Workspace
Settings
Providers
App Shell
```

如果发现问题：

记录：

```markdown
## Deferred Issues
```

只允许修复明确由 Phase 4 引起的 regression。

---

# 39. 推荐实施顺序

## Step 1
完成 `phase-4-preflight.md`。

## Step 2
完成重复 polling / event listener 审计。

## Step 3
锁定现有：

```text
runtime state
start/stop
port
active provider
failover
retry
timeout
```

业务来源。

## Step 4
重构 Proxy Page Skeleton。

## Step 5
实现 Runtime Control。

## Step 6
验证 StatusBar 状态一致。

## Step 7
实现 Routing / Failover Chain。

## Step 8
实现 Resilience Settings。

## Step 9
补 Loading / Transition / Error / Empty 状态。

## Step 10
验证 Dark Mode / Accessibility / Compact Window。

## Step 11
执行完整 Proxy regression。

---

# 40. Functional Regression Matrix

至少验证：

| Scenario | Expected |
|---|---|
| Proxy stopped | 显示 Stopped，允许 Start |
| Start proxy | 使用现有 handler，最终反映真实 Running |
| Proxy running | 显示 Running / Port / Active Provider |
| Stop proxy | 使用现有 handler，最终反映真实 Stopped |
| Start failure | 显示真实安全错误 |
| Provider switch while stopped | 保持现有行为 |
| Provider switch while running | 保持 hot switch 行为 |
| Failover reorder | 与现有持久化结果一致 |
| Retry config save | 与旧行为一致 |
| Timeout save | 与旧行为一致 |
| StatusBar | 与 Proxy 页面一致 |
| App restart | Proxy 配置保持现有持久化行为 |

---

# 41. Failover Regression

至少验证：

```text
0 fallback
1 fallback
multiple fallback
reorder
remove fallback
current primary provider
provider unavailable（如果已有真实状态）
```

不能因为 UI 排序造成实际 Failover 顺序错误。

---

# 42. Polling / Performance Verification

完成后记录：

```text
Proxy status polling count:
Provider health polling count:
Latency polling count:
Tauri event listeners:
React Query refetch intervals:
```

目标：

> 同一数据尽可能只有一个权威获取机制，多处 UI 共享结果。

如果现有架构暂时无法安全去重：

不要大改。

记录 Deferred。

---

# 43. Build Verification

执行实际存在的：

```bash
npm run typecheck
npm run build
```

如果存在：

```bash
npm run lint
npm run test
```

也执行。

不得伪造结果。

---

# 44. Manual Visual Verification

至少验证：

```text
Light Theme
Dark Theme

Sidebar Expanded
Sidebar Collapsed

Proxy Stopped
Proxy Starting
Proxy Running
Proxy Stopping
Proxy Error（可安全模拟时）

No Failover
Single Failover
Multiple Failover

Long Provider Name
Long Endpoint

Compact Window
Normal Window
Large Window
```

无法安全模拟的状态标记：

```text
Not manually simulated
```

不要为了截图破坏真实配置。

---

# 45. Phase 4 Deliverables

完成后创建：

```text
docs/ui-refactor/phase-4-result.md
```

必须包含：

## A. Changed Files

```text
File
Purpose
Risk
```

## B. Proxy Architecture

```text
ProxyPage
Runtime UI
Failover UI
Resilience UI
State Sources
Service Boundary
```

## C. Business Logic Reuse

明确：

```text
Runtime source:
Start source:
Stop source:
Port source:
Active provider source:
Failover source:
Retry source:
Timeout source:
```

## D. Polling Audit

说明：

```text
Provider health:
Latency:
Proxy status:
StatusBar:
Duplicate polling found:
Changes made:
Deferred:
```

## E. Runtime States

说明实际支持：

```text
Stopped
Starting
Running
Stopping
Error
```

哪些来自后端，哪些只是 UI pending state。

## F. Failover

说明：

```text
Ordering
Add
Remove
Reorder
Persistence
Health display
```

实际支持情况。

## G. Verification

```text
TypeScript:
Build:
Lint:
Tests:
Light:
Dark:
Start:
Stop:
Hot switch:
Failover:
Retry:
Timeout:
StatusBar:
Compact window:
```

## H. Deferred Issues

记录所有 Phase 4 不应解决的问题。

---

# 46. Phase 5 Dashboard Readiness

Phase 4 完成后，对 Dashboard 做只读审计。

回答：

1. 当前是否已有 Dashboard/Home 页面？
2. 当前 Dashboard 使用哪些数据源？
3. 哪些 Runtime 数据已经可从共享状态直接读取？
4. Provider Current State 从哪里读取？
5. Proxy Runtime 从哪里读取？
6. Usage summary 从哪里读取？
7. Antigravity quota summary 从哪里读取？
8. 是否存在为了 Dashboard 需要新增的重复 polling 风险？
9. 哪些 Metric 可以直接复用现有数据？
10. 哪些数据不应该为了 Dashboard 新增后台查询？
11. Phase 5 建议修改哪些文件？
12. 哪些业务文件 Phase 5 仍然不能碰？

只做 Readiness。

不要在 Phase 4 提前实现 Dashboard。

---

# 47. Git / Diff Discipline

保持 Phase 4 Diff 聚焦。

禁止：

```text
全项目 prettier
无关 import 重排
重构 Rust
重构 services
修改 Provider model
修改 Antigravity
重构 Usage
重构 Dashboard
升级 dependencies
```

无关问题进入：

```markdown
## Deferred Issues
```

---

# 48. Phase 4 成功标准

完成后用户应该可以在一个页面快速回答：

```text
Proxy 在运行吗？
→ Runtime Status

监听哪个端口？
→ Port

当前请求走哪个 Provider？
→ Active Provider

失败后走哪里？
→ Failover Chain

什么时候重试？
→ Resilience

如何启动/停止？
→ Primary Runtime Action
```

代码层必须保持：

```text
New Proxy UI
      ↓
Existing State / Hooks
      ↓
Existing Services
      ↓
Existing IPC
      ↓
Existing Rust Runtime
```

而不是：

```text
New Proxy UI
├── New Polling
├── New Proxy State
├── New Retry Logic
├── New Failover Logic
└── Existing Runtime
```

---

# 49. 最终执行指令

现在执行 Phase 4。

必须遵守：

1. 先创建 `docs/ui-refactor/phase-4-preflight.md`
2. 首先审计重复 polling / event listener
3. 不等待确认，完成 preflight 后继续实施
4. Proxy UI 可以明显重构
5. Proxy Runtime / Failover / Retry 业务逻辑禁止重写
6. 不创建第二套 Proxy state
7. 不创建第二套 runtime polling
8. StatusBar 与 Proxy 页面必须消费一致状态
9. Active Provider 必须与 Providers 页面保持一致
10. Failover UI 顺序必须严格对应真实业务顺序
11. 不修改 Rust Proxy Engine
12. 不绕过 `src/services/ipc.ts`
13. 优先复用 Phase 1 Primitives
14. 优先复用 Phase 2 App Shell / StatusBar
15. 复用 Phase 3 Provider Identity 表达
16. 完成 Start / Stop / Hot Switch / Failover / Retry / Timeout 回归
17. 完成后创建 `docs/ui-refactor/phase-4-result.md`
18. 输出 Phase 5 Dashboard Readiness

**最高优先级：把 Proxy 页面重构成清晰、高效、可信的 Desktop Runtime Control Center，同时确保 UI 只是现有 Proxy Engine 的控制与观察层，而不是重新实现 Proxy Engine。**
