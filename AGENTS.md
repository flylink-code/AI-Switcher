# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.7.6`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.7.6）

- v0.7.6：Ubuntu/Linux GUI 探测 Codex 时注入 fnm Node PATH，修复「已安装但无法运行」/`env: node` 缺失；收紧前端 Node 缺失误报。
- 推送 `main` 与带注释的 `v0.7.6` 标签后由 GitHub Actions 云端构建发布。
- 不要将 `release/` 调试文件或编译缓存纳入提交；`task.md` / `bug.md` 为本地规划（gitignore）。

## 已验证结果

- `commands::tools::tests`：通过。

## 后续操作建议

1. 关注 Actions Release。
2. Windows 用户优先 NSIS；Linux 优先 AppImage。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
