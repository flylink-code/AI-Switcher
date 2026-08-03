# AI-Switcher

> 面向 **Claude Code**、**Claude Desktop** 与 **Codex** 的本地配置与供应商管理器。

[English](README_en.md) · [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest)

基于 **Tauri 2 + Rust + React**。把分散在配置文件、系统凭据库和本地目录里的能力收进一个界面；Claude Code、Claude Desktop、Codex 的供应商与当前激活配置彼此独立。

默认只在本机工作：API Key 进系统凭据库，改配置前自动备份，会话只读本地 JSONL。

| 平台 | 安装包 | 说明 |
|---|---|---|
| Windows 10/11 | NSIS `.exe`（推荐）/ MSI | 完整功能 |
| Linux（预览） | AppImage / `.deb` | 尽力支持；Claude Desktop 中文化等部分能力不可用 |

---

## 安装

从 [GitHub Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) 下载最新版：

- **Windows**：优先 NSIS 安装包（当前用户安装，通常无需管理员）。安装后主程序为 `AISwitcher.exe`。
- **Linux**：优先 `.AppImage`（`chmod +x` 后运行）。

按需安装 Claude Code、Claude Desktop 或 Codex CLI。

---

## 能做什么

### 供应商与切换

- 分别管理 Claude Code / Desktop / Codex 的第三方 API、模型映射、导入导出、连接测试与模型发现
- 一键切换并备份；可恢复官方登录配置
- Claude 侧可用 **ChatGPT 订阅**（经本地代理）；Codex 官方账号用终端 `codex login`
- Codex 写入 `~/.codex/config.toml`，不经过 Claude 本地代理

### 本地代理

Anthropic Messages 兼容转发、模型映射、密钥注入、流式请求、状态与日志。可选自动故障切换（默认关闭）。

### MCP / Prompts / Skills

统一维护 MCP（可同步到 Codex）、`CLAUDE.md` 预设；Claude Code 与 Codex Skills 支持 GitHub / ZIP 安装、启停、更新与删除。

### 会话

浏览、筛选、搜索 Claude Code 与 Codex 本地 JSONL；支持导出 / 导入 / 备份 / 回收站。不解析 Claude Desktop 私有历史格式。

### 中文化

Claude Code 插件、编辑器补丁助手、Claude Desktop 语言包分区管理；编辑器补丁始终需在编辑器内确认。

### 用量、环境与系统

- 用量：合并代理日志与 Codex 本地事件（预估成本仅来自代理日志）
- 环境：配置路径、资料库迁移 / 便携导出、WSL·SSH 同步预览推送、Claude Code / Codex CLI 安装检测
- 托盘快捷切换、中英界面、浅色 / 深色 / 跟随系统、开机自启

---

## 会话说明

| 来源 | 路径 |
|---|---|
| Claude Code | `~/.claude/projects/**/*.jsonl` |
| Codex | `$CODEX_HOME/sessions/**/*.jsonl`（默认 `~/.codex/sessions/`） |

列表只读元数据；打开详情或全文搜索时才读消息。路径限制在会话根目录内。浏览不改原文件。

Claude Desktop 仅检测数据目录并提供官方入口 `claude://claude.ai/new`；已知会话 ID 可用 [官方深链](https://support.claude.com/en/articles/14729294-open-claude-desktop-with-a-link)。

---

## 数据与配置

| 路径 | 用途 |
|---|---|
| `~/.claude/settings.json` | Claude Code 当前供应商 |
| `~/.claude.json` | Claude Code MCP / 项目配置 |
| `~/.claude/projects/` | Claude Code 会话 |
| `~/.claude/skills/` | Claude Code Skills |
| `%LOCALAPPDATA%\Claude-3p\configLibrary\` | Claude Desktop 第三方配置（Windows） |
| `$CODEX_HOME` 或 `~/.codex/` | Codex 配置、会话、Skills |
| `~/.claude-switcher/`（可改） | 本应用资料库：数据库、备份、日志 |

产品名已改为 AI-Switcher，仍保留原应用标识与默认资料库路径以兼容旧用户。资料库可迁到其他盘（SHA-256 校验，重启生效）。导出 / 同步默认不含 API Key。

---

## 安全与隐私

- API Key：Windows Credential Manager / macOS Keychain / Linux Secret Service
- 配置：原子写入 + 轮换备份
- 会话：不建全文库；导入导出与回收站校验根目录与符号链接
- 除连接测试、模型发现、更新检查、用户主动下载与确认的远端归档推送外，不上传本地内容

---

## 从源码开发

需要：Node.js 20+、pnpm 9+（可用 Corepack）、Rust stable。Windows 还需 VS 2022 C++ 桌面开发组件。

```powershell
pnpm install
pnpm tauri dev
# 无 MSVC 环境变量时：
scripts\tauri-msvc.bat dev
```

### 构建（Windows）

脚本默认先跑完整 Rust 测试：

```powershell
pnpm build:exe              # 正式版 exe → release\AISwitcher.exe
pnpm build:exe:debug        # 调试版 → release\AISwitcher-debug.exe
pnpm build:exe:bundle       # MSI + NSIS
scripts\build-exe.bat release skip-tests   # 跳过测试
```

| 产物 | 路径 |
|---|---|
| 正式版 | `src-tauri\target\release\AISwitcher.exe` |
| 测试副本 | `release\AISwitcher.exe` / `AISwitcher-debug.exe` |
| 安装包 | `src-tauri\target\release\bundle\` |

---

## 项目结构

```text
src/                  React + Ant Design + Zustand + i18next
src-tauri/src/        Rust：配置、代理、数据库、托盘、会话
scripts/              Windows 开发 / 构建脚本
```

---

## 当前边界

- 产品范围：Claude Code + Claude Desktop + Codex（不做 Grok / Gemini 等）
- 会话「恢复」只复制命令，不自动开终端
- 不同步自动合并远端冲突，不做团队分享
- Claude Code 与 Desktop 的供应商列表与激活状态始终独立

---

## 参考与致谢

独立项目，与下列仓库及 Anthropic 均无隶属关系。引用或移植请遵守各自许可证。

| 项目 | 参考方向 | 上游 |
|---|---|---|
| AI Toolbox | 多工具配置、会话与桌面信息架构 | [coulsontl/ai-toolbox](https://github.com/coulsontl/ai-toolbox) MIT |
| cc Proxy | Desktop 本地代理与模型替换 | [arhsis/cc-proxy](https://github.com/arhsis/cc-proxy) |
| CC Switch | 供应商切换、Tauri、会话与托盘 | [farion1231/cc-switch](https://github.com/farion1231/cc-switch) MIT |
| Claude Code VS Code 中文包 | 扩展定位与汉化流程 | [zstings/claude-code-zh-cn](https://github.com/zstings/claude-code-zh-cn) MIT |
| Claude Code 中文插件 | CLI 中文化安装与恢复 | [taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn) |
| Claude Desktop 中文补丁 | 安装发现与语言包 | [javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn) |
| Code Switch | 本地代理、故障切换、Codex 配置 | [daodao97/code-swtich](https://github.com/daodao97/code-swtich) Apache-2.0 |
| Codex++ | Codex API 写入与历史会话同步 | [BigPizzaV3/CodexPlusPlus](https://github.com/BigPizzaV3/CodexPlusPlus) AGPL-3.0 |
