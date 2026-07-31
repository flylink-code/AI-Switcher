# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.5.5`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.5.5）

- 配置备份 / 资料库导出导入支持选目录；可选同步 API Key；用量今天/24h 按小时；环境路径弹窗；About Codex CLI；Ubuntu Claude Code 安装与所有权误判修复。
- 推送 `main` 与带注释的 `v0.5.5` 标签后由 GitHub Actions 云端构建发布。
- 不要将 `release/` 调试文件或编译缓存纳入提交。

## 已验证结果

- `cargo check --lib` / 相关单元测试：通过。
- `tsc --noEmit`：通过。

## 后续操作建议

1. 推送标签后关注 Actions Release 工作流与 GitHub Release 产物。
2. 不要把用户本地的 `release/` 调试文件或编译缓存纳入提交。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
