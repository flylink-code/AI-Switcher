# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区为 `1.0.3`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v1.0.3）

- 修复：供应商模型映射打开编辑被默认模型同步覆盖的竞态；默认变更只同步空/旧默认角色
- 修复：Responses 用量把 total `input_tokens` 当 fresh 导致 proxy↔session 去重失败双重计数；`*-fast` 模型去重互认
- UI：供应商表单角色映射两列布局；故障切换分组与白名单并排
- 壳层 / Desktop 中文包 / 更新后恢复：沿用 1.0.2
- Schema `user_version` = 20（沿用）
- 规划 / 问题笔记：`task.md`、`bug.md`（gitignore）
- 不要将 `release/` 调试文件或编译缓存纳入提交

## 已验证结果（1.0.3）

- 前端 `tsc --noEmit`：通过
- `usage_parser` Responses cache 相关 Rust 单测：通过

## 后续操作建议

1. 推送 `main` 与带注释的 `v1.0.3` 标签，由 GitHub Actions 云端构建发布。
2. Windows 用户优先 NSIS。
3. 已损坏的角色映射需用户重新填写并保存；历史用量可用「重建 Codex 会话用量」。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
