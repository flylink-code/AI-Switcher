# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.8.0`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.8.0）

- v0.8.0：Claude Code Agents 管理；MCP 远程 HTTP/SSE 与 OAuth 状态/清理；Desktop Connectors/`.mcpb` 冲突提示；用量统计支持 CNY 等成本汇总。
- 规划见 `task.md`（0.8 已落地；随后项待定）；`task.md` / `bug.md` 为本地规划（gitignore）。
- 不要将 `release/` 调试文件或编译缓存纳入提交。

## 已验证结果

- `mcp_oauth` / `agents` / `mcp` / `onboarding_tip_keys_are_allowlisted` / `apply_codex_estimated_cost_*`：通过
- 前端 `tsc --noEmit`：通过

## 后续操作建议

1. 推送 `main` 与带注释的 `v0.8.0` 标签，由 GitHub Actions 云端构建发布。
2. Windows 用户优先 NSIS；Linux 优先 AppImage。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
