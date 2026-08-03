# 项目交接上下文

## 项目概览

- 项目：Claude Switcher / AI-Switcher，Windows 桌面端，用于统一配置 Claude Desktop、Claude Code、Codex 及相关工具。
- 技术栈：Tauri 2（Rust 后端）+ React + TypeScript + Vite（前端）+ pnpm。
- 版本：工作区已更新为 `0.8.3`（`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`）。
- 构建与测试：允许执行本项目的编译和测试；包管理器请使用 Corepack 读取 `packageManager` 锁定的 pnpm 版本。

## 当前工作区状态（v0.8.3）

- v0.8.3：修复 Claude Code `settings.json` 中 `spinnerVerbs` 被写成数组导致整份设置失效；安装中文时自动规范化。
- v0.8.2：用量统计范围 / 来源筛选等页面偏好持久化到 localStorage。
- v0.8.1：新增供应商时在弹窗内选择常用预设快速填写（移除页顶按钮）。
- v0.8.0：Claude Code Agents 管理；MCP 远程 HTTP/SSE 与 OAuth 状态/清理；Desktop Connectors/`.mcpb` 冲突提示；用量统计支持 CNY 等成本汇总。
- 规划见 `task.md`（0.8 已落地；随后项待定）；`task.md` / `bug.md` 为本地规划（gitignore）。
- 不要将 `release/` 调试文件或编译缓存纳入提交。

## 已验证结果

- `normalizes_bare_spinner_verbs_array_to_object`：通过

## 后续操作建议

1. 推送 `main` 与带注释的 `v0.8.3` 标签，由 GitHub Actions 云端构建发布。
2. Windows 用户优先 NSIS；Linux 优先 AppImage。

# 注释
本项目运行本地编译测试
编译脚本：scripts/
