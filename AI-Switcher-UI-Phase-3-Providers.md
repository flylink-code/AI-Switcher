# AI-Switcher UI Refactor — Phase 3 Execution

## Providers Experience + Provider Domain UI

> 前置条件：Phase 0、Phase 1、Phase 2 已完成。Phase 1 已建立 Semantic Design Tokens 与 Primitive UI；Phase 2 已建立 AppShell / Sidebar / ContextHeader / ClientSwitcher / StatusBar，并继续保留 activeKey + pageRegistry.ts。

## 0. 核心原则

Phase 3 是 **Providers UI / UX Refactor**，不是 Provider Business Logic Rewrite。

必须保持：
- 现有 Provider 状态与 Service
- Client Isolation
- Provider Switching 行为
- IPC 协议
- Codex compatibility
- API Key / Token 处理方式

禁止为了新版 UI 重写 Provider API、认证、数据模型或 Proxy 联动逻辑。

## 1. 总目标

将 Providers 页面重构为核心 Provider Control Center：

```text
Providers
├── Client Context
├── Provider Toolbar
├── Current Provider
├── Provider List
├── Add Provider
└── Edit Provider
```

要求：
- 当前 Provider 一眼可见
- Endpoint / Model / Provider 类型层级清晰
- 高频操作明确
- 危险操作降级
- Desktop 高信息密度
- Light / Dark Theme 完整
- 复用 Phase 1 Tokens / Primitives
- 复用 Phase 2 ContextHeader / ClientSwitcher
- 清除重复 Client Tabs 与 Page Header

## 2. Phase 3 Preflight

修改前读取：

```text
docs/ui-refactor/phase-1-preflight.md
docs/ui-refactor/phase-1-result.md
docs/ui-refactor/phase-2-preflight.md
docs/ui-refactor/phase-2-result.md
```

检查 Provider page/components/hooks/store/React Query、`src/services/providers.ts`、`pagePreferencesStore`、`ContextHeader`、`ClientSwitcher`。

创建：

`docs/ui-refactor/phase-3-preflight.md`

记录：

### Provider Page Map

```text
File
Responsibility
Client scoped?
Business logic?
Presentation only?
Safe to refactor?
```

### Provider Data Model

记录真实字段，不修改模型。字段名称以代码为准，禁止为了匹配设计新增字段。

### Provider Actions

定位真实 Add / Edit / Delete / Switch / Test / Enable / Disable / Copy 等操作，并记录：

```text
UI Entry
Hook / Handler
Service
IPC Command
Risk
```

### Client Scope

确认 Claude Code / Claude Desktop / Codex / OpenCode 如何决定当前 Provider List，不得猜测。

## 3. 强制业务保护

原则上不得修改：

```text
src/services/providers.ts
src/services/ipc.ts
```

禁止改变：
- Provider IPC commands
- Provider persistence / serialization
- Provider switching algorithm
- Client config synchronization
- API key persistence
- validation business rules
- Proxy hot switch behavior

### Codex 特别保护

不得破坏：
- 官方登录
- 第三方 Provider
- Bearer Token 动态注入
- Responses API compatibility
- Base URL / Endpoint compatibility
- 现有 Codex 特殊配置

UI 可以统一，但业务模型不得被强行统一。

## 4. Client Context

Phase 2 的 `ContextHeader > ClientSwitcher` 成为 Providers 的唯一主要 Client Context 入口。

旧页面中的 Claude Code / Claude Desktop / Codex / OpenCode Tabs 若与其重复，应安全移除。

必须复用现有 active client 单一状态源。

禁止创建 `providerSelectedClient`、`activeClientV2` 等第二套状态。

## 5. 页面结构

推荐：

```text
ContextHeader
├── Providers
├── Description
├── ClientSwitcher
└── + Add Provider

Page Content
├── Provider Toolbar
├── Current Provider
└── Other Providers
```

若 Provider 数量较少，不强制拆物理列表，但 Current Provider 必须视觉明确。

ContextHeader 已拥有标题后，Content 不再显示重复的大标题。

## 6. Provider Toolbar

仅基于已有前端数据提供轻量：
- Search
- Status Filter
- Type Filter（真实字段支持时）
- Sort

Search 可覆盖 Provider Name / Endpoint / Model。

禁止新增后端搜索 API。

如果没有可靠 Error / Health 状态，不得伪造状态。

若现有顺序有业务意义，不得改变默认顺序。

## 7. Provider Card

建立 Domain Component，例如：

`src/components/providers/ProviderCard.tsx`

它应消费 Phase 1 的 `Surface / Stack / Inline / StatusBadge / IconButton`，而不是重新造 Primitive。

推荐层级：

```text
┌─────────────────────────────────────────────┐
│ [Logo] OpenRouter                [CURRENT] │
│        OpenAI Compatible                   │
│                                             │
│ Endpoint                                    │
│ https://openrouter.ai/api/v1        [Copy] │
│                                             │
│ Model                                       │
│ anthropic/claude-sonnet-4                   │
│                                             │
│                            Test Edit   •••  │
└─────────────────────────────────────────────┘
```

一级：Provider Name / Current State / Identity  
二级：Endpoint / Model / Type  
三级：兼容模式等已有 metadata。

不存在的数据不得创建。

### Current Provider

推荐：
- `StatusBadge: Current`
- subtle brand border
- very subtle brand background

禁止整卡高饱和、Glow 或巨型 Success 状态。

### Logo

优先现有 Logo/Icon；否则使用 generic icon 或 initials。禁止引入重量级 Logo library 或网络动态 Logo。

### Endpoint

应可读、可截断、可复制，hover 可查看完整值。

### API Key

不得在 Card 显示完整 Key。维持或提高当前安全性，例如 mask；不得改变存储方式。

### Model

有值才展示，无此概念时自然缺省。

## 8. Card Actions

高频：
- Switch / Use
- Test
- Edit

低频/危险：
- Delete
- Duplicate
- Advanced

避免所有操作同权横排。

非 Current Provider 显示明确 Switch/Use；Current Provider 显示状态而非无意义 Switch。

所有操作必须调用现有 handler/service。

Test 复用现有测试逻辑，提供 Idle / Testing / Success / Failed UI；禁止新建后端 Health API。

Delete 必须确认并显示 Provider Name；Current Provider 删除规则遵循现有业务。

## 9. Add / Edit Provider

ContextHeader 提供 `+ Add Provider`。

优先使用 antd Drawer 建立统一 Provider Editor，例如：

`src/components/providers/ProviderEditorDrawer.tsx`

如果现有 Modal/Form 高度耦合，可保留 Modal；不要为了 Drawer 重写业务逻辑。

推荐表单信息架构：

```text
Provider
├── Identity
├── Connection
│   ├── Endpoint
│   └── API Key
├── Model
├── Client-specific Options
└── Advanced
```

实际字段必须来自现有模型。

复杂 Headers / Compatibility / Protocol / Custom Params / Codex Options 可使用 Progressive Disclosure，但日常必填字段不能隐藏。

若已有 Provider Presets 可以改善选择器；若没有，不新造 preset system。

Validation、Save、Query invalidation 全部复用现有机制。

Save 时应避免重复提交，并有 loading 状态。

## 10. UI States

必须区分：

### Loading
保持 Shell/Header 存在，可使用 Skeleton 或现有 Loading。

### Empty
```text
No providers configured
Add a provider to connect this client to an AI service.
[Add Provider]
```

### Filtered Empty
```text
No providers match your search.
[Clear filters]
```

### Error
```text
Couldn't load providers
[Retry]
```

不得把错误伪装成 0 Providers。

Backend/IPC 不可用只有在现有架构能可靠判断时才展示，禁止为此新建 Health Subsystem。

## 11. Provider List Layout

Desktop 优先：

```text
Normal/Large → 2 columns
Compact → 1 column
```

不要强制 3~4 列，因为 Endpoint/Model 需要宽度。

保持紧凑，使用 Phase 1 spacing tokens，避免巨型卡片、Logo 与空白。

Phase 3 不需要独立 Provider Detail Page，优先 Card + Editor Drawer。

## 12. Context-specific UI

不同 Client 可以有不同字段和能力。

目标是 **Visual Consistency**，不是 **Business Model Uniformity**。

Codex 等特殊字段必须根据真实业务模型自然呈现。

## 13. Domain Component Boundary

可按实际需要建立：

```text
src/components/providers/
  ProviderCard.tsx
  ProviderList.tsx
  ProviderToolbar.tsx
  ProviderEditorDrawer.tsx
  ProviderEmptyState.tsx
```

不要机械创建空壳组件。

推荐职责：

```text
ProviderPage          orchestration
ProviderToolbar       filter presentation
ProviderList          list presentation
ProviderCard          provider presentation/actions
ProviderEditorDrawer  add/edit presentation
```

ProviderCard 推荐接收 `provider / isCurrent / onSwitch / onEdit / onDelete / onTest` 等 props。

组件内部禁止直接绕过业务层调用 Tauri IPC。

## 14. Existing Foundation Reuse

必须优先消费 Phase 1：
- Surface
- Stack
- Inline
- StatusBadge
- Metric
- IconButton
- Semantic Tokens

继续使用 antd 的 Button / Input / Select / Dropdown / Drawer / Form / Tooltip / Popconfirm / Skeleton 等。

禁止大量 `.ant-* !important` hack。

Phase 3 不应继续大规模重构 AppShell，也不要扩建一个新的 Generic UI Framework。

## 15. Dark Mode / Accessibility / Security

必须验证 Provider Card、Toolbar、Search、Dropdown、Drawer、Form、API Key Input、Endpoint、StatusBadge、Delete Confirmation 的 Dark Mode。

要求：
- Icon Button 有 `aria-label`
- 操作 keyboard accessible
- Search 有 accessible name
- 状态使用文字 + 色彩
- Drawer focus 正常
- Delete confirmation 可键盘操作

敏感信息包括 API Key / Bearer Token / OAuth Token / Headers。

禁止：
- console.log secret
- toast secret
- raw error 泄露 token
- Card 显示完整 token
- HTML title 泄露 secret

Endpoint / Model 等可提供轻量 Copy；API Key Copy 能力遵循现有产品行为。

## 16. Legacy Cleanup

可以删除已确认只属于旧 Provider UI 且无其他依赖的 CSS。

不确定则保留并记录 Deferred。

旧 Client Tabs 的迁移顺序：

```text
1. 确认与 ClientSwitcher 使用同一状态源
2. ContextHeader 成为主入口
3. 移除重复 Tabs
4. 验证四个 Client 切换
5. 验证 Provider 内容同步更新
```

不得先删除再重造状态。

## 17. 文件修改边界

允许重点修改：

```text
Provider page
Provider presentation components
Provider CSS
components/providers
ContextHeader page-action integration
Provider UI i18n
docs/ui-refactor
```

仅必要时轻微修改：

```text
pageRegistry metadata
layout integration
UI-only store
```

原则上禁止修改：

```text
src/services/ipc.ts
src/services/providers.ts
src/services/proxy.ts
src/services/antigravity.ts
Rust backend
Tauri commands
Provider persistence model
```

若 UI 必须依赖业务层变更，停止该部分并记录：

`## Deferred Business Dependency`

## 18. 推荐实施顺序

1. 完成 `phase-3-preflight.md`
2. 标记现有 provider query/mutations/client state/switch/delete/test/form submit
3. 重构 Provider Page Skeleton
4. 建立 ProviderCard
5. 接入真实 Current Provider
6. 迁移 Switch/Test/Edit/Delete
7. 建立安全的前端 Toolbar
8. Add/Edit 接入统一 Editor Shell
9. 完成 Loading/Empty/Filtered Empty/Error
10. 移除重复 Client Tabs/Page Header
11. 验证 Theme/Accessibility/Security
12. 执行完整业务回归

## 19. Functional Regression Matrix

至少验证：

| Client | List | Add | Edit | Switch | Delete | Test |
|---|---|---|---|---|---|---|
| Claude Code | ✓ | ✓ | ✓ | ✓ | ✓ | ✓/N/A |
| Claude Desktop | ✓ | ✓ | ✓ | ✓ | ✓ | ✓/N/A |
| Codex | ✓ | ✓ | ✓ | ✓ | ✓ | ✓/N/A |
| OpenCode | ✓ | ✓ | ✓ | ✓ | ✓ | ✓/N/A |

原本不支持的功能标记 `N/A — unsupported by existing implementation`，禁止为了矩阵全绿新增业务能力。

Codex 额外验证：
- Official Login
- Third-party Provider
- Bearer Token
- Current Provider
- Provider Switching
- compatibility behavior

Proxy 联动至少确认：
- Proxy stopped → switch provider
- Proxy running → switch provider
- Active Provider 正确
- StatusBar 正确

不得修改 Proxy 逻辑。

## 20. Build / Visual Verification

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

也执行，不得伪造结果。

人工验证：
- Light / Dark
- Sidebar Expanded / Collapsed
- 四个 Client
- 0 / 1 / 多 Provider
- Long Provider Name / Endpoint / Model
- Current / Non-current
- Editor
- Validation Error
- Delete Confirmation

## 21. Phase 3 Deliverables

创建：

`docs/ui-refactor/phase-3-result.md`

必须包含：

### A. Changed Files
```text
File
Purpose
Risk
```

### B. Provider Architecture
```text
ProviderPage
ProviderCard
ProviderToolbar
ProviderEditor
State Sources
Service Boundary
```

### C. Business Logic Reuse
明确 Query / Mutation / Switch / Test / Delete / Client source，证明没有第二套业务实现。

### D. UI States
记录实际支持的 Loading / Empty / Filtered Empty / Error / Current / Disabled / Testing。

### E. Client Regression
输出四 Client 验证结果。

### F. Security
说明 API key masking、secret handling、copy behavior、error handling。

### G. Verification
```text
TypeScript:
Build:
Lint:
Tests:
Light:
Dark:
Claude Code:
Claude Desktop:
Codex:
OpenCode:
Proxy interaction:
```

### H. Deferred Issues
记录所有不属于 Phase 3 的问题。

## 22. Phase 4 Proxy Readiness

Phase 3 完成后只读审计下一阶段 Proxy Control Center，并回答：

1. 当前 Proxy 页面由哪些文件组成？
2. Running / Stopped 状态来自哪里？
3. Start / Stop handler 在哪里？
4. Port 设置来自哪里？
5. Active Provider 与 Proxy 的关系是什么？
6. Failover Chain UI 在哪里？
7. Retry / Timeout / Status Code 配置 UI 在哪里？
8. 哪些 Proxy UI 只是 presentation？
9. 哪些与业务逻辑高度耦合？
10. Phase 4 哪些文件可安全重构？
11. 哪些文件绝对不能修改？
12. StatusBar 是否复用现有 Proxy 状态且无额外 polling？

只记录，不提前重构 Proxy。

## 23. Git / Diff Discipline

保持 Phase 3 Diff 聚焦。

不要：
- 全项目格式化
- 无关重命名
- 重构 services
- 重构 Proxy / Accounts / Workspace
- 修改 Rust
- 升级 dependencies
- 替换 UI framework

发现无关问题写入 `## Deferred Issues`。

## 24. Phase 3 成功标准

代码关系必须保持：

```text
New Provider UI
        ↓
Existing Hooks / State
        ↓
Existing Services
        ↓
Existing IPC
        ↓
Existing Tauri Backend
```

禁止形成 New UI + New Business Logic + New Provider State + New IPC 的并行体系。

最终用户应能快速回答：

```text
Where am I?          → Providers
Which client?        → ContextHeader / ClientSwitcher
Which provider?      → Current state immediately visible
How do I switch?     → One obvious action
How do I configure?  → Edit
How do I add one?    → ContextHeader primary action
What is dangerous?   → Secondary + confirmed actions
```

## 25. 最终执行指令

现在执行 Phase 3。

必须遵守：

1. 先完成 `docs/ui-refactor/phase-3-preflight.md`
2. 然后继续实施，不等待确认
3. Provider UI 可以明显重构
4. Provider Business Logic 不允许重写
5. ClientSwitcher 继续使用 Phase 2 单一状态源
6. 不创建第二套 Provider state/API
7. 不绕过 `src/services/ipc.ts`
8. 不修改 Proxy / Antigravity / Workspace 业务
9. 优先消费 Phase 1 Primitives
10. 优先消费 Phase 2 App Shell
11. 删除重复 Header/Client Tabs 前确认状态来源一致
12. 完成四 Client 回归
13. 单独验证 Codex 特殊行为
14. 生成 `docs/ui-refactor/phase-3-result.md`
15. 输出 Phase 4 Proxy Readiness

**最高优先级：在完全保护 Provider 业务行为的前提下，把 Providers 页面真正重构成清晰、紧凑、高效的 Desktop Provider Control Center。**
