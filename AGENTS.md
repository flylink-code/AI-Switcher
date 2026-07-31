# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.5.4`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.5.4 发布中）

- 资料库 ZIP 导入（校验后替换托管资料，需重启；不含 API Key）。
- 用量统计支持今天 / 最近 24 小时及原有天数选项。
- 推送 `main` 与带注释的 `v0.5.4` 标签后由 GitHub Actions 云端构建发布。
- 不要将 `release/` 调试文件或编译缓存纳入提交。

## 已验证结果

- `cargo test --lib library_archive`：通过。
- `tsc --noEmit`：通过。

## 后续操作建议

1. 推送标签后关注 Actions Release 工作流与 GitHub Release 产物。
2. 不要把用户本地的 `release/` 调试文件或编译缓存纳入提交。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
