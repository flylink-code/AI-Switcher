# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.5.8`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.5.8）

- Ubuntu/国内网络：fnm 经 npmmirror 安装 Node；npm 全局 Claude/Codex 走 `registry.npmmirror.com`；有 Node≥22 时 Claude 优先 npm。
- 推送 `main` 与带注释的 `v0.5.8` 标签后由 GitHub Actions 云端构建发布。
- 不要将 `release/` 调试文件或编译缓存纳入提交。

## 已验证结果

- `cargo test`（node_runtime / tools）：通过。
- `tsc --noEmit`：通过。

## 后续操作建议

1. 推送标签后关注 Actions Release 工作流与 GitHub Release 产物。
2. Windows 用户优先 NSIS；Linux 优先 AppImage，避免 MSI/deb 的管理员提示。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
