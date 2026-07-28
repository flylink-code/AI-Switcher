# 项目交接上下文

## 项目概览

- 项目：Claude Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.2.1`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 已完成的主要能力

- 修复更新、关闭提示与托盘双击恢复行为。
- 统一调试版输出目录：`release/ClaudeSwitch-debug.exe`。
- Skills 仓库配置与多选安装，默认仓库为 `https://github.com/anthropics/skills`。
- MCP 官方 Registry 的浏览、搜索及安全添加（HTTP/SSE 与不含环境变量的 npm stdio）。
- “中文化配置”统一页面：
  - 保留 Claude Desktop 中文化能力；
  - Claude Code 通过插件管理器安装中文插件，并备份后写入基础中文设置；
  - 仅检测到官方 Claude Code 扩展后，才为 VS Code/Cursor 安装中文辅助扩展；不自动执行补丁。

## 当前工作区状态（v0.2.1 已发布）

- 已提交中文化配置及 v0.2.1 版本变更：`befa354 feat: add unified localization hub for v0.2.1`。
- 已推送 `main` 和带注释的 `v0.2.1` 标签；GitHub Actions 已成功完成云端构建与发布：`https://github.com/flylink-code/AI-Switcher/actions/runs/30265100959`。
- Release 已由工作流自动创建，包含云端构建的 NSIS/MSI、签名文件和 `latest.json`；无需上传本机安装包。
- `.github/workflows/release.yml` 已更新为本次中文化配置功能的中英双语 Release Notes。
- 工作区仅保留本文件的交接上下文修改；不要将 `release/` 调试文件或编译缓存纳入提交。
- GitHub CLI 账号 `flylink-code` 的令牌仍失效；后续若需使用 `gh` 写入或管理 Release，请先执行 `gh auth login -h github.com`。

## 已验证结果

- `cargo test`：99 项通过。
- `corepack pnpm build`：TypeScript 检查与 Vite 生产构建通过。
- 调试构建：`corepack pnpm build:exe:debug -- skip-tests`，输出到 `release/ClaudeSwitch-debug.exe`。

## 后续操作建议

1. 后续若需手动维护 GitHub Release，先重新认证 GitHub CLI。
2. 不要把用户本地的 `release/` 调试文件或编译缓存纳入提交。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
