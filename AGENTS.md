# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.9.0`（壳层 UI）；缺陷修复 `0.8.12` 已发布。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.9.0）

- 已发：`0.8.12` 用量多币种主显示 + Codex OpenAI 直连/多模型目录
- 进行中：壳层 UI P0–P5（侧栏分组、全局 Target、顶栏状态、供应商降噪、环境 Tabs、轻量抛光）
- Schema `user_version` = 20
- 规划 / 问题笔记：`task.md`、`bug.md`（gitignore）

## 已验证结果

- 0.8.12：usage/proxy_logs/Codex 相关单测 + `tsc` 通过并已打标签发布
- 0.9.0：前端 `tsc --noEmit` 本地验证

## 后续操作建议

1. 验证壳层 UI 后提交并发布 `v0.9.0`（或按需继续打磨再发）。
2. Windows 用户优先 NSIS；0.8.12 用户升级后打开一次应用以修复 Codex 直连。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
