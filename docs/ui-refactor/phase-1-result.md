# Phase 1 交付报告 — Semantic Design Tokens + Primitive UI Foundation

> 本文档记录 Phase 1 执行结果，作为后续 Phase 2 App Shell 重构的 UI 基础设施。

---

## A. Changed Files (修改与新增文件清单)

| 文件路径 | 类型 | 作用 / 目的 | 风险层级 |
|---|---|---|---|
| `docs/ui-refactor/phase-1-preflight.md` | 新增文档 | Phase 1 预检文件级映射 | 零风险 |
| `src/styles/tokens/primitive.css` | 新增 CSS | 原始 Design Tokens (Color, Font, Spacing, Radii, Shadows) | 零风险 |
| `src/styles/tokens/semantic.css` | 新增 CSS | 语义 Design Tokens (支持 Light / Dark Theme 模式切换) | 零风险 |
| `src/styles/tokens/components.css` | 新增 CSS | 组件级通用 CSS 规范与辅助样式 | 零风险 |
| `src/styles/tokens/index.css` | 新增 CSS | Token 入口文件 | 零风险 |
| `src/styles.css` | 修改 CSS | 引入 Token index 入口，全局挂载 Token | 极低 |
| `src/components/ui/Surface.tsx` | 新增组件 | 基础卡片/容器 Primitive 组件 | 零风险 |
| `src/components/ui/Stack.tsx` | 新增组件 | 垂直间距排版 Primitive 组件 | 零风险 |
| `src/components/ui/Inline.tsx` | 新增组件 | 水平间距与对齐 Primitive 组件 | 零风险 |
| `src/components/ui/StatusBadge.tsx` | 新增组件 | 包含 Accessibility 支持的统一状态徽章 | 零风险 |
| `src/components/ui/Metric.tsx` | 新增组件 | 指标 KPI 统一渲染组件 | 零风险 |
| `src/components/ui/IconButton.tsx` | 新增组件 | 包含 aria-label & Tooltip 的图标按钮组件 | 零风险 |
| `src/components/ui/index.ts` | 新增导出 | Primitive UI 组件统一导出入口 | 零风险 |
| `src/pages/EnvironmentPage.tsx` | 低风险 Dogfood | 使用 `StatusBadge` 替代内联 Ping 结果 `Tag` 完成 Dogfood 验证 | 极低 |

---

## B. Token Architecture (Design Token 架构说明)

采用了标准四层 Token 分层架构：

1. **Primitive Tokens (`primitive.css`)**: 纯物理数值命名，包含 Neutral Slate/Gray、Blue/Green/Red/Amber 色彩梯队，4px 基准间距步进 (`--space-1` 至 `--space-8`)、圆角与阴影。
2. **Semantic Tokens (`semantic.css`)**: 映射业务语义，支持 `[data-theme="dark"]` 动态覆盖，包含 `--color-bg-app`, `--color-bg-surface`, `--color-border`, `--color-brand`, `--color-success`, `--color-danger` 等。
3. **Component Tokens (`components.css`)**: 定义 `.ui-surface`, `.ui-card`, `.ui-badge`, `.ui-status-dot` 等基础组件样式模板。
4. **Antd Mapping (兼容模式)**: 保留 `App.tsx` 中 ConfigProvider 的 Ant Design 官方算法与 theme token 挂载，不破坏现有的 Antd 组件渲染。

---

## C. New Primitive Components (新增 Primitive 组件)

1. **Surface (`src/components/ui/Surface.tsx`)**:
   - 目的：取代散落的 `Card` 或带 border 的 `div`
   - API: `variant` ('default' | 'subtle' | 'elevated'), `padding` ('none' | 'sm' | 'md' | 'lg')
2. **Stack (`src/components/ui/Stack.tsx`)**:
   - 目的：统一垂直布局，消除内联 `marginBottom: 16` 等 hard-code
   - API: `gap` ('none' | 'xs' | 'sm' | 'md' | 'lg' | 'xl'), `align`, `justify`
3. **Inline (`src/components/ui/Inline.tsx`)**:
   - 目的：统一水平布局与对齐
   - API: `gap`, `align`, `justify`, `wrap`
4. **StatusBadge (`src/components/ui/StatusBadge.tsx`)**:
   - 目的：解决状态表达不一致问题，遵循 Accessibility 规范（同时包含文本/图标与状态圆点）
   - API: `status` ('running' | 'stopped' | 'healthy' | 'slow' | 'warning' | 'error' | 'active' | 'current'), `label`, `showDot`
5. **Metric (`src/components/ui/Metric.tsx`)**:
   - 目的：标准 KPI 数值块展示
   - API: `label`, `value`, `supporting`
6. **IconButton (`src/components/ui/IconButton.tsx`)**:
   - 目的：统一纯图标按钮的交互与 aria-label 属性
   - API: `icon`, `title`, `danger`, `disabled`, `loading`, `onClick`

---

## D. Dogfood Migration (Dogfooding 迁移验证)

- **验证区域**: `src/pages/EnvironmentPage.tsx` 中的 Ping 状态展示。
- **验证内容**: 使用 `<StatusBadge status="healthy" label={`ping: ${pingResult}`} />` 替换原有 `Tag`。
- **验证结果**: 状态表达清晰，Semantic Color 渲染正常，无样式破裂与业务破坏。

---

## E. Phase 2 Readiness (Phase 2 重构准备就绪评估)

1. **App Shell 是否可以开始重构？**:
   - **是**。Design Token（特别是 `--app-sidebar-width`, `--app-header-height`）与 Primitive 组件已准备就绪，Phase 2 可以直接构建 `Sidebar.tsx`, `ContextHeader.tsx` 和 `StatusBar.tsx`。
2. **阻碍 Phase 2 的现有布局因素**:
   - 当前 `AppLayout.tsx` 采用无 Sidebar 的 flex column 模式，Phase 2 需要在保留现有页面内容的同时演进为具有侧栏和顶栏的响应式 Grid/Flex 容器。
3. **Phase 2 建议修改的文件**:
   - `src/components/AppLayout.tsx`
   - 新建 `src/components/layout/Sidebar.tsx`
   - 新建 `src/components/layout/ContextHeader.tsx`
   - 新建 `src/components/layout/StatusBar.tsx`
4. **Phase 2 仍然不得触碰的区域**:
   - 业务逻辑 Services (`src/services/*`)
   - Zustand 业务逻辑 Store（除全局 activeClient/layout UI 状态外）
   - Tauri IPC 指令与后端 Handler

---

*Phase 1 交付时间: 2026-08-10*
