# AI-Switcher UI 重构 Audit 报告 (Phase 0)

> 本文档为 Phase 0 产物，用于记录系统当前的技术栈架构、页面与组件映射、数据流与 Tauri IPC 封装，并制定渐进式 UI 重构的迁移 Mapping。

---

## 1. 项目技术栈概览

- **桌面框架**: Tauri v2 (`@tauri-apps/api` v2.9.1, `@tauri-apps/plugin-*`)
- **视图层**: React 19 (`react` v19.2.3, `react-dom` v19.2.3)
- **UI 组件库**: Ant Design v6 (`antd` v6.4.3, `@ant-design/icons` v6.2.5)
- **图表渲染**: Recharts (`recharts` v3.10.1)
- **全局与 UI 状态管理**: Zustand (`zustand` v5.0.10)
- **服务端数据获取与缓存**: React Query (`@tanstack/react-query` v5.90.3)
- **国际化**: i18next (`i18next` v25.7.4, `react-i18next` v16.5.3)
- **构建工具**: Vite v7 (`vite` v7.3.1) + TypeScript 5.9

---

## 2. React 入口与全局 Provider

- **入口文件**: `src/main.tsx`
  - 挂载 `QueryClientProvider` (`queryClient`)
  - 引入 `antd/dist/antd.css` 和 `@/styles.css`
  - 初始化 side-effects: `i18n` 资源、`initializeProviderHealthEvents()`、`initializeProxyStatusEvents()`
- **根根组件**: `src/App.tsx`
  - 包裹 `ConfigProvider`（支持动态 CSS 变量、CSP Nonce、zh-CN/en-US 语言包、Antd Dark/Light Theme 算法与自定义 token）
  - 包裹 `AntApp`（提供全局 static message/notification/modal 实例）
  - 包含 `StartupScreen`（启动预热）与 `NavigationContext.Provider`
  - 渲染根 Shell `AppLayout` 并在内部挂载动态加载的 `ActivePage`

---

## 3. 路由架构诊断

- **现状**: 
  - `react-router-dom` 已在之前的优化中移除，目前采用简单高效的按需预加载注册表 `src/lib/pageRegistry.ts`。
  - 通过 `activeKey` (类型 `PageKey`) 控制当前页面切换。
  - 页面 loader 列表：`workbench`, `settings`, `providers`, `profiles`, `proxy`, `antigravity`, `mcp`, `prompts`, `skills`, `agents`, `codexPlugins`, `sessions`, `usage`, `localization`, `environment`, `about`。
- **重构诉求**:
  - 当前布局为 **Sidebar-free**（顶部无边框 TitleBar + 单一 Content 区 + 底部 Footer），导致部分导航集中在设置页和 Workbench 内部。
  - 新规划（Phase 2）将演进为 **Workspace Shell + 左侧窄 SideBar + 顶栏 ContextHeader + 上下文切换**。
  - `pageRegistry.ts` 机制良好，可继续复用作为按需加载基础，避免重新引入重型 Router。

---

## 4. 主题与样式系统

- **主题存储**: `src/stores/themeStore.ts` (基于 Zustand，支持 `light` | `dark` | `system`)
- **CSS Token 机制**:
  - `App.tsx` 中的 `themeConfig` 使用 `antdTheme.darkAlgorithm` / `defaultAlgorithm`。
  - 自定义 Semantic Tokens 注入在 ConfigProvider: `colorBgLayout`, `colorBgContainer`, `colorBorder`, `colorPrimary` (#007aff / #58a6ff) 等。
  - `styles.css` 包含基础全局重置与一小部分兼容样式。
- **重构规范 (Phase 1)**:
  - 统一建立全局 Semantic Design CSS Variables (`--bg-app`, `--bg-surface`, `--border-default`, `--accent` 等)，减少在组件中分散使用 HEX 硬编码。

---

## 5. Tauri IPC 与 Service 封装

前端禁止在 View/Page 组件中直接裸调用 `invoke()`，全部通过 `src/services/` 层集中进行类型安全封装：

- **基础 IPC 桥接**: `src/services/ipc.ts` 
  - `call<T>(cmd: string, args?: Record<string, unknown>): Promise<T>`（支持非 Tauri 浏览器预览模式下友好报错与降级）
- **领域 Service 拆分**:
  - `system.ts`: 环境变量、环境信息、进程管理、前端性能上报
  - `config.ts`: 基础配置读取与保存
  - `providers.ts`: 供应商增删改查、批量导入导出、测速、切换
  - `antigravity.ts`: Google OAuth 登录、账号池操作、额度刷新
  - `proxy.ts`: 本地代理启动/停止/重启、Failover 配置
  - `mcp.ts` / `prompts.ts` / `skills.ts` / `sessions.ts` / `usage.ts` / `tools.ts`

---

## 6. 页面与组件现状清单

### 6.1 页面清单 (`src/pages/`)
1. `WorkbenchPage.tsx`: 当前主首页（高频切换供应商、运行状态、热力图/柱状图、最近卡片）
2. `ProvidersPage.tsx`: 供应商配置管理列表与编辑
3. `ProxyPage.tsx`: 代理运行控制中心与故障切换配置
4. `UsagePage.tsx`: 用量统计、Token 分析、图表与请求记录
5. `AntigravityPage.tsx`: Antigravity / Google 账号与额度面板
6. `ProfilesPage.tsx`: 项目配置快照 Profiles 页面
7. `McpPage.tsx`: MCP Servers 管理
8. `PromptsPage.tsx`: Prompts 全局指令管理
9. `SkillsPage.tsx`: Skills 技能包管理
10. `AgentsPage.tsx`: Agents 自定义智能体配置
11. `CodexPluginsPage.tsx`: Codex 插件配置
12. `SessionsPage.tsx`: 会话历史管理
13. `SettingsPage.tsx`: 系统级通用设置
14. `AboutPage.tsx`: 关于与客户端安装检测（Claude/Codex/OpenCode）
15. `DesktopLocalizationPage.tsx`: 桌面中文化选项
16. `EnvironmentPage.tsx`: 运行环境详情

### 6.2 域名与通用组件 (`src/components/`)
- `AppLayout.tsx`: Shell 容器
- `TitleBar.tsx`: 自定义窗口标题栏
- `StartupScreen.tsx`: 启动预热界面
- `ProviderForm.tsx`: 供应商新增/编辑表单
- `ProviderBrandIcon.tsx`: 供应商品牌图标匹配与绘制
- `WorkspaceTargetSegmented.tsx`: 目标 Client 切换分段控件
- `FloatingViewSwitcher.tsx`: 悬浮页面切换器
- `AntigravityQuotaBars.tsx`: Antigravity 模型额度可视化条
- `UsageCalendar.tsx`: 365 天横向用量热力图
- `UsageMetric.tsx` / `UsageBreakdownCard.tsx`: 用量核心 KPI 卡片
- `UsageSourceIcons.tsx` / `UsageSourceFilterSegmented.tsx`: 来源分布与过滤
- `ImportPreviewDialog.tsx`: 配置文件导入预览弹窗
- `OnboardingTip.tsx`: 首次使用提示组件

---

## 7. 不可破坏的核心功能基线

1. **多客户端隔离与上下文绑定**: Claude Code / Claude Desktop / Codex / OpenCode 4 大目标客户端的配置同步逻辑（`claude_desktop_config.json`, `config.json`, `config.toml`+`auth.json`, `opencode.json`+`AGENTS.md`）。
2. **Codex 官方登录维持机制**: `apply_auth_strategy` 保持 Bearer Token 注入与备份回滚，不破坏官方登录态。
3. **Antigravity 自动轮换与额度定时刷新**: 45s 首刷 + 5min 循环刷新 `antigravity-quota-refreshed` 事件监听。
4. **Proxy 无缝热切换与 Failover**: 代理端口配置、自修复故障切换、重试状态码逻辑。
5. **用量日志与诊断防泄露**: 统计明细脱敏、请求体内容保护。

---

## 8. 渐进式重构迁移 Mapping (Phase 1 ~ Phase 9)

| 阶段 | 模块 / 目标 | 涉及主要文件 | 迁移策略 |
|---|---|---|---|
| **Phase 1** | Design Tokens & Primitive UI | `src/styles.css`, `src/components/ui/*` | 新建原子 UI 组件 (Button, Badge, Card, Drawer 等)，暂不破坏现有页面 |
| **Phase 2** | App Shell 重构 | `AppLayout.tsx`, `Sidebar.tsx`, `ContextHeader.tsx` | 实现左侧 Narrow Sidebar + 顶栏 ContextHeader，保持内嵌 Route 无缝过渡 |
| **Phase 3** | Providers 页重构 | `ProvidersPage.tsx`, `ProviderCard.tsx`, Drawer | 迁移为 Compact Card / List 混合视图 + 右侧 Drawer 编辑 |
| **Phase 4** | Proxy 控制中心 | `ProxyPage.tsx` | 升级 Hero Status + Runtime 详情区 + Failover 可视化链条 |
| **Phase 5** | Dashboard 概览页 | `DashboardPage.tsx` | 新增系统概览页（融合运行状态、当前供应商、24h 趋势与健康监控） |
| **Phase 6** | Usage 用量与诊断 | `UsagePage.tsx` | 重构 Filter Bar + 2x2 图表矩阵 + 近期请求日志抽屉 |
| **Phase 7** | Accounts (Antigravity) | `AntigravityPage.tsx` | 重构卡片式账号列表 + 额度进度条 + 交互抽屉 |
| **Phase 8** | Workspace 资源整合 | `WorkspacePage.tsx` | 将 Projects/MCP/Prompts/Skills/Agents/Plugins 归口进入 Workspace 导航 |
| **Phase 9** | Settings 精简与清理 | `SettingsPage.tsx` | 清理已被 Workspace 领走的页面，只保留全局应用配置 |

---

*Phase 0 完成时间: 2026-08-10*
