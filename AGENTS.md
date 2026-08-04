# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.8.9`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.8.9）

- P2：Skills Discovery；供应商 Base URL RTT 测速；全局 `web_search`；Codex marketplace list/add/remove + 插件卸载
- P3：README 热切换/Deep Link/`web_search` 说明；doctor `spinnerVerbs` / model-catalog 单条修复
- Schema `user_version` = 20
- 规划 / 问题笔记：`task.md`、`bug.md`（gitignore）
- 不要将 `release/` 调试文件或编译缓存纳入提交

## 已验证结果（0.8.9）

- Skills discovery / web_search / health latency / plugins 相关单测：通过
- 前端 `tsc --noEmit`：通过

## 后续操作建议

1. 推送 `main` 与带注释的 `v0.8.9` 标签，由 GitHub Actions 云端构建发布。
2. Windows 用户优先 NSIS。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
