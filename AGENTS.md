# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.5.2`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 已完成的主要能力

- 修复更新、关闭提示与托盘双击恢复行为。
- 统一调试版输出目录：`release/ClaudeSwitch-debug.exe`。
- Skills 仓库配置与多选安装，默认仓库为 `https://github.com/anthropics/skills`。
- MCP 官方 Registry 的浏览、搜索及安全添加（HTTP/SSE 与不含环境变量的 npm stdio）。
- “中文化配置”统一页面（Desktop / Claude Code / IDE 辅助扩展）。
- Codex 兼容与代理正确性（v0.5.0）。
- Windows 开机自启（v0.5.1）：自管 HKCU Run + StartupApproved；环境页「启动与关闭」置顶。
- Linux WebKit 渲染（v0.5.2）：启动时默认 `WEBKIT_DISABLE_DMABUF_RENDERER=1`，修复 Wayland UI 拖影。

## 当前工作区状态（v0.5.2 发布中）

- Linux 启动在 `src-tauri/src/main.rs` 写入 DMABUF 禁用（仅当环境变量未设置时）。
- 推送 `main` 与带注释的 `v0.5.2` 标签后由 GitHub Actions 云端构建发布。
- 不要将 `release/` 调试文件或编译缓存纳入提交。

## 已验证结果

- 用户在 Ubuntu 22.04 确认 `WEBKIT_DISABLE_DMABUF_RENDERER=1 ai-switcher` 可消除界面异常。

## 后续操作建议

1. 推送标签后关注 Actions Release 工作流与 GitHub Release 产物。
2. 不要把用户本地的 `release/` 调试文件或编译缓存纳入提交。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
