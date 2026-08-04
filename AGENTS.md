# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.8.8`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.8.8）

- P1：代理热切换提示；Failover 分组/白名单/多跳；`ai-switcher://` Deep Link 预览导入
- Codex：归档会话扫描、provider sync 占用警告、托管 Base URL 偏出本地代理时启动修复
- 用量：热力图与用量页日期/来源独立；模型定价模糊匹配（`kimi-k3`↔`k3`）
- Schema `user_version` = 20
- 规划 / 问题笔记：`task.md`、`bug.md`（gitignore）
- 不要将 `release/` 调试文件或编译缓存纳入提交

## 已验证结果（0.8.8）

- 热切换 / failover / deeplink / pricing fuzzy / onboarding tip allowlist 单测：通过
- 前端 `tsc --noEmit`：通过

## 后续操作建议

1. 推送 `main` 与带注释的 `v0.8.8` 标签，由 GitHub Actions 云端构建发布。
2. Windows 用户优先 NSIS。
3. P2 未做：Skills Discovery、测速、全局 web_search、Plugins marketplace。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
