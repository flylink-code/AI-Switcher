# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.8.6`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.8.6）

- v0.8.6：Codex 用量同步锁/路径规范化与会话 SQLite 兜底；用量窄时间窗提示；VS Code/Cursor 汉化助手 CLI 探测（InstallLocation + PATH）与空格路径 `cmd` 引用修复。
- v0.8.5：修复 Codex/Claude Code「会话·用量·插件消失」可见性；Claude Code `~/.claude/projects` JSONL 用量同步；插件错误/空态诊断；会话页签持久化；doctor 可见性修复。
- 规划：`task.md`（gitignore）；问题笔记：`bug.md`（gitignore）。
- 不要将 `release/` 调试文件或编译缓存纳入提交。

## 已验证结果（0.8.6）

- localization CLI 解析 / `formats_cmd_script_line`、Codex session sync、plugins、`onboarding_tip_keys_are_allowlisted`：通过
- 前端 `tsc --noEmit`：通过

## 后续操作建议

1. 推送 `main` 与带注释的 `v0.8.6` 标签，由 GitHub Actions 云端构建发布。
2. Windows 用户优先 NSIS；Linux 优先 AppImage。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
