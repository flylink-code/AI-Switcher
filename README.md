# Claude Switcher

> Visual third-party API configuration manager for **Claude Code** and **Claude Desktop**. Add providers, switch configurations independently, and avoid manual configuration-file editing.

[简体中文](#简体中文) · [English](#english)

---

## 简体中文

### 当前状态

当前已完成 **P0–P6**：基础工程、独立的 Claude Code / Claude Desktop 供应商管理、本地代理、MCP、Prompts、Skills、用量统计、托盘切换、主题/中英文和开机自启。

> **P6 说明**：Claude Code 与 Claude Desktop 的供应商列表、当前激活状态和 live 配置彼此独立。新数据库不会预置任何第三方供应商，两个应用均默认使用官方登录。

### 功能

- **独立供应商配置**
  - Claude Code：管理并安全写入 `~/.claude/settings.json`
  - Claude Desktop：管理并安全写入 `configLibrary` gateway profile
  - 分别新增、编辑、导入、排序、切换和恢复官方登录；切换一个应用不会更改另一个应用
- **本地代理**：本地 Anthropic `/v1/messages` 代理、模型映射、密钥注入、流式透传和请求日志。
- **MCP 与 Prompts**：统一管理 Claude Code / Desktop MCP；管理和激活 `CLAUDE.md` Prompt 预设。
- **Skills**：从公开 GitHub 仓库或本地 ZIP 安装，支持启停和删除。
- **用量统计**：代理请求数、状态、Token、趋势、按供应商/模型聚合，以及每百万 Token 定价的成本估算。
- **系统集成**：托盘中按应用独立快捷切换；浅色/深色/跟随系统；中文/English；开机自启。
- **安全写入**：原子写入、写前备份、自动轮换最近 10 个备份。

### 运行要求

- Node.js 20+、pnpm 9+
- Rust stable（MSVC 目标）
- Windows：Visual Studio 2022「使用 C++ 的桌面开发」工作负载与 Windows SDK

### 开发

```powershell
pnpm install
pnpm tauri dev
```

如果当前终端没有 MSVC 的环境变量，请使用仓库脚本：

```powershell
scripts\tauri-msvc.bat dev
```

常用命令：

```powershell
# Rust 静态检查 / 测试
cd src-tauri
..\scripts\cargo-msvc.bat check
..\scripts\cargo-msvc.bat test

# 构建 Windows MSI 和 NSIS 安装包
cd ..
scripts\tauri-msvc.bat build
```

### 数据和配置路径

| 路径 | 用途 |
|---|---|
| `~/.claude/settings.json` | Claude Code 的 live 配置 |
| `~/.claude.json` | Claude Code MCP 配置 |
| `%LOCALAPPDATA%\Claude\configLibrary\` | Claude Desktop gateway 配置（Windows） |
| `~/.claude/skills/` | Claude Code Skills |
| `~/.claude-switcher/app.db` | 本地 SQLite 数据库 |
| `~/.claude-switcher/backups/` | 自动轮换备份 |

### 项目结构

```text
src/                     React 19 前端、Ant Design、Zustand、i18next
src-tauri/src/config/    Claude Code/Desktop 配置发现、原子写入
src-tauri/src/database/  SQLite schema、迁移和 DAO
src-tauri/src/proxy/     本地 Anthropic 兼容代理与日志
src-tauri/src/commands/  Tauri IPC 命令
src-tauri/src/tray.rs    系统托盘与双应用快捷切换
task.md                  产品规划、阶段和后续任务
```

### 路线图

| 阶段 | 状态 | 产出 |
|---|---|---|
| P0–P5 | ✅ | 基础能力、供应商、Desktop/代理、MCP、Prompts、用量、Skills、托盘与自启 |
| P6 | 🚧 | Code/Desktop 独立供应商与迁移、无预置第三方供应商 |
| P7 | 计划中 | 系统凭据库、连接测试、模型发现、字段级回滚、数据维护 |
| P8 | 计划中 | OpenAI Chat/Responses 协议转换、代理健康状态、并发双应用代理 |

完整规划见 [task.md](./task.md)。

---

## English

### Status

**P0–P6** are implemented: project foundations, independent Claude Code and Claude Desktop provider management, local proxy, MCP, prompts, skills, usage dashboard, tray switching, themes/i18n, and launch at login.

> **P6:** Claude Code and Claude Desktop have independent provider lists, active selections, and live configuration. A fresh database contains no third-party providers; both applications default to official login.

### Features

- **Independent provider configuration** for Claude Code (`~/.claude/settings.json`) and Claude Desktop (`configLibrary`). Adding, importing, activating, or reverting a provider in one app never modifies the other.
- **Local proxy** for Anthropic `/v1/messages`: model mapping, key injection, streaming pass-through, and request logging.
- **MCP and prompts** management across both Claude applications.
- **Skills** installation from public GitHub repositories or local ZIP archives, plus enable/disable and deletion.
- **Usage dashboard** for proxied requests, status, tokens, trends, provider/model breakdowns, and estimated cost based on custom model prices.
- **System integration**: per-application tray switching, light/dark/system theme, Chinese/English UI, and launch at login.
- **Safe configuration writes**: atomic writes, pre-write backups, and rotation of the latest ten backups.

### Requirements

- Node.js 20+ and pnpm 9+
- Stable Rust with the MSVC target
- On Windows: Visual Studio 2022 Desktop development with C++ workload and a Windows SDK

### Development

```powershell
pnpm install
pnpm tauri dev
```

Use the bundled MSVC wrapper when the terminal does not already have MSVC environment variables:

```powershell
scripts\tauri-msvc.bat dev
```

```powershell
# Rust check / tests
cd src-tauri
..\scripts\cargo-msvc.bat check
..\scripts\cargo-msvc.bat test

# Build Windows MSI and NSIS installers
cd ..
scripts\tauri-msvc.bat build
```

### Roadmap

| Phase | Status | Deliverable |
|---|---|---|
| P0–P5 | ✅ | Foundations, providers, Desktop/proxy, MCP, prompts, usage, skills, tray, and autostart |
| P6 | 🚧 | Separate Code/Desktop providers and migration; no bundled third-party providers |
| P7 | Planned | OS credential storage, connection tests, model discovery, field-level rollback, data maintenance |
| P8 | Planned | OpenAI Chat/Responses conversion, proxy health, simultaneous per-app proxy routing |

See [task.md](./task.md) for the complete plan.
