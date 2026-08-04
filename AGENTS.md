# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区为 `1.0.5`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v1.0.5）

- 修复：会话管理恢复 Claude Code / Codex 切换（不再绑死 workspace，默认 Code 时扫不到 Codex）
- 总览：增加按供应商 / 按模型统计
- 用量：多币种预估成本按近似汇率换算 USD 后相加
- Schema `user_version` = 20（沿用）
- 规划 / 问题笔记：`task.md`、`bug.md`（gitignore）
- 不要将 `release/` 调试文件或编译缓存纳入提交

## 已验证结果（1.0.5）

- 前端 `tsc --noEmit`：通过
- FX / 多币种汇总相关 Rust 单测：通过
- 本机 `~\.codex\sessions` 约 74 个 rollout；切到 Codex 后应可列出

## 后续操作建议

1. 推送 `main` 与带注释的 `v1.0.5` 标签，由 GitHub Actions 云端构建发布。
2. Windows 用户优先 NSIS；会话管理页验证 Codex 切换可见列表。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
