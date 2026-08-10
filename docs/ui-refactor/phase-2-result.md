# Phase 2 交付报告 — App Shell 重构与导航模型迁移

> 本文档记录 Phase 2 的执行结果。系统已成功演进为拥有统一侧边栏 (Sidebar)、顶栏上下文 Header (ContextHeader)、客户端切换器 (ClientSwitcher) 和底部固定状态栏 (StatusBar) 的桌面级 Developer Tool 应用骨架。

---

## A. Changed Files (修改与新增文件清单)

| 文件路径 | 类型 | 作用 / 目的 | 风险层级 |
|---|---|---|---|
| `docs/ui-refactor/phase-2-preflight.md` | 新增文档 | Phase 2 预检与 Legacy 映射日志 | 零风险 |
| `src/components/layout/ClientSwitcher.tsx` | 新增组件 | 客户端上下文 (Client Context) 切换器 | 零风险 |
| `src/components/layout/StatusBar.tsx` | 新增组件 | 底部固定 Runtime 运行状态栏 | 零风险 |
| `src/components/layout/ContextHeader.tsx` | 新增组件 | 顶栏 Context Header（包含 Section 标题、描述与 Client Switcher） | 零风险 |
| `src/components/layout/Sidebar.tsx` | 新增组件 | 支持 220px 展开 / 60px 折叠的桌面级侧栏导航 | 零风险 |
| `src/components/layout/AppShell.tsx` | 新增组件 | 全局响应式 App Shell 布局框架 | 极低 |
| `src/components/layout/index.ts` | 新增导出 | 布局组件统一导出 | 零风险 |
| `src/App.tsx` | 修改组件 | 将根 AppLayout 挂载升级为 AppShell | 极低 |
| `src/i18n/locales/zh-CN.json` | 修改 i18n | 补充 `navigation` 导航多语言字典 | 零风险 |
| `src/i18n/locales/en-US.json` | 修改 i18n | 补充 `navigation` 英文导航多语言字典 | 零风险 |

---

## B. Navigation Architecture (导航架构说明)

1. **Primary Navigation (一级主导航)**:
   - **OVERVIEW**: Dashboard (对应 `workbench`)
   - **RUNTIME**: Providers (`providers`), Proxy (`proxy`), Usage (`usage`)
   - **RESOURCES**: Accounts (`antigravity`), Workspace (`mcp` / `prompts` / `skills` 等聚合)
   - **SYSTEM**: Settings (`settings` 等子设置)
2. **Client Context (客户端上下文解耦)**:
   - Claude Code / Claude Desktop / Codex / OpenCode 四大客户端从原本散落的一级导航剥离，升级为置顶 ContextHeader 中的 **Client Context Switcher**。
   - 切换 Client 仅改变查看/配置的目标 App，不破坏任何后端 Client Isolation 隔离规则。
3. **Legacy Compatibility Layer (无缝兼容层)**:
   - 保持 `src/lib/pageRegistry.ts` 与 `activeKey` 状态作为根加载与组件挂载机制。
   - 没引入 `react-router-dom`，完全无缝兼容现有的 16 个 Legacy 页面。

---

## C. App Shell 实现情况

- **Sidebar (侧栏)**:
  - 默认展开宽度 220px，支持点击底部切为 60px 极窄图标模式。
  - 折叠模式下自动气泡提示 Tooltip，支持 Highlight 高亮与 Hover 微动效。
- **ContextHeader (顶栏)**:
  - 根据 activeKey 自动展示当前 Section 的主标题与副描述。
  - 在需 Target 切换的页面（Dashboard, Providers, Proxy, Usage, Workspace）自动嵌入 `ClientSwitcher`。
- **StatusBar (底栏)**:
  - 28px 固定底部状态栏，真实展示 Proxy 运行状态 (`Proxy :15821` / `Stopped`)、当前 Active Provider 名称与应用版本。
- **Layout & Scroll**:
  - 全屏 100vh 强约束，`overflow: hidden`，内部 Content 区域 `overflow: auto` 独立滚动，消除了双重滚动条。

---

## D. State Reuse (状态复用与 Single Source of Truth)

- **Active Page State**: 继续复用 `src/App.tsx` 中驱动的 `activeKey` & `handleNavigate`。
- **Client Context State**: 直接消费 `src/stores/pagePreferencesStore.ts` 中的 `workspaceTarget` & `setWorkspaceTarget`，一键联动 `providersTarget` 与 `proxyTarget`，保证全局单一可信源。
- **Theme & Dark Mode**: 完全兼容 Phase 1 的 `semantic.css` 与 `html[data-theme="dark"]`，侧栏、顶栏、状态栏在 Dark Mode 下对比度良好。

---

## E. Deferred Issues (延迟处理事项)

1. **部分 Legacy 页面内部标题重复**: 部分 Legacy 页面（如 `ProxyPage`, `UsagePage`）内部仍包含大字号 Header，在 Phase 3~9 逐页重构时将进行清理并统一收归 ContextHeader。
2. **Workspace 内子 Tab / Secondary Navigation**: Phase 2 建立了 Workspace 的聚合入口（默认导航至 MCP），Phase 8 将建立专属的二级导航与 Dashboard。

---

## F. Phase 3 Readiness (Phase 3 供应商页面重构准备情况)

1. **Providers 页面是否可以开始视觉重构？**:
   - **是**。App Shell 与 Client Switcher 已就位，Providers 页面可以从原先的顶栏中剥离，专注于 Compact Card / List 视图与右侧 Drawer 编辑器重构。
2. **Provider 页面当前重复区域**:
   - 页面顶部的 `WorkspaceTargetSegmented` 客户端选择器已在 ContextHeader 中提供，未来 Providers 页面内部可隐去重复的 Client Tab。
3. **Phase 3 建议修改的文件**:
   - `src/pages/ProvidersPage.tsx`
   - 新建 `src/components/domain/ProviderCard.tsx`
   - 新建 `src/components/domain/ProviderDrawer.tsx`

---

*Phase 2 交付时间: 2026-08-10*
