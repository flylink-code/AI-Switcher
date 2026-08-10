# Phase 2 Preflight 预检日志

## 1. Legacy Page Key 与 Primary Navigation 映射表

| Legacy PageKey | 组件名 | 当前导航名称 | 建议 Primary 归属分类 | Client 上下文敏感? | Workspace 资源? |
|---|---|---|---|---|---|
| `workbench` | `WorkbenchPage` | 工作台 (Home) | **Dashboard** (OVERVIEW) | 是 | 否 |
| `providers` | `ProvidersPage` | 供应商服务 | **Providers** (RUNTIME) | 是 | 否 |
| `proxy` | `ProxyPage` | 运行状态/代理 | **Proxy** (RUNTIME) | 否 (运行全局/多客户端) | 否 |
| `usage` | `UsagePage` | 用量与统计 | **Usage** (RUNTIME) | 是 (支持 Filter) | 否 |
| `antigravity` | `AntigravityPage` | Antigravity 账号 | **Accounts** (RESOURCES) | 否 | 否 |
| `profiles` | `ProfilesPage` | 快照配置 | **Workspace** (RESOURCES) | 是 | 是 |
| `mcp` | `McpPage` | MCP 服务器 | **Workspace** (RESOURCES) | 是 | 是 |
| `prompts` | `PromptsPage` | Prompts 指令 | **Workspace** (RESOURCES) | 是 | 是 |
| `skills` | `SkillsPage` | Skills 技能包 | **Workspace** (RESOURCES) | 是 | 是 |
| `agents` | `AgentsPage` | Agents 智能体 | **Workspace** (RESOURCES) | 是 | 是 |
| `codexPlugins` | `CodexPluginsPage` | Codex 插件 | **Workspace** (RESOURCES) | 仅 Codex | 是 |
| `sessions` | `SessionsPage` | 会话管理 | **Workspace** (RESOURCES) | 是 | 是 |
| `settings` | `SettingsPage` | 设置 | **Settings** (SYSTEM) | 否 | 否 |
| `about` | `AboutPage` | 关于 | **Settings** (SYSTEM) | 否 | 否 |
| `environment` | `EnvironmentPage` | 运行环境 | **Settings** (SYSTEM) | 否 | 否 |
| `localization` | `DesktopLocalizationPage` | 桌面中文化 | **Settings** (SYSTEM) | 否 | 否 |

---

## 2. 状态源映射 (State Sources)

- **Active Page Source**: `src/App.tsx` 中的 `activeKey` 状态与 `NavigationContext`。
- **Client Context Source**: `src/stores/pagePreferencesStore.ts` 中的 `workspaceTarget` 以及 `setWorkspaceTarget`。
- **Sidebar Collapsed State**: 将在 `src/stores/uiStore.ts` 或 `localStorage` 维护。
- **Proxy Runtime Source**: `src/lib/proxyStatusEvents.ts` / `src/services/proxy.ts`。
- **Theme Source**: `src/stores/themeStore.ts` (`data-theme="dark"` / `light`)。

---

## 3. 风险评估 (Risk Map)

- **高风险项**: 一度将 16 个 Legacy activeKey 替换为新的路由库（**已明确禁用**，只用兼容层适配映射）。
- **中风险项**: 双重 Header/Title 冲突（需要当 AppShell ContextHeader 存在时控制 Legacy 页面的内嵌标题）。
- **低风险项**: Sidebar 折叠模式下图标的语义与 Tooltip 提示。

---
*预检完成，下一步：构建 App Shell 与 Navigation Components。*
