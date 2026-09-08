# AI-Switcher

本地配置与供应商管理器，面向 **Claude Code**、**Claude Desktop**、**Codex**、**OpenCode**、**Pi**、**DSH**、**Cline**。**v1.4.12**

**本版**：Claude Code 统一目录可开启 Opus Plan，规划与执行分模型（可跨供应商）。

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

- **供应商**：各 Agent 独立配置。Claude Code / Codex 可开统一模型目录，CLI `/model` 选用全部可见模型。卡片可复制到其他 Agent（自动改编协议与 URL）。OpenCode / Pi / DSH 多供应商并存，保存即同步。
- **本地代理**：Anthropic 兼容转发、密钥注入、429/5xx 故障切换。入口在设置 → 工具与环境 → 本地代理。
- **Antigravity 网关**：`127.0.0.1:15830`，把 Cloud Code 接到 Anthropic Messages / OpenAI Chat / Responses。浏览器登录账号池、按额度调度。个人自用，请自行评估上游条款。
- **工作区**：MCP、Prompts、Skills、Agents、插件、项目快照；按当前 Agent 只显示其支持的 Tab。
- **会话与用量**：浏览、搜索、备份本地会话；合并代理日志与会话事件估算费用。不解析 Claude Desktop 私有历史。
- **工具与汉化**：安装/更新各 Agent CLI；Claude Code、VS Code/Cursor、Desktop 中文包（对照 GitHub latest，可装可卸）。

## 路径

| | |
| --- | --- |
| Claude Code | `~/.claude/` |
| Claude Desktop | `%LOCALAPPDATA%\Claude-3p\`（Windows） |
| Codex | `$CODEX_HOME` 或 `~/.codex/` |
| OpenCode | `~/.config/opencode/` · `~/.local/share/opencode/` |
| Pi | `~/.pi/agent/` |
| DSH | `~/.dsh/` |
| Cline | `~/.cline/`（sidecar `ai-switcher.json`，代理 `:15827`） |
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

- 客户端就是上述七个 Agent；AG 网关把 Gemini / Cloud Code 接到它们。
- Pi / DSH / Cline 不接插件、Agents、Profiles、托盘切换。
- Pi / DSH 的 OpenAI 兼容上游直连；Cline 始终走本机代理 `:15827`。
- 不同步远端冲突，不做团队分享。
- Linux 仅 Ubuntu 22.04 / Debian 12+。

## 许可与致谢

[MIT](LICENSE)。独立社区项目，与 Anthropic、OpenAI、Google 无隶属关系。Claude、Codex、ChatGPT 等为各自权利人商标。欢迎 Issue / PR。

思路参考：[Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) · [free-claude-code](https://github.com/Yeachan-Heo/free-claude-code) · [sub2api](https://github.com/sub2api) · [AI Toolbox](https://github.com/coulsontl/ai-toolbox) · [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) · [cc-switch](https://github.com/farion1231/cc-switch) · [code-switch](https://github.com/daodao97/code-swtich) · [Codex++](https://github.com/BigPizzaV3/CodexPlusPlus)

汉化：[taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn) · [shanjiancaofu/claude-code-vscode-zh-cn](https://github.com/shanjiancaofu/claude-code-vscode-zh-cn) · [javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn)
