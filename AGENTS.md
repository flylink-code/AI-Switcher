# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.6.2`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.6.2）

- v0.6 六项已收口：回滚/延迟、MCP 结构化排序、autoReview、Profiles+托盘、Codex Anthropic 桥。
- 推送 `main` 与带注释的 `v0.6.2` 标签后由 GitHub Actions 云端构建发布。
- 不要将 `release/` 调试文件或编译缓存纳入提交。

## 已验证结果

- `cargo check` / profiles / codex_anthropic 相关测试：通过。
- `tsc --noEmit`：通过。

## 后续操作建议

1. 关注 Actions Release；下一步可评估 v0.7 或同步维护项。
2. Windows 用户优先 NSIS；Linux 优先 AppImage。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
