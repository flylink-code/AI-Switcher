# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.8.12`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.8.12）

- 修复：用量「全部」多币种主预估成本优先 USD，导致 CNY 大额被小额 USD 盖住
- 修复：Codex OpenAI 兼容供应商被写成依赖本地代理（代理未开则对话失败）；启动修复会改回直连上游 + 真实 API Key
- 修复：`ai-switcher-model-catalog.json` 只写默认模型导致无法切换；现合并 failover / 发现缓存 / 内置建议列表
- Schema `user_version` = 20
- 规划 / 问题笔记：`task.md`、`bug.md`（gitignore）
- 不要将 `release/` 调试文件或编译缓存纳入提交
- 下一阶段：壳层 UI（侧栏分组 + 全局 Target 等）走 `0.9.x`

## 已验证结果（0.8.12）

- usage / proxy_logs 多币种主显示单测：通过
- Codex 直连 / 多模型 catalog / requires_local_proxy 相关单测：本地跑
- 前端 `tsc --noEmit`：通过

## 后续操作建议

1. 推送 `main` 与带注释的 `v0.8.12` 标签，由 GitHub Actions 云端构建发布。
2. Windows 用户优先 NSIS；升级后打开一次 AI-Switcher，让启动修复把 Codex 从本地代理改回直连。
3. UI 壳层路线图见 `task.md`（发版后执行）。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
