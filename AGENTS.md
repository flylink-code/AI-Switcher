# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / **AI-Switcher**，Windows 桌面端，统一配置 Claude Desktop、Claude Code、Codex 及相关工具（供应商、本地代理、MCP、会话、用量等）。
- 仓库：`flylink-code/AI-Switcher`；发版靠打注释标签触发 GitHub Actions（Windows 优先 NSIS）。
- 技术栈：Tauri 2（Rust）+ React + TypeScript + Vite；包管理用 Corepack 读取 `packageManager` 锁定的 **pnpm**。
- 当前版本：**1.0.9**（`package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml` 已对齐）。
- Schema：`user_version` = **20**（近期改动以前端壳层 / 用量展示 / 会话 UX 为主，未升 schema）。
- 本地笔记：`task.md`、`bug.md`（gitignore，不提交）。不要把 `release/`、`src-tauri/target/` 等编译产物纳入提交。

## 目录与职责（快速导航）

| 区域 | 路径 | 说明 |
|------|------|------|
| 前端页 | `src/pages/` | Overview / Providers / Sessions / Usage / Proxy 等 |
| 壳层 | `src/components/AppLayout.tsx`、`src/stores/pagePreferencesStore.ts` | 侧栏分组；顶栏显示 Code/Desktop/Codex 进程运行状态；目标切换在供应商/代理页 |
| 供应商表单 | `src/components/ProviderForm.tsx`、`src/lib/providerPresets.ts` | 模型角色映射（易出竞态） |
| 代理 | `src-tauri/src/proxy/`、`src-tauri/src/commands/proxy.rs` | 本地代理生命周期、更新后恢复 |
| Codex 配置 | `src-tauri/src/config/codex.rs`、`codex_provider_sync.rs` | 直连/代理写入、`model_provider` 历史同步 |
| 会话扫描 | `src-tauri/src/session_manager.rs`、`src/pages/SessionsPage.tsx` | Code / Codex 本地会话列表 |
| 用量 | `src-tauri/src/commands/usage.rs`、`src-tauri/src/usage/fx.rs`、`proxy_logs.rs` | 仪表盘、多币种→USD、去重 |

## 近期已发版要点（1.0.2 → 1.0.9）

### 1.0.9（最新）

- **更新后切换 IO 183**：Windows `atomic_write` 改用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`，避免 delete+rename TOCTOU；Codex catalog 纳入切换快照回滚。
- **会话空列表**：Codex 空扫后端/前端重试加长；目录存在但扫不到时 degraded 提示更明确。
- **顶栏状态**：改为 Code / Desktop / Codex 进程运行状态（绿点=运行中），点击进环境页。

### 1.0.8

- **DeepSeek / Codex catalog**：预设补齐 flash/pro failover；Codex Responses 预设；catalog 友好 display_name，第三方不再注入 GPT 建议。
- **更新后切换 IO 183（首轮）**：`atomic_write` 进程锁 + Windows rename 重试（1.0.9 才换成 MoveFileEx）。
- **UI**：Code/Desktop/Codex 切换移到供应商/代理页；壳层玻璃顶栏与供应商工具栏风格优化。

### 1.0.3 / 1.0.4（相关但勿当成「会话为空」根因）

- **1.0.3**：`ProviderForm` 默认模型同步竞态会把角色映射全改成默认；Responses 用量把 total `input_tokens` 当 fresh → proxy↔session 去重失败双重计数；表单映射两列布局。
- **1.0.4**：更新后 `recover_runtime_after_relaunch` 增加 `sync_to_managed_provider` 重试 + `runtime-recovered` 事件 + 空列表再扫。这些对「更新瞬间锁」有帮助，**不能**解释「会话管理里一直看不到 Codex」——那是 UX 绑死 workspace。

### 更早（仍有效）

- **0.8.12**：用量「全部」主显示勿盲目偏 USD；Codex OpenAI 兼容走直连上游 + 多模型 catalog。
- **1.0.2**：更新退出优雅停代理 / WAL / 端口重绑 / 启动后二次恢复。
- **1.0.1**：Desktop 中文包跟踪 `javaht/claude-desktop-zh-cn` GitHub Releases latest。

## 易踩坑（给下一任 AI）

1. **会话管理空列表**：先看 `SessionsPage` 当前 `sessionsProvider` 是不是 `codex`，再查扫描路径。不要一上来只修更新后代理恢复。
2. **模型映射「全变默认」**：看 `ProviderForm` 默认模型 sync effect；用 `syncMappingOnDefaultChange`（只填空/旧默认），打开编辑时预置 `prevModelRef`。
3. **用量钱不对**：区分「多币种 headline」「Responses cache 双重计数」「定价表模糊匹配」。FX 是展示近似汇率，不是实时牌价。
4. **Codex 对话失败 vs 会话列表空**：前者多是 `config.toml` / catalog / `requires_local_proxy`；后者是 Sessions 页扫描与 UI 来源切换。
5. **workspaceTarget**：顶栏不再切换目标；供应商/代理页用 `WorkspaceTargetSegmented`；会话页可独立切 Code/Codex（`setSessionsProvider`）。
6. **IO 183**：Windows 配置写必须用 `MoveFileExW(REPLACE_EXISTING)`，不要 delete-then-rename。

## 构建与验证

- 前端：`corepack pnpm exec tsc --noEmit`
- 后端：`cargo test --manifest-path src-tauri/Cargo.toml --lib <filter>`
- 本地编译脚本：`scripts/`（见仓库说明）
- 发版：bump 三处 version → 提交 → 注释标签 `vX.Y.Z` → `git push` + `git push origin vX.Y.Z` → Actions 出安装包；可用 `gh release edit` 写说明。Windows 用户优先 NSIS。

## 建议下一任优先确认

1. 用户装 **1.0.9** 后：更新后立刻切 Codex 供应商是否还报 IO 183。
2. 会话管理切到 Codex 是否能列出本地会话（磁盘有 `~\.codex\sessions` 时）。
3. 顶栏 Code/Desktop/Codex 绿点是否与实际进程一致。

# 注释
本项目允许本地编译测试；包管理器请用 Corepack + 锁定 pnpm。
