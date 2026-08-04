# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.8.10`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.8.10）

- 修复：Codex 插件「刷新市场」在 GUI 无 npm PATH 时误报 `program not found`（复用 doctor 同款 CLI 探测）
- Schema `user_version` = 20
- 规划 / 问题笔记：`task.md`、`bug.md`（gitignore）
- 不要将 `release/` 调试文件或编译缓存纳入提交

## 已验证结果（0.8.10）

- Codex CLI resolve / `codex --version` 启动路径单测：通过
- plugins list/toggle 单测：通过

## 后续操作建议

1. 推送 `main` 与带注释的 `v0.8.10` 标签，由 GitHub Actions 云端构建发布。
2. Windows 用户优先 NSIS。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
