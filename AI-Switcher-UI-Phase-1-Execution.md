# AI-Switcher UI Refactor — Phase 1 Execution

你已经完成 Phase 0 架构审计。

现在开始执行：

> Phase 1 — Semantic Design Tokens + Primitive UI Foundation

本阶段允许修改 UI 基础设施，但禁止改变任何业务行为、IPC 协议、业务状态模型、代理逻辑、Provider 切换逻辑和页面功能。

---

# 0. Phase 1 Preflight

在修改代码前，请先快速补充 Phase 0 中缺失的“文件级映射”。

不要生成长篇文档，只需要确认并记录：

1. React 应用入口文件
2. 当前 App 根组件
3. 当前主导航 / activeKey 控制位置
4. `pageRegistry.ts` 的具体页面注册结构
5. Zustand store 所在文件
6. React Query Provider 初始化位置
7. antd ConfigProvider / ThemeProvider 当前所在位置
8. 全局 CSS / reset / variables 文件
9. 当前共享 UI 组件目录
10. 当前 Provider Card / Section / Empty State / Modal / Form 等重复组件所在位置
11. 当前页面最大量使用的 spacing / radius / color hard-code
12. 是否已经存在 dark mode token 或主题变量

将结果写入：

`docs/ui-refactor/phase-1-preflight.md`

如果 `docs/ui-refactor` 不存在，可以创建。

完成后继续执行 Phase 1，不需要等待确认。

---

# 1. Phase 1 总目标

本阶段只建立新的 UI Foundation。

目标：

- 建立 Semantic Design Token 层
- 建立少量可复用 Primitive UI
- 建立统一样式规范
- 为 Phase 2 App Shell 重构提供基础
- 尽可能兼容现有 antd
- 不大规模改现有页面
- 不改变业务行为

本阶段不是视觉重写阶段。

不要重写 Provider 页面。  
不要重写 Proxy 页面。  
不要重写 Usage 页面。  
不要重写 Account 页面。

---

# 2. 强制保护项

以下内容不得修改其业务行为。

## Client Isolation

必须保持：

- Claude Code
- Claude Desktop
- Codex
- OpenCode

四类客户端配置隔离逻辑。

不得合并配置状态。  
不得改变 active client 的业务语义。

## Codex

不得修改：

- 官方登录保持逻辑
- Bearer Token 动态注入逻辑
- OpenAI Responses 相关协议处理
- Codex Provider compatibility 行为

## Antigravity

不得修改：

- Account Pool
- Active Account
- Account rotation
- quota refresh
- 45 秒首次刷新
- 5 分钟事件轮询
- Google OAuth / Token 行为

## Proxy

不得修改：

- local proxy startup / shutdown
- provider hot switch
- retry mechanism
- failover chain
- HTTP status retry configuration
- streaming timeout behavior

## IPC

不得绕过：

`src/services/ipc.ts`

现有：

```ts
call<T>(cmd, args)
```

继续作为统一 Tauri IPC Gateway。

不得让 React Component 直接调用 Tauri invoke。

---

# 3. 不要做架构迁移

Phase 1 禁止引入：

- react-router-dom
- Redux
- MobX
- Tailwind CSS
- shadcn/ui
- Chakra
- Material UI
- 新 CSS-in-JS framework

不要替换：

- Zustand
- React Query
- antd
- recharts
- pageRegistry

这些不是本轮 UI 重构目标。

---

# 4. Design Token Architecture

建立 Semantic Design Tokens。

推荐结构：

```text
src/
  styles/
    tokens/
      primitive.css
      semantic.css
      components.css
      index.css
```

如果项目现有 CSS 架构明显更适合其他位置，可以调整，但必须保持：

Primitive Token  
↓  
Semantic Token  
↓  
Component Token  
↓  
Component Style

四层关系。

---

# 5. Primitive Tokens

Primitive Token 不应该直接表达业务语义。

例如：

```css
--gray-50
--gray-100
--gray-200
--gray-300
--gray-400
--gray-500
--gray-600
--gray-700
--gray-800
--gray-900

--blue-50
--blue-100
--blue-500
--blue-600

--green-50
--green-500
--green-600

--red-50
--red-500
--red-600

--amber-50
--amber-500
```

不要求必须完全使用这个命名。

需要结合当前 UI 与 antd 色彩体系设计。

---

# 6. Semantic Tokens

UI Component 不应直接大量引用 Primitive Color。

建立语义层，例如：

```css
--color-bg-app
--color-bg-surface
--color-bg-subtle
--color-bg-elevated

--color-border
--color-border-subtle
--color-border-strong

--color-text-primary
--color-text-secondary
--color-text-tertiary
--color-text-disabled

--color-brand
--color-brand-hover
--color-brand-active

--color-success
--color-warning
--color-danger
--color-info
```

另外需要建立：

```css
--shadow-xs
--shadow-sm
--shadow-md

--radius-xs
--radius-sm
--radius-md
--radius-lg
--radius-xl

--space-1
--space-2
--space-3
...
```

避免：

```css
margin: 13px;
padding: 17px;
border-radius: 7px;
```

这种无体系 hard-code 持续扩散。

---

# 7. Typography Tokens

建立统一 Typography Scale。

至少考虑：

```text
Page Title
Section Title
Card Title
Body
Secondary Body
Caption
Metric
Code / Endpoint
```

例如可以设计：

```css
--font-size-xs
--font-size-sm
--font-size-md
--font-size-lg
--font-size-xl
--font-size-2xl

--font-weight-regular
--font-weight-medium
--font-weight-semibold

--line-height-tight
--line-height-normal
```

不要为了 UI 重构强制引入新的网络字体。

优先使用系统字体栈。

---

# 8. Layout Tokens

考虑 AI-Switcher Desktop App 的实际使用环境。

建立：

```css
--app-sidebar-width
--app-sidebar-collapsed-width

--app-header-height
--app-statusbar-height

--content-max-width

--page-padding-x
--page-padding-y

--section-gap
--card-gap
```

这些 Token Phase 1 可以先定义。

Phase 2 再真正落地 App Shell。

---

# 9. Dark Mode Architecture

即使 Phase 1 不要求完整重做 Dark Mode，也必须让 token architecture 支持：

```css
:root {
   ...
}

[data-theme="dark"] {
   ...
}
```

或者使用项目现有主题标识。

禁止以后为了 Dark Mode 再复制整套 Component CSS。

---

# 10. antd Integration

当前项目已经依赖：

`antd v6`

因此 Phase 1 应当充分利用：

`ConfigProvider`

将 Semantic Tokens 与 antd Theme Token 对齐。

例如考虑映射：

- colorPrimary
- colorBgContainer
- colorBgLayout
- colorBorder
- colorText
- colorTextSecondary
- borderRadius
- controlHeight
- fontSize

但不要：

1. 强行覆盖所有 antd tokens
2. 创建非常庞大的 theme.ts
3. 深度依赖 `.ant-*` CSS selector hack
4. 大面积 `!important`

如果需要覆盖 antd，应优先使用官方 Theme Token API。

---

# 11. Primitive UI Components

Phase 1 只创建高复用、小职责组件。

不要一次性建立几十个组件。

推荐建立：

```text
src/components/ui/
```

至少评估以下组件：

```text
Surface
Card
Section
Stack
Inline
Badge
StatusDot
Metric
IconButton
EmptyState
PageHeader
SectionHeader
```

最终不一定全部创建。

原则：

只有真正能够减少重复代码或统一视觉规范的 Primitive 才创建。

---

# 12. 推荐 Primitive 定义

## Surface

用于：

- cards
- settings panels
- information sections
- dashboard blocks

支持：

```ts
variant:
  default
  subtle
  elevated

padding:
  none
  sm
  md
  lg
```

## Stack

用于统一 vertical layout。

类似：

```tsx
<Stack gap="md">
  ...
</Stack>
```

不要在页面中到处：

```tsx
<div style={{ marginBottom: 16 }}>
```

## Inline

处理水平布局：

```tsx
<Inline gap="sm" align="center">
```

## StatusBadge

统一：

- Running
- Stopped
- Healthy
- Warning
- Failed
- Active
- Current

避免每个页面各自定义绿点、红点和 badge。

## Metric

统一 Usage / Dashboard 中：

```text
Label
Value
Supporting information
```

例如：

```text
Estimated Cost
$67.2928
```

## IconButton

统一当前大量：

- edit
- copy
- delete
- switch
- refresh
- test

icon 操作。

必须：

- 有 tooltip 或 aria-label
- 统一 hit area
- hover / active / disabled 一致

---

# 13. 不要建立过度抽象

禁止类似：

```tsx
<UniversalCard
  type="provider"
  variant="usage"
  mode="proxy"
  density="..."
  ...
/>
```

这种“大一统组件”。

优先：

small primitives + domain components。

例如以后：

```text
ui/Surface
ui/StatusBadge

providers/ProviderCard
proxy/FailoverChain
accounts/QuotaMeter
```

---

# 14. CSS 使用策略

优先沿用项目现有 CSS 方案。

如果当前项目使用普通 CSS / CSS Modules，则继续使用。

禁止因为 Phase 1 引入新的大型 CSS Framework。

降低：

```tsx
style={{
  padding: 14,
  marginTop: 12,
  color: '#666'
}}
```

这类 inline hard-code。

允许非常动态的运行时 style 保留。

---

# 15. Phase 1 Dogfooding

Phase 1 完成 Primitive 后，需要选择 **一个低风险区域** 做非常小范围迁移验证。

建议优先：

- Settings 某个纯展示 section

或者：

- 一个简单 Empty State

或者：

- Status Badge

不要选择：

- Provider 主卡片
- Proxy 控制中心
- Antigravity Account
- Usage Chart

作为 Phase 1 dogfood。

目标只是确认新 Token + Primitive 能正常工作。

---

# 16. TypeScript

所有新增 Primitive：

- 使用 TypeScript
- Props 类型明确
- 不使用 `any`
- 避免过度 Generic
- 支持 className
- 必要时支持 children

例如：

```ts
interface SurfaceProps {
  children: React.ReactNode;
  className?: string;
  variant?: 'default' | 'subtle' | 'elevated';
}
```

---

# 17. Accessibility

Phase 1 必须开始建立基础 accessibility。

Icon-only button：

必须拥有：

```tsx
aria-label
```

或者 Tooltip + accessible name。

颜色不能作为状态的唯一表达方式。

例如：

错误状态：

❌ 只有红色圆点

应当：

✔ 红色圆点 + Failed

---

# 18. Desktop Density

AI-Switcher 是 Desktop Developer Tool。

不要把 UI 做成大型 SaaS Dashboard。

目标：

- compact
- information dense
- readable
- efficient

避免：

- 过大的卡片
- 过大的标题
- 大面积 hero UI
- 手机端风格 spacing
- 每个按钮都 oversized

---

# 19. Visual Direction

目标视觉：

```text
Developer Tool
+
Desktop Control Center
+
Modern Neutral UI
```

而不是：

```text
Marketing SaaS
```

整体：

- 中性色背景
- 清晰 surface hierarchy
- 少量 brand blue
- 状态色克制
- border 比 shadow 更重要
- shadow 只用于真正 elevation
- typography 层级明显
- card density 高于常规 SaaS

---

# 20. Phase 1 文件修改边界

允许：

```text
styles
theme
components/ui
App theme bootstrap
少量低风险页面用于 dogfood
docs/ui-refactor
```

谨慎：

```text
App.tsx
main.tsx
store
```

仅允许为了 Theme / Token Foundation 所必须的轻微修改。

禁止修改业务逻辑文件：

```text
src/services/providers.ts
src/services/proxy.ts
src/services/antigravity.ts
src/services/ipc.ts
src/services/mcp.ts
```

除非只是 TypeScript import 整理且完全不改变行为。

最好完全不碰。

---

# 21. 禁止行为

禁止：

- 顺便重构 services
- 顺便重构 hooks
- 顺便改 Provider 数据模型
- 顺便改 IPC args
- 顺便优化 Proxy 算法
- 顺便替换 Zustand
- 顺便换 Router
- 顺便删除旧页面
- 顺便修改接口命名
- 全项目 prettier 重写导致巨大 diff
- 为了统一 UI 一次性改所有页面

Phase 1 必须保持小 diff、低风险。

---

# 22. Testing

完成 Phase 1 后至少执行：

```bash
npm run typecheck
npm run build
```

如果 package.json 有：

```bash
npm run lint
npm run test
```

也执行。

如果命令不同，请根据项目实际 scripts 执行。

不得为了让测试通过删除测试。

---

# 23. Visual Regression

至少人工确认：

- 主页面仍可打开
- Provider 页面仍正常
- Proxy 页面仍正常
- Usage 页面仍正常
- Antigravity 页面仍正常
- Settings 页面仍正常
- Light Theme 正常

如果现有 Dark Theme 可用：

- Dark Theme 不应明显退化

---

# 24. Phase 1 Deliverables

完成后输出：

## A. Changed Files

列出：

```text
file
purpose
risk
```

## B. Token Architecture

说明：

```text
Primitive
Semantic
Component
Antd Mapping
Dark Theme
```

## C. New Primitive Components

列出：

```text
Component
Purpose
API
Used by
```

## D. Dogfood Migration

说明哪个现有 UI 已迁移使用新基础设施。

## E. Verification

输出：

```text
TypeScript:
Build:
Lint:
Tests:
Manual:
```

## F. Phase 2 Readiness

回答：

1. App Shell 是否已经可以开始重构？
2. 哪些旧布局 CSS 会阻碍 Phase 2？
3. Phase 2 建议修改哪些文件？
4. 哪些文件 Phase 2 仍然不能碰？

并创建：

`docs/ui-refactor/phase-1-result.md`

---

# 25. Git / Diff Discipline

保持 Phase 1 diff 可审查。

如果发现一个与 Phase 1 无关的问题：

不要顺便修复。

记录到：

```markdown
## Deferred Issues
```

即可。

---

# 26. 最终执行原则

本阶段最重要的不是“看起来变化很大”。

而是：

> 建立一套 Phase 2 ~ Phase 9 都能持续使用的 UI 基础设施。

如果最终截图看起来与当前版本差异不大，这是正常的。

Phase 1 成功标准是：

- Token 体系稳定
- Primitive 体系清晰
- antd 集成正确
- Theme architecture 可扩展
- 没有业务行为变化
- 没有巨大页面重写
- 后续 App Shell 可以直接建立在这套 Foundation 上

现在执行 Phase 1。
