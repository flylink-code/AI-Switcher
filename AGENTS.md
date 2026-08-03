# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.8.5`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.8.5）

- v0.8.5：修复 Codex/Claude Code「会话·用量·插件消失」可见性；Claude Code `~/.claude/projects` JSONL 用量同步（Anthropic 兼容第三方直连可回填）；插件错误/空态诊断；会话页签持久化与 SQLite rollout 兜底；doctor 可见性检查与一键修复（不强制改直连 `ANTHROPIC_BASE_URL`）。
- v0.8.4：Codex Plugins 启停；供应商 Web Search（catalog）；环境 doctor；Prompt 重命名；MCP registry；Opus/Codex fast 成本。
- 规划：`task.md`（gitignore）；问题笔记：`bug.md`（gitignore）。
- 不要将 `release/` 调试文件或编译缓存纳入提交。

## 已验证结果（0.8.5）

- `list_snapshot_reads_quoted_plugin_table_keys`、`syncs_anthropic_usage_from_assistant_jsonl`、`onboarding_tip_keys_are_allowlisted`：通过
- 前端 `tsc --noEmit`：通过

## 后续操作建议

1. 推送 `main` 与带注释的 `v0.8.5` 标签，由 GitHub Actions 云端构建发布。
2. Windows 用户优先 NSIS；Linux 优先 AppImage。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
