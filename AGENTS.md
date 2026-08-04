# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.8.7`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.8.7）

- v0.8.7：Codex 会话详情改为解析 `response_item` JSONL（不再误用 Claude Code 消息格式）；路径/`cwd` 去掉 `\\?\` 前缀。
- v0.8.6：Codex 用量同步锁/路径规范化与会话 SQLite 兜底；VS Code/Cursor 汉化 CLI 探测与空格路径修复。
- 规划：`task.md`（gitignore）；问题笔记：`bug.md`（gitignore）。
- 不要将 `release/` 调试文件或编译缓存纳入提交。

## 已验证结果（0.8.7）

- `loads_codex_response_item_messages`、`normalize_path_key_strips_windows_extended_prefix`：通过

## 后续操作建议

1. 推送 `main` 与带注释的 `v0.8.7` 标签，由 GitHub Actions 云端构建发布。
2. Windows 用户优先 NSIS；Linux 优先 AppImage。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
