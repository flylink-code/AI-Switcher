# AI-Switcher

面向 **Claude Code**、**Claude Desktop**、**Codex**、**VS Code Copilot**、**OpenCode**、**Pi** 的本地 AI 配置、统一供应商与智能网关工作台。**v1.6.1**

所有 Agent 共用一份全局上游池（`upstreams`），连接方式统一为 **智能网关（推荐）**、**直连** 或 **官方**。内置 Antigravity / Kiro 反代网关默认以独立 Sidecar 子进程（`--gateway-sidecar`）运行，与主界面进程隔离并自动守护。

**升级与架构重构说明**：
- **资料库迁移**：主资料库已升级至 **Schema 37**。从 1.5.16（Schema 33）升级前会自动生成 `app.db.v33.bak`。旧版程序无法打开新 Schema 数据库；如需降级，请先完全退出新版并恢复备份文件，建议升级前另行备份 `~/.claude-switcher`。1.4.xx 热修仍走 `release/1.4.x` 分支。
- **导航与架构重构**：主导航重构为 7 项（**概览 / Agent / 供应商 / 网关 / 用量 / 工作区 / 设置**），支持顶栏（默认）与侧栏双布局切换；全局供应商池、智能网关聚合目录、VS Code Copilot 托管模型与独立反代网关全面打通。

[English](README_en.md) · [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) · [MIT](LICENSE)

基于 Tauri 2 + Rust + React 构建。将散落在各处的 Agent 配置文件、系统安全凭据、OAuth 账号池、本地会话与 Token 账单收归同一界面。默认纯本机运行：API Key 存入系统凭据库，修改配置前原子备份，会话数据仅读取本机文件。

| 平台 | 安装包 | 说明 |
| --- | --- | --- |
| Windows 10/11 | NSIS `.exe`（推荐）/ MSI | 完整功能（含 Claude Desktop 托管与汉化） |
| Linux（预览） | AppImage / `.deb` | Ubuntu 22.04 / Debian 12+（WebKitGTK 4.1） |

## 快速开始

1. 从 [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) 下载安装包。Windows 推荐使用 NSIS `.exe`（当前用户安装，通常无需管理员权限）；Linux 使用 AppImage（先执行 `chmod +x`）。
2. 打开 **Agent** 页，一键检测、安装或升级各 Agent CLI（需本机 **Node.js ≥ 22**）。
3. 在 **供应商** 页添加 API Key 上游（支持预设模板、自动获取模型列表与连通性探测），或在 **网关** 页登录 **Antigravity / Kiro / ChatGPT (Codex OAuth)** 账号池。
4. 回到 **Agent** 页，将目标 Agent 切换为 **智能网关** 或指定 **直连** 上游即可开始使用。

## 核心功能

- **Agent 中心（`Agent` 页）**：
  - 集中管理 **Claude Code**、**Claude Desktop**、**Codex**、**VS Code Copilot**、**OpenCode**、**Pi** 的连接状态与 CLI 版本（支持卡片内一键升级）。
  - **连接模式**：统一提供 **智能网关（推荐）**、**直连（单选上游）** 与 **官方**。Claude Code 直连限 Anthropic 协议，Codex 直连限 OpenAI Chat / Responses 协议；跨协议转换与 Codex OAuth 统一经由智能网关。
  - **VS Code Copilot 深度集成**：将智能网关聚合模型一键写入 VS Code 原生 `chatLanguageModels.json`（仅维护名为 `AI-Switcher` 的托管项，完整保留官方模型与用户自定义 Provider），原生支持 Gemini / Claude / GPT 多档推理等级（`low` / `medium` / `high`）选择与网关路由联动。
  - **高级设置与中文化**：支持 Claude Code / Desktop 直连角色模型映射、Codex 联网搜索与输出档位配置；内置 Claude Code、VS Code / Cursor 扩展、Claude Desktop 一键中文包安装与卸载（对照 GitHub latest）。
- **全局供应商与智能网关（`供应商` 页 · `127.0.0.1:15828`）**：
  - **唯一上游池**：API Key 与 OAuth 账号池全局配置一次，所有 Agent 共享。支持拖拽/上下调整上游优先级（`sort_index`）、按模型粒度开启/隐藏、额度查询与无密钥元数据导入导出。
  - **同名模型自动区分**：单一来源模型保持简洁裸名；当多个上游提供同名模型时，公开目录 ID 自动附加供应商前缀（如 `<provider>.<model>`），精准命中指定上游。
  - **跨协议桥接与防断连**：支持 Anthropic Messages ↔ OpenAI Chat / Responses 双向转换、Responses API 流式即时握手、长上下文思考流保活，以及 Codex 本地环回地址（`localhost:15828`）自动绕过系统代理干扰。
- **反代网关与账号池（`网关` 页）**：
  - **独立 Sidecar 守护**：Antigravity 与 Kiro 反代默认运行于独立子进程（`--gateway-sidecar`），可在「设置 → 通用」切换运行模式并查看实时监督状态。
  - **Antigravity 反代（`127.0.0.1:15830`）**：Google OAuth 多账号池与额度感知调度，将 Cloud Code 映射为标准 Anthropic Messages / OpenAI Chat / Responses 接口。
  - **Kiro 反代（`127.0.0.1:15831`）**：支持导入 Builder ID、Social PKCE、Kiro IDE Token 或设备码登录，内置完整 AWS EventStream 帧解码、连接超时重试与账号自动轮换。
  - **ChatGPT 账号池（Codex OAuth）**：支持多账号登录、一键切换默认账号，并作为智能网关上游供各 Agent 调用。
- **工作区（`工作区` 页）**：
  - 顶部先选 Agent，下方动态展示该 Agent 支持的资源页签：**会话管理**（置顶）、**项目快照（Profiles）**、**MCP 服务**、**提示词（Prompts）**、**技能（Skills）**、**Agent 智能体**、**插件（Plugins）**。
- **用量与可观测性（`用量` 页 & `概览` 页）**：
  - **多跳链路去重**：同一请求链路（`correlation_id`）仅统计最内层实际消耗跳（`kiro` / `antigravity` 优先于 `smart_gateway`），绝不双计；`count_tokens` 探测不计入账单。
  - **供应商统一归并**：自动合并内置反代与网关别名（统一归并为 `Antigravity`、`Kiro`、`ChatGPT` 等），合并实时代理日志与本地会话增量同步（Claude Code / Codex / OpenCode / Pi），支持多币种折算与 Excel 价目表导入导出。

## 本机端口与配置路径

### 本机监听端口（仅 `127.0.0.1` / `localhost`）

| 端口 | 服务 | 说明 |
| --- | --- | --- |
| **15828** | 智能网关 | 聚合模型目录与跨协议路由入口；Claude Code / Codex / VS Code Copilot 绑定后直连该端口 |
| **15830** | Antigravity 反代 | Google Cloud Code 账号池反代网关（默认 Sidecar 进程运行） |
| **15831** | Kiro 反代 | Kiro 账号池反代网关（默认 Sidecar 进程运行） |
| **15822** | Claude Desktop 本地代理 | 为 Claude Desktop 提供 `/v1/models` 别名映射与探活本地应答 |
| 15821 / 15823 / 15826 / 15827 | 兼容本地代理 | Code / Codex / DSH / Cline 旧版兼容端口 |

### 配置文件路径

| 目标 | 路径与说明 |
| --- | --- |
| Claude Code | `~/.claude/`（`settings.json` 等） |
| Claude Desktop | `%LOCALAPPDATA%\Claude-3p\`（Windows） |
| Codex | `$CODEX_HOME` 或 `~/.codex/`（`config.toml`、`auth.json`、`ai-switcher-model-catalog.json`） |
| VS Code Copilot | `%APPDATA%\Code\User\chatLanguageModels.json`（仅维护 `AI-Switcher` 托管项） |
| OpenCode | `~/.config/opencode/` · `~/.local/share/opencode/` |
| Pi | `~/.pi/agent/` |
| Cline / DSH | `~/.cline/`（`ai-switcher.json`）· `~/.dsh/`；**T3 兼容保留，已从界面隐藏** |
| 本应用资料库 | `~/.claude-switcher/`（支持迁移目录；导出与远端同步默认不含 API Key） |

## 隐私与安全

API Key 存储在系统安全凭据库（Windows Credential Manager / macOS Keychain / Linux Secret Service）。所有配置文件采用原子写入并保留轮换备份。除你主动触发的连接测试、模型发现、版本更新检查和远端同步外，不向外部上传任何本地数据。

## 开发与构建

需要 Node.js 22+、pnpm 9+（Corepack）、Rust stable；Windows 需安装 VS 2022 C++ 桌面开发组件。开发服务器默认端口 **5250**。

```powershell
pnpm install
pnpm dev:hot                # 热加载开发；若 5250 被占用可在脚本中临时切至 5251
pnpm clean:dev              # 停止调试进程并恢复已安装正式版状态
pnpm build:exe              # 编译正式版二进制 → release\AISwitcher.exe
pnpm build:exe:bundle       # 编译并打包 NSIS (.exe) 与 MSI 安装包
```

## 边界说明

- 产品面聚焦 **Claude Code、Claude Desktop、Codex、VS Code Copilot、OpenCode、Pi** 六大入口；Cline 与 DSH 仅保留后端、Schema 与配置兼容，不在界面展示。
- Antigravity / Kiro / ChatGPT OAuth 账号池仅供个人自用学习，请自行评估并遵守对应上游服务条款。
- OpenCode / Pi 仅维护选定的托管入口，不接管插件、子智能体与项目快照。
- 不做多端实时冲突合并，不做局域网公开监听或团队共享服务端。
- Linux 预览版仅支持 Ubuntu 22.04 / Debian 12+。

## 许可与致谢

[MIT](LICENSE)。独立开源社区项目，与 Anthropic、OpenAI、Google、GitHub、Amazon 无隶属关系。Claude、Codex、ChatGPT、Copilot、Gemini 等名称为各自权利人商标。欢迎提交 Issue 与 PR。

思路参考：[Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) · [free-claude-code](https://github.com/Yeachan-Heo/free-claude-code) · [sub2api](https://github.com/sub2api) · [AI Toolbox](https://github.com/coulsontl/ai-toolbox) · [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) · [cc-switch](https://github.com/farion1231/cc-switch) · [code-switch](https://github.com/daodao97/code-swtich) · [Codex++](https://github.com/BigPizzaV3/CodexPlusPlus)

汉化致谢：[taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn) · [shanjiancaofu/claude-code-vscode-zh-cn](https://github.com/shanjiancaofu/claude-code-vscode-zh-cn) · [javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn)

