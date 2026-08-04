# AI-Switcher

> 面向 **Claude Code**、**Claude Desktop** 与 **Codex** 的本地配置与供应商管理器。**v1.0.0**

[English](README_en.md) · [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) · [License: MIT](LICENSE)

基于 **Tauri 2 + Rust + React**。把分散在配置文件、系统凭据库和本地目录里的能力收进一个界面；Claude Code、Claude Desktop、Codex 的供应商与当前激活配置彼此独立。

默认只在本机工作：API Key 进系统凭据库，改配置前自动备份，会话只读本地 JSONL。

| 平台 | 安装包 | 说明 |
|---|---|---|
| Windows 10/11 | NSIS `.exe`（推荐）/ MSI | 完整功能 |
| Linux（预览） | AppImage / `.deb` | 尽力支持；已覆盖 Claude Desktop 官方 Linux 配置路径；中文化等部分能力仍受限 |

---

## 开源说明

本项目以 **[MIT License](LICENSE)** 开源，源码位于 [flylink-code/AI-Switcher](https://github.com/flylink-code/AI-Switcher)。

- **可以**：自由使用、修改、分发、商用；衍生作品可用其他许可证（保留 MIT 声明与版权即可）
- **需要**：在副本或显著部分中保留 `LICENSE` 中的版权与许可声明
- **无关声明**：AI-Switcher 为独立社区项目，与 Anthropic、OpenAI 及下述参考项目均无隶属、赞助或官方关系；Claude、Claude Code、Claude Desktop、Codex、ChatGPT 等为各自权利人的商标
- **第三方依赖**：构建产物会链接 npm / crates 等第三方库，请同时遵守其各自许可证
- **思路参考**：下文「参考与致谢」中的项目仅作产品与实现思路参考；若你移植了其中受版权保护的代码，须另行遵守对应上游许可证（例如 AGPL-3.0 项目）
- **欢迎贡献**：Issue / PR 请提交到 GitHub 仓库；合入代码默认按本仓库 MIT 许可授权

---

## 安装

从 [GitHub Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) 下载最新版：

- **Windows**：优先 NSIS 安装包（当前用户安装，通常无需管理员）。安装后主程序为 `AISwitcher.exe`。
- **Linux**：优先 `.AppImage`（`chmod +x` 后运行）。

按需安装 Claude Code、Claude Desktop 或 Codex CLI。

---

## 能做什么

### 工作区壳层（1.0）

- **总览**：用量四指标汇总 + 每日活跃热力图（GitHub 风格，短/长周期自适应排布）
- **全局工具切换**：顶栏在 Claude Code / Desktop / Codex 间切换，侧栏与页面跟随同一工作区上下文
- **状态台**：顶栏显示代理状态与当前供应商，可一键跳转对应页面
- **侧栏分组**：核心 / 扩展 / 数据 / 系统

### 供应商与切换

- 分别管理 Claude Code / Desktop / Codex 的第三方 API、模型映射、导入导出、连接测试、Base URL 测速与模型发现
- Codex 供应商可开关 catalog **Web Search**（写入模型目录 `supports_search_tool` / `web_search_tool_type`）
- 环境页可设置全局顶层 `web_search`：`disabled | cached | indexed | live`（与 catalog 开关层次不同；不写已弃用 `features.web_search*`）
- 一键切换并备份；可恢复官方登录配置
- Claude 侧可用 **ChatGPT 订阅**（经本地代理）；Codex 官方账号用终端 `codex login`
- Codex 写入 `~/.codex/config.toml`；OpenAI 兼容上游可直连，Anthropic / OAuth 仍经本地代理
- Deep Link：`ai-switcher://v1/import?resource=provider|mcp&payload=...`（导入前预览确认）

### 本地代理

Anthropic Messages 兼容转发、模型映射、密钥注入、流式请求、状态与日志。可选自动故障切换（默认关闭）。**经本地代理的会话可热切换上游**；直连非代理场景仍可能需重启 CLI。

### MCP / Prompts / Skills / Agents / Plugins

- MCP：统一维护并可同步到 Codex；支持远程 HTTP/SSE、OAuth 状态清理，以及 Desktop Connectors / `.mcpb` 冲突提示
- MCP Registry：浏览官方 Registry 并安装可安全转换为 Claude 配置的条目（需密钥/URL 模板的仍需手动配置）
- Prompts：`CLAUDE.md` / Codex `AGENTS.md` 预设，支持重命名与一键激活
- Skills：Claude Code 与 Codex 支持 GitHub / ZIP 安装、启停、更新与删除；可扫描散落 Skill 一键登记/忽略
- Agents：管理 Claude Code 用户级 `~/.claude/agents`
- Codex Plugins：启停、卸载；包装 `codex plugin marketplace list/add/remove`（不做完整商店浏览）

### 项目（Profiles）

为 Claude Code / Desktop / Codex 分别快照供应商、MCP、Skills、Prompt 选择；可一键应用与重命名。

### 会话

浏览、筛选、搜索 Claude Code 与 Codex 本地 JSONL；支持导出 / 导入 / 备份 / 回收站。不解析 Claude Desktop 私有历史格式。

### 中文化

Claude Code 插件、编辑器补丁助手、Claude Desktop 语言包分区管理；编辑器补丁始终需在编辑器内确认。安装中文时会规范化错误的 `spinnerVerbs` 数组格式。

### 用量、环境与系统

- 用量：合并代理日志与 Codex / Claude Code 本地会话事件（含 Anthropic 兼容第三方直连的 JSONL 回填）；支持多币种预估（汇总按最大绝对值选主币种）；识别 Opus / Codex Fast tier（`*-fast`）
- 环境：配置路径、资料库迁移 / 便携导出、WSL·SSH 同步、**doctor 诊断与一键可见性修复**（不强制改写直连 `ANTHROPIC_BASE_URL`）；环境页按 Tabs 组织
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
| `$CODEX_HOME` 或 `~/.codex/` | Codex 配置、会话、Skills、Plugins |
| `~/.claude/agents/` | Claude Code Agents |
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
- Codex Plugins 仅本地探测与启停，不做完整官方商店
- 会话「恢复」只复制命令，不自动开终端
- 不同步自动合并远端冲突，不做团队分享
- Claude Code 与 Desktop 的供应商列表与激活状态始终独立
- 不解析 Claude Desktop 私有会话格式

---

## 参考与致谢

独立项目，与下列仓库及 Anthropic / OpenAI 均无隶属关系。表中许可证指**上游仓库自身**的许可；AI-Switcher 源码仍以本仓库 [MIT](LICENSE) 为准。引用或移植上游代码时，请同时遵守其许可证与版权声明。

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
