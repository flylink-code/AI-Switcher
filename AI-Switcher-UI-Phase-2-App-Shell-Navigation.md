# AI-Switcher UI Refactor — Phase 2 Execution

## App Shell + Navigation Model

> 前置条件：Phase 0 与 Phase 1 已完成。
>
> 当前技术栈：Tauri v2 + React 19 + Vite 7 + TypeScript 5.9 + antd v6 + Zustand + TanStack React Query。
>
> 当前导航机制：`activeKey + src/lib/pageRegistry.ts`，没有 `react-router-dom`。
>
> Phase 1 已建立 Semantic Design Tokens 与 `src/components/ui/` Primitive UI Foundation。

---

# 0. Phase 2 核心原则

Phase 2 是：

> **Navigation UI Migration + App Shell Reconstruction**

不是：

> Router Migration / Business Architecture Refactor

必须继续保留当前：

```text
activeKey
+
pageRegistry.ts
```

作为页面挂载与导航基础。

本阶段禁止为了新版 Sidebar 引入 `react-router-dom`。

新版 UI 的导航模型可以与旧 `activeKey` 不同，但必须通过兼容映射层连接现有页面。

---

# 1. Phase 2 总目标

本阶段建立 AI-Switcher 新版应用骨架：

```text
App Shell
├── Sidebar
├── Main Area
│   ├── Context Header
│   └── Content
└── Status Bar
```

同时正式拆分四种不同概念：

```text
App Navigation
Client Context
Runtime State
Workspace Scope
```

解决旧 UI 中：

- Client Tab 与页面导航混杂
- 功能入口层级不统一
- Proxy 状态分散
- Settings 承载过多 Workspace 功能
- 页面之间缺少统一 Header
- Sidebar 信息密度和视觉层级不一致

的问题。

---

# 2. Navigation Model

新版一级导航定义为：

```text
Dashboard

Providers
Proxy
Usage
Accounts
Workspace

────────────

Settings
```

推荐逻辑分组：

```text
OVERVIEW
  Dashboard

RUNTIME
  Providers
  Proxy
  Usage

RESOURCES
  Accounts
  Workspace

SYSTEM
  Settings
```

UI 中不一定显示 `OVERVIEW / RUNTIME / RESOURCES / SYSTEM` 文字。

如果显示，应保持克制，避免 Sidebar 过度复杂。

---

# 3. Client Context

以下四项：

```text
Claude Code
Claude Desktop
Codex
OpenCode
```

不再被视为一级 App Navigation。

它们属于：

> **Client Context**

含义是：

> “当前正在查看 / 管理哪个 AI Client 的配置与运行上下文？”

Client Context 应由 Context Header 提供切换能力。

概念结构：

```text
App Navigation
        │
        ▼
    Providers
        │
        ├── Claude Code Context
        ├── Claude Desktop Context
        ├── Codex Context
        └── OpenCode Context
```

切换 Client Context：

- 不代表进入另一个一级页面
- 不改变导航体系
- 不合并四个 Client 的业务配置
- 不破坏 Client Isolation

---

# 4. Client Isolation 强制保护

必须继续保持：

```text
Claude Code
Claude Desktop
Codex
OpenCode
```

各自配置隔离。

禁止：

- 合并 Provider 配置
- 合并客户端状态
- 修改 active client 的业务语义
- 修改现有同步逻辑
- 修改 Codex 官方登录逻辑
- 修改 Bearer Token 动态注入逻辑

Phase 2 只改变：

> Client Context 的 UI 表达方式。

不改变：

> Client Context 的业务行为。

---

# 5. Runtime State

Runtime State 与 Navigation / Client Context 分离。

至少包括：

```text
Proxy
  Running
  Stopped
  Error

Port
  例如 15721

Active Provider

Failover State
```

Runtime State 的核心摘要应进入 App Shell。

推荐由：

```text
Status Bar
```

负责。

例如：

```text
● Proxy Running     :15721     Provider: OpenRouter
```

或：

```text
Proxy ● Running    Port 15721    OpenRouter
```

具体视觉可以根据现有信息调整。

---

# 6. Workspace Scope

以下功能不再长期归属于 Settings：

```text
Projects
MCP
Prompts
Skills
Agents
Codex Plugins
Sessions
```

它们统一归入：

```text
Workspace
```

但是：

**Phase 2 不要求重写这些页面。**

Phase 2 只需要：

1. 建立 Workspace 导航入口
2. 建立 Workspace 子导航模型
3. 通过旧 `activeKey` 映射现有页面
4. 保证旧功能仍可访问

真正页面重构放在后续 Phase。

---

# 7. Legacy Navigation Compatibility Layer

当前项目有约 16 个 `activeKey` 页面。

禁止 Phase 2 一次性删除或重写这些 key。

建立兼容映射。

推荐概念：

```ts
type PrimaryNavigationKey =
  | 'dashboard'
  | 'providers'
  | 'proxy'
  | 'usage'
  | 'accounts'
  | 'workspace'
  | 'settings';
```

旧页面仍然保留自己的：

```ts
LegacyPageKey
```

建立：

```text
Primary Navigation
        ↓
Navigation Mapping
        ↓
Legacy activeKey
        ↓
pageRegistry
        ↓
Existing Page
```

---

# 8. Navigation Mapping

结合实际 `pageRegistry.ts` 建立真实映射。

不要凭空创造不存在的 key。

推荐最终形成类似：

```ts
const navigationMap = {
  dashboard: ...,
  providers: ...,
  proxy: ...,
  usage: ...,
  accounts: ...,
  workspace: ...,
  settings: ...,
};
```

Workspace 可以进一步拥有：

```ts
const workspaceNavigationMap = {
  projects: ...,
  mcp: ...,
  prompts: ...,
  skills: ...,
  agents: ...,
  plugins: ...,
  sessions: ...,
};
```

实际 key 必须以现有代码为准。

---

# 9. App Shell Architecture

推荐：

```text
src/
  components/
    shell/
      AppShell.tsx
      Sidebar.tsx
      SidebarItem.tsx
      ContextHeader.tsx
      ClientSwitcher.tsx
      StatusBar.tsx
```

如果现有目录结构已有类似组件，应优先复用。

不要为了完全匹配本规划而机械搬文件。

---

# 10. AppShell

AppShell 负责：

```text
application chrome
layout
navigation presentation
context presentation
runtime summary presentation
content slot
```

不负责：

```text
Provider API
Proxy API
Antigravity API
MCP API
business mutation
```

推荐概念：

```tsx
<AppShell
  sidebar={...}
  header={...}
  statusBar={...}
>
  {activePage}
</AppShell>
```

不要把整个业务状态全部塞进 AppShell。

---

# 11. Shell DOM Hierarchy

推荐：

```text
.app-shell
├── .app-sidebar
└── .app-main
    ├── .app-context-header
    ├── .app-content
    │   └── active page
    └── .app-status-bar
```

Shell 应填满 Tauri Window：

```css
height: 100vh;
overflow: hidden;
```

滚动原则：

```text
Sidebar
  independent / fixed

Header
  fixed in shell

Content
  primary scroll container

StatusBar
  fixed in shell
```

避免整个 window body 与内部 content 同时滚动。

---

# 12. Sidebar

Sidebar 是 Phase 2 的主要视觉变化。

目标：

```text
compact
desktop-native
developer-tool
high information density
```

不要设计成大型 SaaS Dashboard Sidebar。

推荐：

```text
┌────────────────────┐
│ AI-Switcher        │
│                    │
│ ▣ Dashboard        │
│                    │
│ ◇ Providers        │
│ ↔ Proxy            │
│ ◷ Usage            │
│ ◎ Accounts         │
│ ▦ Workspace        │
│                    │
│ ────────────────── │
│ ⚙ Settings         │
└────────────────────┘
```

图标请优先使用当前已有：

```text
@ant-design/icons
```

不要为了 Sidebar 再引入一套 icon library。

---

# 13. Sidebar Width

使用 Phase 1 Layout Tokens。

例如：

```css
--app-sidebar-width
--app-sidebar-collapsed-width
```

推荐 Desktop 默认宽度约：

```text
200px ~ 240px
```

具体根据当前窗口尺寸决定。

Collapsed：

```text
52px ~ 64px
```

不要硬编码到多个组件。

---

# 14. Sidebar Collapse

支持 Sidebar Collapse。

Expanded：

```text
Icon + Label
```

Collapsed：

```text
Icon only
```

Collapsed 状态：

- Icon 必须可识别
- Hover 提供 Tooltip
- active state 必须明显
- Settings 仍保持底部位置

Collapse 状态可以进入 Zustand UI Store。

但：

不要修改业务 Store。

如果已有 UI Store，优先复用。

---

# 15. Sidebar Active State

Active Item 不应只依赖文字颜色。

建议：

```text
subtle background
+
brand foreground
+
optional left/right indicator
```

避免：

- 高饱和整块蓝色
- 大面积渐变
- 强 shadow
- 过度圆角

整体保持 Developer Tool 风格。

---

# 16. Context Header

Main Content 顶部建立统一：

```text
ContextHeader
```

它不是传统网站 Header。

主要负责：

```text
Current Section
Client Context
Page-level Actions
```

例如 Providers：

```text
Providers                        [Claude Code ▾]   [+ Add Provider]
Manage API providers and routing
```

Usage：

```text
Usage                            [Claude Code ▾]
Token usage and request analytics
```

Proxy：

```text
Proxy
Local routing and failover control
```

Client Switcher 只在需要 Client Context 的页面显示。

---

# 17. ClientSwitcher

建立：

```text
ClientSwitcher
```

支持：

```text
Claude Code
Claude Desktop
Codex
OpenCode
```

推荐使用：

- compact segmented control

或者：

- Select / Dropdown

选择取决于窗口宽度。

如果四项始终需要高频切换，优先：

```text
Segmented
```

但必须避免占据过大 Header 宽度。

---

# 18. ClientSwitcher Business Boundary

ClientSwitcher 只能调用现有 Client Context 切换机制。

禁止：

```text
重新实现 client switching
复制 client state
创建第二套 activeClient
```

如果现有 Zustand 已经维护 active client：

直接消费它。

不要创造：

```text
activeClientV2
newSelectedClient
uiClientContext
```

导致双状态源。

Single Source of Truth 必须保持。

---

# 19. Context Header Responsive Behavior

AI-Switcher 是 Desktop App，但窗口可能缩小。

当空间不足：

优先级：

```text
Page Title
>
Client Context
>
Primary Action
>
Description
>
Secondary Actions
```

必要时：

- Description 隐藏
- Secondary Action 进入 overflow
- Client Switcher 切换为 Dropdown

不要让 Header 横向溢出。

---

# 20. Status Bar

建立轻量 Status Bar。

建议高度：

```text
24px ~ 32px
```

显示高价值 Runtime 信息。

例如：

```text
● Proxy Running
Port 15721
OpenRouter
```

可以根据真实业务状态增加：

```text
Failover Ready
```

但 Phase 2 不要塞入大量信息。

---

# 21. Status Bar 数据来源

必须使用现有状态来源。

禁止为了 StatusBar：

- 新建 Proxy polling
- 新建 IPC timer
- 新建重复 React Query
- 新建第二套 runtime state

如果已有：

```text
React Query cache
Zustand
existing hook
```

优先复用。

如果无法安全获得某项状态：

Phase 2 可以暂时不显示。

不要为了显示 StatusBar 改 Proxy 业务层。

---

# 22. Dashboard Navigation

Phase 2 建立 Dashboard 一级入口。

如果当前已经有对应页面：

直接映射。

如果没有完整 Dashboard：

允许建立：

```text
DashboardShell / DashboardPlaceholder
```

但只允许：

- 页面标题
- 简单已有状态摘要
- 导航入口

不要在 Phase 2 提前实现 Phase 5 的完整 Dashboard。

---

# 23. Providers Navigation

Providers 是一级功能。

Phase 2：

- 建立入口
- 保留现有 Provider 页面
- 让 Client Context 从旧导航视觉中解耦
- 不重写 Provider Card

如果旧 Provider 页面内部仍然存在：

```text
Claude Code
Claude Desktop
Codex
OpenCode
```

旧 Tab：

允许 Phase 2 暂时保留兼容。

但如果能在不改变业务逻辑的情况下安全隐藏重复 UI，可以迁移到 ContextHeader。

禁止同时创建两套 active client state。

---

# 24. Proxy Navigation

Proxy 成为一级入口。

Phase 2 不修改：

```text
proxy startup
proxy shutdown
hot switching
failover
retry
timeout
```

只处理：

```text
navigation
shell integration
page header
runtime summary
```

---

# 25. Usage Navigation

Usage 成为一级入口。

本阶段不重构：

- Chart
- Request table
- Metrics
- Filter logic

只接入新的 Shell。

---

# 26. Accounts Navigation

Accounts 主要承载：

```text
Antigravity Account Pool
```

未来可以扩展其他账号系统。

Phase 2 只建立一级 Accounts 语义。

不要修改：

- Account rotation
- quota refresh
- OAuth
- polling

---

# 27. Workspace Navigation

Workspace 是一个聚合一级入口。

子项：

```text
Projects
MCP
Prompts
Skills
Agents
Codex Plugins
Sessions
```

推荐 Workspace 内部使用：

```text
secondary navigation
```

可以是：

- compact vertical sub-nav
- tabs
- segmented navigation

根据现有页面布局选择。

不要在主 Sidebar 中直接塞 7 个 Workspace 子项。

---

# 28. Settings

Settings 回归真正的应用设置。

未来应该主要包含：

```text
Appearance
Language
General
Update
Advanced
About
```

但：

Phase 2 不要求立刻把旧 Settings 内容全部搬走。

当前仍存在于 Settings 的 Workspace 功能可以通过兼容入口继续工作。

真正迁移在后续 Phase 完成。

---

# 29. Page Header Ownership

需要避免：

```text
ContextHeader
+
Legacy Page Header
```

产生双标题。

Phase 2 需要建立规则：

如果 Shell 已经提供：

```text
Page Title
Description
Primary Action
```

旧页面中的重复 Header 应逐步移除。

但只允许修改 presentation。

不要因此重写页面业务逻辑。

如果移除风险较高：

暂时保留并记录：

```markdown
## Deferred Header Migration
```

---

# 30. Phase 1 Primitive Reuse

Phase 2 必须优先使用已建立的：

```text
Surface
Stack
Inline
StatusBadge
Metric
IconButton
```

以及 Semantic Tokens。

禁止 Shell 又建立第二套：

```text
spacing
colors
radius
status badge
icon button
```

---

# 31. Token Usage

Sidebar / Header / StatusBar 必须使用 Semantic Token。

例如：

```css
background: var(--color-bg-surface);
color: var(--color-text-primary);
border-color: var(--color-border);
```

不要出现大量：

```css
#fff
#f5f5f5
#1677ff
rgba(...)
```

除非确实属于 Primitive Token 定义。

---

# 32. Dark Mode

新版 App Shell 必须支持 Phase 1 已建立的：

```text
[data-theme="dark"]
```

或项目实际主题机制。

必须验证：

```text
Sidebar
ContextHeader
Content
StatusBar
Active Navigation
Hover State
Divider
Tooltip
```

在 Dark Theme 下可读。

---

# 33. Window Size Strategy

Phase 2 需要检查 Tauri 当前窗口最小尺寸。

不要擅自修改 Tauri Window Configuration，除非现有尺寸导致新版 Shell 完全不可用。

UI 至少应合理支持：

```text
Compact Desktop Window
Normal Desktop Window
Large Desktop Window
```

重点不是 Mobile Responsive。

不要为了手机布局增加大量 breakpoint。

---

# 34. Content Width

不要让所有页面强制固定同一个 max-width。

推荐：

```text
Dashboard
  medium / wide

Providers
  wide

Proxy
  medium / wide

Usage
  full available width

Accounts
  wide

Workspace
  full available width

Settings
  medium
```

App Shell 提供：

```text
content container
```

页面决定：

```text
content density / max width
```

---

# 35. Scrolling

必须检查：

```text
100vh
min-height
overflow
flex: 1
min-width: 0
min-height: 0
```

React Desktop Shell 很容易产生：

```text
double scrollbar
horizontal overflow
chart overflow
table overflow
```

Phase 2 必须避免。

尤其验证：

- Usage Chart
- Request Table
- Provider Cards
- Settings long page
- Workspace long page

---

# 36. Keyboard / Accessibility

Sidebar Item：

- 使用可交互元素
- 支持 keyboard focus
- active state 可访问
- collapsed 时有 accessible name

ClientSwitcher：

- keyboard 可操作
- current client 可读

IconButton：

继续使用 Phase 1 accessibility 规范。

不要用：

```tsx
<div onClick={...}>
```

模拟按钮。

---

# 37. Animation

允许非常轻微的：

```text
sidebar collapse
hover
active transition
```

建议：

```text
120ms ~ 200ms
```

禁止：

- 大幅页面飞入
- Spring-heavy navigation
- 大量 blur animation
- flashy gradient animation

AI-Switcher 是工具软件。

---

# 38. App Branding

Sidebar 顶部可以保留：

```text
AI-Switcher
```

Collapsed 模式：

可以使用现有 Logo / App Icon。

不要在 Phase 2 重新设计品牌 Logo。

如果没有适合的 Logo：

使用简洁文字 / 现有图标。

不要阻塞 Shell 实施。

---

# 39. Phase 2 文件修改范围

允许重点修改：

```text
App root
current layout
navigation presentation
pageRegistry integration
components/shell
styles
theme
UI Zustand state
low-risk page header presentation
docs/ui-refactor
```

允许新增：

```text
src/components/shell/
src/lib/navigation.ts
```

实际目录根据项目结构调整。

---

# 40. 谨慎修改

以下文件只能在必要时轻微调整：

```text
src/lib/pageRegistry.ts
UI-related Zustand store
main.tsx
App.tsx
```

`pageRegistry.ts`：

允许：

```text
metadata
navigation grouping
mapping
label
icon metadata
```

不要改变页面加载语义。

---

# 41. 禁止修改

原则上禁止修改：

```text
src/services/ipc.ts
src/services/providers.ts
src/services/proxy.ts
src/services/antigravity.ts
src/services/mcp.ts
```

以及：

```text
Rust proxy implementation
Tauri commands
Provider data model
Antigravity polling logic
Failover algorithm
Codex authentication
```

---

# 42. 禁止 Router Migration

再次强调：

禁止：

```bash
npm install react-router-dom
```

禁止把：

```text
activeKey
pageRegistry
```

一次性替换成 URL Router。

如果未来需要 Router：

另开独立 Architecture Phase。

不属于 UI Phase 2。

---

# 43. 禁止“大爆炸式迁移”

不要一次性：

```text
重写 16 个页面
移动全部文件
重命名全部 activeKey
重构所有 CSS
删除全部旧导航代码
```

Phase 2 的核心：

```text
New Shell
+
Compatibility Layer
+
Existing Pages
```

而不是：

```text
New Shell
+
New Router
+
New Pages
+
New State
+
New Business Logic
```

---

# 44. 推荐执行顺序

严格建议按照以下顺序：

## Step 1

读取：

```text
docs/ui-refactor/phase-1-preflight.md
docs/ui-refactor/phase-1-result.md
src/lib/pageRegistry.ts
App root
navigation state
UI store
```

确认真实结构。

## Step 2

输出当前：

```text
Legacy activeKey → Page Component
```

映射。

## Step 3

设计：

```text
PrimaryNavigationKey
Navigation Metadata
Compatibility Mapping
```

## Step 4

建立：

```text
AppShell
Sidebar
```

先不加入复杂 Header。

## Step 5

将现有页面挂入：

```text
AppShell Content
```

确认全部页面仍可打开。

## Step 6

加入：

```text
ContextHeader
```

## Step 7

接入：

```text
ClientSwitcher
```

只复用现有 active client source。

## Step 8

加入：

```text
StatusBar
```

只消费现有 Runtime State。

## Step 9

加入：

```text
Workspace Navigation Compatibility
```

## Step 10

执行视觉、类型、构建、业务回归检查。

---

# 45. Phase 2 Preflight Output

修改前创建：

```text
docs/ui-refactor/phase-2-preflight.md
```

内容必须包含：

## Legacy Page Map

```text
activeKey
component
current navigation label
proposed primary section
client-scoped?
workspace-scoped?
```

## State Sources

记录：

```text
active page source
active client source
sidebar state source
proxy runtime source
active provider source
theme source
```

## Risk Map

至少标记：

```text
High
Medium
Low
```

如果发现导航与业务状态高度耦合：

不要直接拆。

先通过 compatibility adapter 隔离。

---

# 46. Visual Acceptance Criteria

Phase 2 完成后，新 UI 应明显具备统一 Desktop Shell。

必须达到：

```text
✓ Sidebar 层级清晰
✓ Current Page 清晰
✓ Client Context 清晰
✓ Runtime State 清晰
✓ Workspace 入口清晰
✓ Settings 不再承担一级导航职责
✓ Content 区域滚动正常
✓ Dark Mode 正常
✓ 小窗口不发生严重溢出
```

同时避免：

```text
✗ SaaS Admin Template 感
✗ 巨型 Sidebar
✗ 巨型 Header
✗ 巨型 Cards
✗ 过度 Shadow
✗ 过度 Gradient
✗ Navigation 与 Client Context 再次混合
```

---

# 47. Functional Regression Checklist

必须确认：

```text
Claude Code Provider
Claude Desktop Provider
Codex Provider
OpenCode Provider

Provider switching

Proxy start
Proxy stop
Proxy status

Usage page

Antigravity account page

MCP
Prompts
Skills
Agents
Plugins
Sessions

Settings

Theme switching
```

仍然可以正常访问 / 工作。

不要仅验证 Dashboard。

---

# 48. Build Verification

至少运行项目实际存在的：

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

如果命令不存在：

记录：

```text
Not available
```

不要伪造成功结果。

---

# 49. Phase 2 Deliverables

完成后创建：

```text
docs/ui-refactor/phase-2-result.md
```

必须包含：

## A. Changed Files

```text
File
Purpose
Risk
```

## B. Navigation Architecture

记录：

```text
Primary Navigation
Legacy Mapping
Client Context
Workspace Scope
Runtime State
```

## C. App Shell

说明：

```text
Sidebar
ContextHeader
ClientSwitcher
Content
StatusBar
```

实现情况。

## D. State Reuse

明确说明：

```text
activeKey source
activeClient source
runtime source
theme source
```

确保没有重复状态源。

## E. Compatibility

说明哪些 Legacy 页面仍通过 Compatibility Layer 工作。

## F. Verification

```text
TypeScript:
Build:
Lint:
Tests:
Light Theme:
Dark Theme:
Navigation:
Client Switching:
Proxy:
Workspace:
```

## G. Deferred Issues

所有 Phase 2 不应该顺便解决的问题统一记录。

## H. Phase 3 Readiness

回答：

1. Providers 页面是否已经可以开始视觉重构？
2. Provider 页面目前有哪些重复 Header / Client Tab？
3. Provider Card 当前由哪些组件组成？
4. Provider 操作逻辑位于哪里？
5. 哪些 Provider UI 可以安全重构？
6. 哪些 Provider 行为必须保持不动？
7. Phase 3 推荐修改哪些文件？

---

# 50. Git / Diff Discipline

保持 Diff 可审查。

不要：

```text
全项目格式化
无关 import 重排
无关文件重命名
无关 CSS cleanup
顺手修业务 bug
```

如果发现问题：

写入：

```markdown
## Deferred Issues
```

---

# 51. Phase 2 成功标准

Phase 2 成功并不意味着：

> 所有页面都已经变成新版设计。

Phase 2 成功意味着：

> AI-Switcher 已经拥有一套稳定的新应用骨架，旧页面可以安全运行在新骨架中，并且后续可以逐页迁移。

最终架构应接近：

```text
┌───────────────────────────────────────────────────────────┐
│ Sidebar │ Context Header                                  │
│         ├─────────────────────────────────────────────────│
│         │                                                 │
│         │                  Page Content                   │
│         │                                                 │
│         │                                                 │
│         ├─────────────────────────────────────────────────│
│         │ Runtime Status Bar                              │
└───────────────────────────────────────────────────────────┘
```

概念关系必须明确：

```text
Sidebar
=
Where am I?

Client Context
=
Which AI client am I managing?

Page Content
=
What am I working on?

Status Bar
=
What is currently running?

Workspace
=
What reusable development resources am I managing?
```

---

# 52. 最终执行指令

现在执行 Phase 2。

执行过程中遵守：

1. 先完成 `phase-2-preflight.md`
2. 不等待确认，继续实施
3. 保留 `activeKey + pageRegistry`
4. 不引入 Router
5. 不修改业务协议
6. 不创建第二套 active client state
7. 不创建第二套 proxy polling
8. 优先复用 Phase 1 Tokens / Primitives
9. 使用 Compatibility Layer 渐进迁移
10. 完成后生成 `phase-2-result.md`
11. 完成 TypeScript / Build / Lint / Test 验证
12. 把无法安全处理的问题放入 Deferred Issues

**本阶段的最高优先级是建立稳定的新 App Shell 和清晰的 Navigation Model，而不是追求所有页面一次性视觉重写。**
