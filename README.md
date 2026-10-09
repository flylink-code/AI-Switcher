# AI-Switcher

本地配置与供应商管理器，面向 **Claude Code**、**Codex**、**OpenCode**、**Pi**、**Cline**。**v1.5.17-beta.1（本地测试版）**

**Beta 测试（未正式发布）**：统一供应商池，所有 Agent 共用一份上游配置；连接方式为智能网关、直连或官方。此包用于本地安装测试，尚未完成 GUI 与真实出网验收；最新正式版仍为 v1.5.16。

**升级注意**：开发版主资料库升级至 Schema 34，自动合并旧供应商并保留旧卡。迁移前生成 SQLite 一致性备份 `app.db.v33.bak`；旧版不能直接打开 Schema 34，降级前须退出新版并恢复备份或回滚导出库。建议另行备份 `~/.claude-switcher`。1.4.xx 热修仍走 `release/1.4.x`，不回写新 Schema。

[English](README_en.md) · [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) · [MIT](LICENSE)

Tauri 2 + Rust + React。把配置文件、系统凭据和本地目录收进一个界面。默认只在本机工作：API Key 进系统凭据库，改配置前备份，会话只读本地文件。

| 平台 | 安装包 | 说明 |
| --- | --- | --- |
| Windows 10/11 | NSIS `.exe`（推荐）/ MSI | 完整功能 |
| Linux（预览） | AppImage / `.deb` | Ubuntu 22.04 / Debian 12+（WebKitGTK 4.1） |

## 开始

1. 从 [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) 下载。Windows 用 NSIS（当前用户，通常无需管理员）；Linux 用 AppImage（先 `chmod +x`）。
2. **设置 → 工具与环境 → Agent 工具** 检测并安装 CLI（需要本机 **Node.js ≥22**）。
3. 在 **供应商** 填 API Key，或在 **账号与额度** 登录 Google / Antigravity。

## 功能

- **供应商**：一份全局上游池，API Key / OAuth 账号不再按 Agent 重复配置。支持预设、模型发现、连接测试、额度与元数据导入导出。「Agent 连接」统一选择 **智能网关（推荐）**、**直连（选择一个上游）** 或 **官方**。Code 直连仅限 Anthropic，Codex 仅限 OpenAI Chat / Responses；跨协议及 Codex OAuth 走网关。OpenCode / Pi / Cline 仅写所选单入口，保留用户自有配置。
- **智能网关**：独立本机服务 `127.0.0.1:15828`，模式路由、推理挡位、模型范围与条件规则。绑定 Agent 后写入指向该端口的 Auto 卡。用量按请求链路只计最内层花费。入口在主导航「网关」。
- **Antigravity 网关**：`127.0.0.1:15830`，把 Cloud Code 接到 Anthropic Messages / OpenAI Chat / Responses。浏览器登录账号池、按额度调度。个人自用，请自行评估上游条款。
- **Kiro 网关**：`127.0.0.1:15831`，账号池与额度调度。可导入 Builder ID、Social、Kiro IDE token，或用设备码 / Social PKCE 登录。用量只计 Kiro 这一跳。个人自用，请自行评估上游条款。
- **工作区**：MCP、Prompts、Skills、Agents、插件、项目快照；按当前 Agent 只显示其支持的 Tab。
- **会话与用量**：浏览、搜索、备份本地会话；合并代理日志与会话事件估算费用。
- **工具与汉化**：安装/更新各 Agent CLI；Claude Code、VS Code/Cursor 中文包（对照 GitHub latest，可装可卸）。

## 路径

| | |
| --- | --- |
| Claude Code | `~/.claude/` |
| Claude Desktop | `%LOCALAPPDATA%\Claude-3p\`（Windows）；**已从界面隐藏**，后端与资料库卡片仍保留 |
| Codex | `$CODEX_HOME` 或 `~/.codex/` |
| OpenCode | `~/.config/opencode/` · `~/.local/share/opencode/` |
| Pi | `~/.pi/agent/` |
| DSH | `~/.dsh/`；**已从界面隐藏**，后端与资料库卡片仍保留 |
| Cline | `~/.cline/`（sidecar `ai-switcher.json`；网关写 Auto，直连写所选上游） |
| 本应用 | `~/.claude-switcher/`（可迁移；库名保持兼容旧用户） |

导出 / 同步默认不含 API Key。

## 隐私

API Key 进 Windows Credential Manager / macOS Keychain / Linux Secret Service。配置原子写入并轮换备份。除连接测试、模型发现、更新检查和你主动确认的远端同步外，不上传本地内容。

## 开发

需要 Node.js 22+、pnpm 9+（Corepack）、Rust stable；Windows 还需 VS 2022 C++ 桌面组件。Dev 端口 **5250**。

```powershell
pnpm install
.\scripts\dev-hot.ps1       # 热加载；5250 被占则改用 5251+
.\scripts\clean-dev.ps1     # 停 debug，交还已安装版本
pnpm build:exe              # 正式 exe → release\AISwitcher.exe
```

## 边界

- 产品面为上述五个 Agent；Desktop / DSH 仅保留兼容后端，不在界面展示。
- Pi / DSH / Cline 不接插件、Agents、Profiles、托盘切换。
- 新直连不启动协议转换代理；升级前依赖本地代理的旧 T1 卡保留兼容链路，建议显式切换智能网关。
- 不同步远端冲突，不做团队分享。
- Linux 仅 Ubuntu 22.04 / Debian 12+。

## 许可与致谢

[MIT](LICENSE)。独立社区项目，与 Anthropic、OpenAI、Google 无隶属关系。Claude、Codex、ChatGPT 等为各自权利人商标。欢迎 Issue / PR。

思路参考：[Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) · [free-claude-code](https://github.com/Yeachan-Heo/free-claude-code) · [sub2api](https://github.com/sub2api) · [AI Toolbox](https://github.com/coulsontl/ai-toolbox) · [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) · [cc-switch](https://github.com/farion1231/cc-switch) · [code-switch](https://github.com/daodao97/code-swtich) · [Codex++](https://github.com/BigPizzaV3/CodexPlusPlus)

汉化：[taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn) · [shanjiancaofu/claude-code-vscode-zh-cn](https://github.com/shanjiancaofu/claude-code-vscode-zh-cn) · [javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn)
