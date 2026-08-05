# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / **AI-Switcher**，Windows 桌面端，统一配置 Claude Desktop、Claude Code、Codex 及相关工具（供应商、本地代理、MCP、会话、用量等）。
- 仓库：`flylink-code/AI-Switcher`；发版靠打注释标签触发 GitHub Actions（Windows 优先 NSIS）。
- 技术栈：Tauri 2（Rust）+ React + TypeScript + Vite；包管理用 Corepack 读取 `packageManager` 锁定的 **pnpm**。
- 当前版本：**1.0.8**（`package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml` 已对齐）。
- Schema：`user_version` = **20**（近期改动以前端壳层 / 用量展示 / 会话 UX 为主，未升 schema）。
- 本地笔记：`task.md`、`bug.md`（gitignore，不提交）。不要把 `release/`、`src-tauri/target/` 等编译产物纳入提交。

## 目录与职责（快速导航）

| 区域 | 路径 | 说明 |
|------|------|------|
| 前端页 | `src/pages/` | Overview / Providers / Sessions / Usage / Proxy 等 |
| 壳层 | `src/components/AppLayout.tsx`、`src/stores/pagePreferencesStore.ts` | 侧栏分组；顶栏只显示代理/当前供应商状态；目标切换在供应商/代理页 |
| 供应商表单 | `src/components/ProviderForm.tsx`、`src/lib/providerPresets.ts` | 模型角色映射（易出竞态） |
| 代理 | `src-tauri/src/proxy/`、`src-tauri/src/commands/proxy.rs` | 本地代理生命周期、更新后恢复 |
| Codex 配置 | `src-tauri/src/config/codex.rs`、`codex_provider_sync.rs` | 直连/代理写入、`model_provider` 历史同步 |
| 会话扫描 | `src-tauri/src/session_manager.rs`、`src/pages/SessionsPage.tsx` | Code / Codex 本地会话列表 |
| 用量 | `src-tauri/src/commands/usage.rs`、`src-tauri/src/usage/fx.rs`、`proxy_logs.rs` | 仪表盘、多币种→USD、去重 |

## 近期已发版要点（1.0.2 → 1.0.8）

### 1.0.8（最新）

- **DeepSeek / Codex catalog**：预设补齐 flash/pro failover；Codex Responses 预设；catalog 友好 display_name，第三方不再注入 GPT 建议。
- **更新后切换 IO 183**：`atomic_write` 进程锁 + Windows rename 重试。
- **UI**：Code/Desktop/Codex 切换移到供应商/代理页；顶栏只保留状态；壳层玻璃顶栏与供应商工具栏风格优化。
- **用量**：日志写入后前端可即时刷新（`usage-log-recorded`）。

### 1.0.5 / 1.0.6 / 1.0.7

- **会话管理 Codex 为空（真因）**：v0.9.0 去掉会话页 Code/Codex 切换后，列表绑死顶栏 `workspaceTarget`；默认 Claude Code 时根本不会 `scanSessions("codex")`。磁盘上 `~\.codex\sessions` 会话是存在的。已恢复 Segmented + 切换来源时目录筛选重置为「全部」。
- **总览**：增加按供应商 / 按模型统计（`UsageBreakdownCard`）。
- **用量多币种**：多种币种并存时按近似汇率换算成 **USD** 再相加（`usage/fx.rs`）；单币种仍显示原币种；`estimated_costs_by_currency` 保留原币明细。
- **1.0.6 / 1.0.7**：更新后 Codex 会话/配置锁重试相关修复。

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
5. **目标切换**：Code/Desktop/Codex 切换在**供应商页**（及代理页独立切换）；不要绑死会话页。会话用 `sessionsProvider`，顶栏只显示状态不切换目标。

## 构建与验证

- 前端：`corepack pnpm exec tsc --noEmit`
- 后端：`cargo test --manifest-path src-tauri/Cargo.toml --lib <filter>`
- 本地编译脚本：`scripts/`（见仓库说明）
- 发版：bump 三处 version → 提交 → 注释标签 `vX.Y.Z` → `git push` + `git push origin vX.Y.Z` → Actions 出安装包；可用 `gh release edit` 写说明。Windows 用户优先 NSIS。

## 建议下一任优先确认

1. 用户装 **1.0.8** 后：供应商页切换目标是否正常；更新后立刻切换供应商是否仍报 IO 183。
2. DeepSeek/Kimi Codex catalog 显示名是否正确（需重新应用供应商并重启 Codex）。
3. 总览多币种合计与分项是否符合预期（CNY≈7.25/USD 等近似值）。

# 注释
本项目允许本地编译测试；包管理器请用 Corepack + 锁定 pnpm。
