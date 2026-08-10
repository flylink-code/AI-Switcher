# Phase 7 交付报告 — Accounts / Antigravity 账号池与额度体验重构

> 本文档记录 Phase 7 的执行结果。系统已将 Accounts / Antigravity 页面升级为可信、紧凑、高效的 Account Pool Control Center。

---

## A. Changed Files (修改与新增文件清单)

| 文件路径 | 类型 | 作用 / 目的 | 风险层级 |
|---|---|---|---|
| `docs/ui-refactor/phase-7-preflight.md` | 新增文档 | Phase 7 预检、架构映射与 Timer / Polling 审计日志 | 零风险 |
| `docs/ui-refactor/phase-7-result.md` | 新增文档 | Phase 7 交付报告与 Readiness | 零风险 |
| `src/components/antigravity/AccountPoolOverview.tsx` | 新增组件 | 账号池整体指标概览卡片 (Active/Available/Rotation/Gateway) | 零风险 |
| `src/components/antigravity/AccountCard.tsx` | 新增组件 | 单账号 Presentation 卡片 (Identity, Status, Quota, Health, Actions) | 零风险 |
| `src/components/antigravity/GatewayCard.tsx` | 新增组件 | 本地 API 网关、端口、密钥、出站代理与 Curl 命令行引导 | 零风险 |
| `src/components/antigravity/BindAppsCard.tsx` | 新增组件 | 一键构建/更新 Client Target 内建 Antigravity 供应商卡片 | 零风险 |
| `src/components/antigravity/ImportAccountsModal.tsx` | 新增组件 | JSON 凭据弹窗式安全导入组件 | 零风险 |
| `src/components/antigravity/index.ts` | 新增导出 | 域名组件 Barrel 导出文件 | 零风险 |
| `src/pages/AntigravityPage.tsx` | 重构页面 | Accounts 页面 Presentation 结构与交互重构 | 极低 |
| `src/i18n/locales/zh-CN.json` | 扩展 | 补充账号池概览、轮换与删除确认文案 | 零风险 |
| `src/i18n/locales/en-US.json` | 扩展 | 补充账号池概览、轮换与删除确认英文文案 | 零风险 |

---

## B. Account Architecture (架构说明)

- **`AntigravityPage`**: 控制中心与调度页。接入 `ContextHeader` 统一顶部管理与快速操作（[刷新额度] [JSON 导入] [用浏览器登录 Google]）。
- **`AccountPoolOverview`**: 呈现池状态（Active 账号、可用/总账号数、自动轮换策略状态、网关 Running 状态与 Address）。
- **`AccountCard` Grid**: 响应式呈现各个账号凭据与 4 维额度进度条（Gemini 5h/7d, Claude 5h/7d），包含安全删除 Popconfirm 与设为 Active 逻辑。
- **`GatewayCard` & `BindAppsCard`**: 管理反代出站代理模式 (SOCKS5/System/Direct)、网关启停与一键应用内建供应商绑定。
- **`ImportAccountsModal`**: 将容易混乱的 TextArea 转换为干净的 Modal 弹窗凭据导入。

---

## C. Runtime & Polling Protection (运行时与轮询保护证明)

| 审计项目 | 原设计 / 约束 | 重构后结果 | 判定 |
|---|---|---|---|
| **OAuth Protocol** | 原有 `startAntigravityOauthLogin` / `login_with_browser` 8085 回调 | 零篡改，完全保留 | 100% 保持 |
| **Token / Secret Visibility** | 不向前端 UI/Console 暴露 Access/Refresh Token | 零暴露，仅在 Modal/Card 展示脱敏 Email | 100% 保持 |
| **Rotation Strategy** | 后端 `AccountPool` 智能健康度与故障转移算法 | 零修改，只作为只读状态在 UI 呈现 | 100% 保持 |
| **45s Initial Refresh** | 后端 `spawn_antigravity_quota_refresh` (45s sleep) | 零修改，保持后端生命周期 | 100% 保持 |
| **5min Event Polling** | 后端 `QUOTA_REFRESH_INTERVAL_SECS` (300s) + `antigravity-quota-refreshed` 事件 | 零修改，前端通过 `listen` 自动刷新缓存 | 100% 保持 |
| **Duplicate Polling** | `Duplicate quota polling added by Phase 7: 0` | `0` 新增 Timer / Polling | 100% 保持 |
| **Dashboard Consistency** | Dashboard (WorkbenchPage) 仅监听 Gateway Status 5s 轮询 | 数据源语义完全一致 | 100% 一致 |

---

## D. Phase 8 Workspace / Resources Readiness (Phase 8 预检审计)

针对下一阶段 (Phase 8 Workspace / Resources 资源与工作区管理) 的只读审计与回答：

1. **当前 Workspace / Resources 页面文件**:
   - `src/pages/ProfilesPage.tsx` (项目/工作区 Profile 管理)
   - `src/pages/McpPage.tsx` (MCP 服务器管理)
   - `src/pages/PromptsPage.tsx` (Prompts 模版管理)
   - `src/pages/SkillsPage.tsx` (Skills 技能工具管理)
   - `src/pages/AgentsPage.tsx` (Agents 代理管理)
   - `src/pages/CodexPluginsPage.tsx` (Codex 插件管理)
   - `src/pages/SettingsPage.tsx` (全局配置与环境)
2. **MCP 是否属于 Workspace / Resources 范围**: 是，MCP 服务器是 Workspace Resources 的核心组成部分，与 Profiles、Prompts、Skills 同属资源层。
3. **Environment / Config / Skills / MCP 等资源组织方式**:
   - MCP Servers: 后端 SQLite 存储 (`mcp_servers` 表)，同步到各个客户端配置文件 (`claude_desktop_config.json`, `opencode.json`, `codex.toml`)。
   - Prompts: 后端本地文件存储 (`prompts/` 目录与全局 AGENTS.md)，支持多模式导入导出。
   - Skills / Agents: 本地 filesystem 扫描 + GitHub 仓库索引与 ZIP 解压。
   - Profiles: 后端 Profiles 管理 (`workspace_profiles` 表)。
4. **每类资源的数据源**:
   - MCP: `listMcpServers`, `saveMcpServer`, `deleteMcpServer`
   - Prompts: `listPrompts`, `readPrompt`, `savePrompt`, `activatePrompt`
   - Skills: `listSkills`, `getSkillRepository`
   - Profiles: `listProfiles`, `createWorkspaceProfile`, `applyProfile`
5. **哪些资源来自文件系统**: Prompts（包含本地 AGENTS.md / markdown）、Skills（`.claude/skills` / `.codex/skills`）、配置文件（PathsInfo）。
6. **哪些资源来自 Tauri IPC**: 全部 CRUD、激活、启用/禁用及冲突校验。
7. **哪些资源支持 Add / Edit / Delete / Enable / Disable**: MCP Servers、Prompts、Skills、Agents、Profiles 均完整支持这五类基本操作。
8. **是否存在文件 watcher / polling / event listener**:
   - Codex session usage watcher (30s)
   - Claude Code session usage watcher (30s)
   - OpenCode session usage watcher (30s)
   - MCP / Skills 不带粗暴轮询，依赖用户操作与唤醒机制。
9. **Workspace 是否按 Client Context 隔离**: 是，支持 `claude_code`, `claude_desktop`, `codex`, `opencode` 四大 Target 分立或跨 Client 同步。
10. **当前 Resource Detail / Editor 如何实现**: Modal 弹窗编辑（如 Prompt 编辑器、MCP 导入导出弹窗）或 Drawer 面板。
11. **是否存在直接编辑用户配置文件的逻辑**: 后端 `config/` 模块安全读写 `opencode.json`, `claude_desktop_config.json`, `codex.toml`；前端不直接读写裸文件，严格走 Tauri 命令。
12. **哪些文件属于高风险业务边界**: `src-tauri/src/config/` (atomic 写入、MoveFileExW、Profile 镜像转换) 及 `src-tauri/src/commands/mcp.rs`。
13. **哪些 Workspace UI 可以安全重构**:
   - `src/pages/ProfilesPage.tsx`
   - `src/pages/McpPage.tsx`
   - `src/pages/PromptsPage.tsx`
   - `src/pages/SkillsPage.tsx`
   - `src/pages/AgentsPage.tsx`
   - 抽出统一的 `ResourceToolbar` 与 `ResourceCard` UI Primitives。
14. **哪些资源操作必须完全保留**: 所有 CRUD IPC、同步命令 (sync_all, activate_prompt)、配置文件应用 (apply_profile) 与版本迁移保护。
15. **Phase 8 推荐修改哪些文件**:
   - `src/pages/ProfilesPage.tsx`
   - `src/pages/McpPage.tsx`
   - `src/pages/PromptsPage.tsx`
   - `src/pages/SkillsPage.tsx`
   - `src/components/resources/` (新增 Workspace / Resource 通用 Domain Components)
16. **是否存在可以复用 Phase 3 Provider / Phase 7 Account 的列表与状态模式**:
   - 完全存在！可以复用 Phase 1 Primitive (`Surface`, `Metric`, `StatusBadge`), Phase 3 `ProviderCard` 栅格布局模式以及 Phase 7 `AccountCard` 的状态与简捷 Action 规范。

---

*Phase 7 交付时间: 2026-08-10*
