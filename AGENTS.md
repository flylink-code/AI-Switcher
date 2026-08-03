# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.7.5`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.7.5）

- v0.7.5：无新版本时检查更新提示「暂无更新」；代理上游 `reqwest` 加 `.no_proxy()` 避免环境代理环路。
- 推送 `main` 与带注释的 `v0.7.5` 标签后由 GitHub Actions 云端构建发布。
- 不要将 `release/` 调试文件或编译缓存纳入提交；`task.md` / `bug.md` 为本地规划（gitignore）。

## 已验证结果

- `commands::app_update::tests`：通过。

## 后续操作建议

1. 关注 Actions Release。
2. Windows 用户优先 NSIS；Linux 优先 AppImage。
3. Ubuntu SDK `check-network.sh` 问题待用户验证（环境缺 curl + ping 兜底）。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
