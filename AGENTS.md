# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.5.1`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 已完成的主要能力

- 修复更新、关闭提示与托盘双击恢复行为。
- 统一调试版输出目录：`release/ClaudeSwitch-debug.exe`。
- Skills 仓库配置与多选安装，默认仓库为 `https://github.com/anthropics/skills`。
- MCP 官方 Registry 的浏览、搜索及安全添加（HTTP/SSE 与不含环境变量的 npm stdio）。
- “中文化配置”统一页面（Desktop / Claude Code / IDE 辅助扩展）。
- Codex 兼容与代理正确性（v0.5.0）：模型目录、用量幂等、Base URL `/v1`、重试/空闲超时、会话分页等。
- Windows 开机自启（v0.5.1）：自管 HKCU Run + StartupApproved；环境页「启动与关闭」置顶。

## 当前工作区状态（v0.5.1 发布中）

- 修复 Windows 开机自启无效：注册名固定 `AI-Switcher`、路径加引号、启用后校验、启动迁移清旧名并尝试重注册。
- 版本与 Release Notes 已更新为 0.5.1；推送 `main` 与带注释的 `v0.5.1` 标签后由 GitHub Actions 云端构建发布。
- 不要将 `release/` 调试文件或编译缓存纳入提交。

## 已验证结果

- `cargo test --lib autostart`：含 Windows 注册表往返测试，通过。
- `corepack pnpm exec tsc --noEmit`：通过。

## 后续操作建议

1. 推送标签后关注 Actions Release 工作流与 GitHub Release 产物（NSIS/MSI、签名、`latest.json`）。
2. 不要把用户本地的 `release/` 调试文件或编译缓存纳入提交。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
