# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区为 `1.0.4`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v1.0.4）

- 修复：更新后 Codex 会话仍空——恢复流程重试 `sync_to_managed_provider`，emit `runtime-recovered`，会话页空列表自动再扫
- 沿用 1.0.3：模型映射竞态、Responses 用量去重、供应商表单两列布局
- Schema `user_version` = 20（沿用）
- 规划 / 问题笔记：`task.md`、`bug.md`（gitignore）
- 不要将 `release/` 调试文件或编译缓存纳入提交

## 已验证结果（1.0.4）

- 前端 `tsc --noEmit`：通过
- `proxy` 相关 Rust 单测：通过

## 后续操作建议

1. 推送 `main` 与带注释的 `v1.0.4` 标签，由 GitHub Actions 云端构建发布。
2. Windows 用户优先 NSIS；验证「更新后不用再手动重启，Codex 会话即出现」。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
