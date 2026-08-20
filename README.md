# AI-Switcher

> 面向 **Claude Code**、**Claude Desktop**、**Codex**、**OpenCode**、**Pi CLI** 与 **DeepSeek Harness** 的本地配置与供应商管理器。**v1.3.20**

[English](README_en.md) · [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) · [License: MIT](LICENSE)

基于 **Tauri 2 + Rust + React**。把分散在配置文件、系统凭据库和本地目录里的能力收进一个界面；Claude Code、Claude Desktop、Codex、OpenCode、Pi、DeepSeek Harness 的供应商彼此独立（OpenCode / Pi / DeepSeek Harness 为多供应商并存、保存即同步）。

默认只在本机工作：API Key 进系统凭据库，改配置前自动备份，会话只读本地 JSONL。


| 平台            | 安装包                  | 说明                                                                 |
| ------------- | -------------------- | ------------------------------------------------------------------ |
| Windows 10/11 | NSIS `.exe`（推荐）/ MSI | 完整功能                                                               |
| Linux（预览）     | AppImage / `.deb`    | **Ubuntu 22.04 / Debian 12 及以上**（WebKitGTK 4.1）；18.04 / 20.04 无法运行 |


---



## 开源说明

本项目以 **[MIT License](LICENSE)** 开源，源码位于 [flylink-code/AI-Switcher](https://github.com/flylink-code/AI-Switcher)。

- **可以**：自由使用、修改、分发、商用；衍生作品可用其他许可证（保留 MIT 声明与版权即可）
- **需要**：在副本或显著部分中保留 `LICENSE` 中的版权与许可声明
- **无关声明**：AI-Switcher 为独立社区项目，与 Anthropic、OpenAI 及下述参考项目均无隶属、赞助或官方关系；Claude、Claude Code、Claude Desktop、Codex、ChatGPT、Pi 等为各自权利人的商标
- **第三方依赖**：构建产物会链接 npm / crates 等第三方库，请同时遵守其各自许可证
- **思路参考**：下文「参考与致谢」中的项目仅作产品与实现思路参考；若你移植了其中受版权保护的代码，须另行遵守对应上游许可证（例如 AGPL-3.0 项目）
- **欢迎贡献**：Issue / PR 请提交到 GitHub 仓库；合入代码默认按本仓库 MIT 许可授权

---



## 安装

从 [GitHub Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) 下载最新版：

- **Windows**：优先 NSIS 安装包（当前用户安装，通常无需管理员）。安装后主程序为 `AISwitcher.exe`。
- **Linux**：优先 `.AppImage`（`chmod +x` 后运行）。系统需 **Ubuntu 22.04 / Debian 12** 或更新（`libwebkit2gtk-4.1`）。Ubuntu 18.04 / 20.04 没有 WebKitGTK 4.1，Tauri 2 无法提供兼容包。

按需安装 Claude Code、Claude Desktop、Codex CLI、OpenCode（CLI / Desktop）、Pi 或 DeepSeek Harness。Agent 工具安装/更新需要本机 **Node.js ≥22**（应用内「设置 → 工具与环境 → Agent 工具」可检测并安装环境）。

---



## 能做什么



### 导航与布局

- **双布局**：标题栏可切换 **左侧导航** / **顶部导航**（类似浏览器默认标签与垂直标签）；偏好写入 `cs.layoutMode`
- **顶部导航七项**：概览 · 供应商 · 用量统计 · 账号与额度 · 工作区 · **会话** · 设置
- **概览**：状态条 → 最近 24 小时用量 Hero → 需要关注 / 最近活动 → 过去一年热力图（Usage Intelligence）
- **设置子页**（工具与环境）：本地代理、环境信息、**Agent 工具**、汉化、关于（带「← 设置」返回；会话已提升为主导航）
- **显示 Agent**：设置页可勾选哪些工具出现在全局与各页切换器中
- **工作区按 Agent 过滤 Tab**：先选 Agent，再只显示其支持的 MCP / Prompts / Skills / Agents / Plugins / 项目
- **Agent 切换**：供应商页 / 代理页 / 工作区内独立切换（Claude Code / Desktop / Codex / OpenCode / Pi / DeepSeek Harness）



### 供应商与切换

- 分别管理 Claude Code / Desktop / Codex / OpenCode / Pi / DeepSeek Harness 的第三方 API、模型映射、导入导出、连接测试、Base URL 测速与模型发现。刷新模型时若 `/v1/models` 返回 404（例如 DeepSeek Anthropic 兼容地址），会回退到宿主根 `GET /models`
- **Claude Code / Codex 路由模式**（默认独立供应商）：开关在对应 Agent 下一级。可切 **统一模型目录**，保存后全部可见模型经本地代理合并，CLI `/model` 选用；目录模式不写 Custom `*_NAME`，角色 id 保持稳定。Code / Codex 可分别指定目录级子代理（Code 写入 `CLAUDE_CODE_SUBAGENT_MODEL`，Codex 代理改写 `x-openai-subagent`），并可 **隐藏官方内置模型**（luna/sol/terra、gpt-5.4* 等建议不进目录；客户端仍请求时改写成子代理或当前默认）。Codex 目录下 AG/上游 429 会切到下一供应商并改写模型，不再拿 Gemini id 去问 OpenAI 中转。供应商可隐藏不进 `/model` 的型号（默认模型不可藏）。Codex 目录下 Chat 中转会把 `/v1/responses` 转成 `/v1/chat/completions`（若 Responses 遇到 400 `Unsupported content type` 亦自动回退为 Chat 格式重试）。OpenCode / Pi / DeepSeek Harness 仍为原生多供应商并存
- 供应商卡片只显示**默认模型**，其余进入目录的型号以 `+N` 展示
- **Thinking / Reasoning 统一参数转译**：供应商可视化配置思考模式、Token 预算（`budget_tokens` / `thinking_budget`）与推理强度（`reasoning_effort`），自动抹平 Anthropic、OpenAI、Gemini 与 DeepSeek（`reasoning_content` 转思考块）协议差异
- 供应商卡片可 **复制到其他 Agent**；供应商页支持 **从其他 Agent 导入**（字段按目标协议转换）
- 供应商页可 **诊断测速** 各节点，并 **一键隔离** 401/403 失效节点（隔离后不再作为故障切换备选）
- Codex 供应商可开关 catalog **Web Search**（写入模型目录 `supports_search_tool` / `web_search_tool_type`）
- 环境页可设置全局顶层 `web_search`：`disabled | cached | indexed | live`（与 catalog 开关层次不同；不写已弃用 `features.web_search*`）
- 一键切换并备份；可恢复官方登录配置
- Claude 侧可用 **ChatGPT 订阅**（经本地代理）；Codex 官方账号用终端 `codex login`
- Codex 写入 `~/.codex/config.toml`；OpenAI 兼容上游可直连，Anthropic / OAuth 仍经本地代理
- Deep Link：`ai-switcher://v1/import?resource=provider|mcp&payload=...`（导入前预览确认）



### 本地代理

Anthropic Messages 兼容转发、模型映射、密钥注入、流式请求、状态与日志。支持**透明故障切换 (Failover)**：在上游遭遇 429、5xx 或连接超时异常时，自动按优先级与模型白名单降级至备用供应商重试，并全程记录降级链路诊断到日志面板。**经本地代理的会话可热切换上游**；直连非代理场景仍可能需重启 CLI。

入口在 **设置 → 工具与环境 → 本地代理**（改端口 / 强制重启 / 故障切换）；日常换供应商时由供应商页自动匹配启停。`openai_responses` 多轮对话的 assistant 历史使用 `output_text`（避免第二轮起 502）。

### Antigravity 网关

内建本地反代（默认 `http://127.0.0.1:15830`），把 Google / Antigravity（Cloud Code）包装成 Agent 可用接口，供 Claude Code、Claude Desktop、Codex、Pi、DeepSeek Harness 使用：

- **协议**：Anthropic `/v1/messages`、OpenAI Chat `/v1/chat/completions`、OpenAI Responses `/v1/responses`（Codex 须绑定 `openai_responses`）
- **账号池与智能调度**：浏览器 OAuth 导入，403 异常熔断与 429 阶梯冷却探针；基于实时剩余配额比例与健康度提供**动态加权轮询与最优账号推荐**；配额不足 (<15%) 自动预警提示；**设为活跃**为软偏好——活跃账号若触达 RPM 压力会让位给更健康的账号（并清会话粘滞与冷却）；后台自动刷新额度与实时模型目录。新账号若项目「待获取」会自动 `onboardUser`，5h/7d 额度条依赖 Cloud Code 项目 ID
- **Claude Desktop 429**：账号池耗尽时向客户端回传 **429 + Retry-After**（不再伪装成 502），避免 Desktop 把网关故障当 5xx 猛重试；Desktop / 独立供应商模式仍不把 AG 429 切到其他供应商。**Codex 统一目录**下会切到下一目录供应商，并把模型改成接管方默认或子代理
- **用量回传**：解析 Gemini `usageMetadata`（含思考 token），在 Anthropic / OpenAI Chat / Responses 流式结束帧带回真实 input/output
- **流式中文**：SSE 按完整行再解码 UTF-8，避免 TCP 半截汉字变成乱码
- **KaTeX 展开**：Gemini 可见正文与 Write/Edit 写盘参数把 `$\\le 50\\text{g}$` 一类公式转成 `≤ 50g`；Edit 的匹配原文与 Grep 模式保持原样
- **Claude Code 子代理**：Explore / Haiku 走目录子代理槽（优先 `gemini-3.7-flash-low`），不再误用当前 `/model` 默认；`thinking: disabled` 不改写已选 Gemini 后缀，也不把 `-low` 粘到主会话
- **模型目录与调度**：Gemini **3.6 与 3.7 并存**（Cloud Code 真实 id 为 `-high` / `-medium` / `-low`）；同账号 429 时按档位链降级（`low` 不再抬升到主会话 `high`，改为同级跨代兄弟如 `3.7-flash-low` → `3.6-flash-low`，单次请求最多 2 档）；区分 URL 级与账号 RPM 型 429，daily 主机限流带 TTL 记忆、烫手时跳过 daily，每请求最多一次 daily→prod 端点 Fallback；子代理经 `x-cs-subagent` 走独立并发池，非流式/流式重试各有截止时间（约 8s / 15s），超时立即 429 + Retry-After。每账号令牌桶（默认约 30 RPM、突发 8）+ 最小间隔退避 + 429 AIMD；**账号与额度**页可手动调节并发与限速
- **一键绑定**：在「账号与额度」页确保供应商后即可在各工具切换使用
- **用量**：网关请求写入 `proxy_request_logs`（`target_app=antigravity`）
- **说明**：个人自用网关，请自行评估账号与上游服务条款；勿用于商业中转



### OpenCode

读写 `~/.config/opencode/opencode.json`（CLI 与 Desktop 共享），多供应商并存、保存即同步，无需切换：

- **供应商同步**：保存/删除/导入后写入 `aisw-<id>` 段；OpenCode 内直接选模型；托管模型会写入 `limit.context`（默认 200000）与 `limit.output`（默认 32000），避免 OpenCode 报 ConfigInvalidError
- **从本地导入**：工作台/供应商页「更新本地已有配置」从 `opencode.json(c)` 批量同步（跳过托管项与 Desktop 内置连接器）
- **会话与用量**：扫描 `opencode.db`；**设置 → Agent 工具** 检测/安装/更新 OpenCode CLI（需 Node.js ≥22）



### Pi

读写 `~/.pi/agent/models.json` 与 `auth.json`。与 OpenCode 相同：多供应商并存、保存即同步，无需切换：

- **供应商同步**：保存/删除/导入后写入全部已启用 Pi 供应商；Pi 内直接选模型
- **直连上游**：OpenAI 兼容（含 Responses）走供应商 Base URL，不经本地代理（Pi 内建代理仅有 `/v1/messages`，Responses 会 404）；Anthropic 可用 Antigravity 网关
- **工作区**：Prompts（`~/.pi/agent/AGENTS.md`）、Skills、MCP（`~/.pi/agent/mcp.json`，需 pi-mcp-adapter / extension）；不接 Plugins、Agents、Profiles
- **会话与用量**：扫描 `~/.pi/agent/sessions/**/*.jsonl` 的 `message.usage`；用量页刷新会同步（与 Antigravity 网关已记账的回合去重）
- **Agent 工具**：检测/安装/更新 Pi CLI（需 Node.js ≥22）



### DeepSeek Harness

读写 `~/.dsh/settings.yaml` 与 `~/.dsh/.credentials.yaml`。与 OpenCode / Pi 相同：多供应商并存、保存即同步，无需切换：

- **供应商同步**：保存/删除/导入后写入托管段；Harness 界面或 CLI 内直接选模型
- **直连上游**：按 YAML 写入各端点，不经本地代理；Anthropic 可用 Antigravity 网关
- **工作区**：Prompts、MCP；不接 Plugins、Agents、Profiles、Skills
- **会话与用量**：扫描 `~/.dsh/sessions/**/*.jsonl.zstd`；用量页刷新会同步
- **Agent 工具**：检测/安装/更新 DeepSeek Harness CLI（需 Node.js ≥22）工作区可一键启动网页端



### Agent 工具

在 **设置 → 工具与环境 → Agent 工具** 中统一检测与安装：

- **Node.js 环境**（本机 ≥22，可用 fnm + 国内镜像安装）
- Claude Code / Codex / OpenCode / **Pi** / **DeepSeek Harness** CLI 安装与更新（npm 全局；兼容 npm 11 与 Windows `%APPDATA%\npm` 落点）

关于页仅保留本应用版本、更新检查、更新镜像与引导提示恢复。

### MCP / Prompts / Skills / Agents / Plugins

- MCP：统一维护并可同步到 Codex / Pi；支持远程 HTTP/SSE、OAuth 状态清理，以及 Desktop Connectors / `.mcpb` 冲突提示
- MCP Registry：浏览官方 Registry 并安装可安全转换为 Claude 配置的条目（需密钥/URL 模板的仍需手动配置）
- Prompts：`CLAUDE.md` / Codex `AGENTS.md` / Pi `~/.pi/agent/AGENTS.md` 预设，支持重命名与一键激活；可编辑当前工作区的项目级 Prompt；Pi 可管理 `~/.pi/agent/prompts/` 模板
- Skills：Claude Code、Codex 与 Pi 支持 GitHub / ZIP 安装、启停、更新与删除；可添加/移除多个 Skill 仓库并进仓挑选安装到对应 Agent；可扫描散落 Skill 一键登记/忽略。「检查全部更新」按仓库合并下载，单条失败不中止整批，也不阻塞窗口
- Agents：管理 Claude Code 用户级 `~/.claude/agents`
- **插件**：工作区单一「插件」Tab，页内切换 Claude Code / Codex；市场列表、安装目录、启停、卸载、检查/更新市场与插件。检查更新在后台执行并有超时，避免 Windows「未响应」



### 项目（Profiles）

为 Claude Code / Desktop / Codex 分别快照供应商、MCP、Skills、Prompt 选择；可一键应用与重命名。

### 会话

浏览、筛选、搜索 Claude Code、Codex、OpenCode、Pi 与 DeepSeek Harness 本地会话；支持导出 / 导入 / 备份 / 回收站（OpenCode 暂不支持归档/导出/回收站）。不解析 Claude Desktop 私有历史格式。

### 中文化

Claude Code 插件、编辑器补丁助手、Claude Desktop 语言包分区管理；编辑器补丁始终需在编辑器内确认。安装中文时会规范化错误的 `spinnerVerbs` 数组格式。

### 用量、环境与系统

- 用量：合并代理日志与 Codex / Claude Code / OpenCode / Pi / DeepSeek Harness 本地会话事件（含 Anthropic 兼容第三方直连、Pi JSONL 与 DSH `jsonl.zstd` 回填）；支持多币种预估；识别 Opus / Codex Fast tier（`*-fast`）；页内数据源过滤；时间范围选择在工具栏右侧
- 环境：配置路径、资料库迁移 / 便携导出、WSL·SSH 同步、**doctor 诊断与一键可见性修复**（不强制改写直连 `ANTHROPIC_BASE_URL`）
- 托盘快捷切换、中英界面、浅色 / 深色 / 跟随系统、开机自启
- 关于页「检查更新」与标题栏共用同一更新弹窗

---



## 会话说明


| 来源               | 路径                                                           |
| ---------------- | ------------------------------------------------------------ |
| Claude Code      | `~/.claude/projects/**/*.jsonl`                              |
| Codex            | `$CODEX_HOME/sessions/**/*.jsonl`（默认 `~/.codex/sessions/`）   |
| OpenCode         | `~/.local/share/opencode/opencode.db`（及 legacy JSON storage） |
| Pi               | `~/.pi/agent/sessions/**/*.jsonl`                            |
| DeepSeek Harness | `~/.dsh/sessions/**/*.jsonl.zstd`                            |


列表只读元数据；打开详情或全文搜索时才读消息。路径限制在会话根目录内。浏览不改原文件。

Claude Desktop 仅检测数据目录并提供官方入口 `claude://claude.ai/new`；已知会话 ID 可用 [官方深链](https://support.claude.com/en/articles/14729294-open-claude-desktop-with-a-link)。

---



## 数据与配置


| 路径                                               | 用途                                                  |
| ------------------------------------------------ | --------------------------------------------------- |
| `~/.claude/settings.json`                        | Claude Code 当前供应商                                   |
| `~/.claude.json`                                 | Claude Code MCP / 项目配置                              |
| `~/.claude/projects/`                            | Claude Code 会话                                      |
| `~/.claude/skills/`                              | Claude Code Skills                                  |
| `%LOCALAPPDATA%\Claude-3p\configLibrary\`        | Claude Desktop 第三方配置（Windows）                       |
| `$CODEX_HOME` 或 `~/.codex/`                      | Codex 配置、会话、Skills、Plugins                          |
| `~/.config/opencode/opencode.json`               | OpenCode 供应商（CLI 与 Desktop 共享；亦支持 `opencode.jsonc`） |
| `~/.local/share/opencode/`                       | OpenCode 会话数据库                                      |
| `~/.pi/agent/models.json` / `auth.json`          | Pi 供应商与凭据                                           |
| `~/.pi/agent/sessions/`                          | Pi 会话 JSONL                                         |
| `~/.pi/agent/AGENTS.md` / `skills/` / `mcp.json` | Pi Prompts / Skills / MCP                           |
| `~/.dsh/settings.yaml` / `.credentials.yaml`     | DeepSeek Harness 供应商与凭据                             |
| `~/.dsh/sessions/`                               | DeepSeek Harness 会话（`jsonl.zstd`）                   |
| `~/.claude/agents/`                              | Claude Code Agents                                  |
| `~/.claude-switcher/`（可改）                        | 本应用资料库：数据库、备份、日志                                    |


产品名已改为 AI-Switcher，仍保留原应用标识与默认资料库路径以兼容旧用户。资料库可迁到其他盘（SHA-256 校验，重启生效）。导出 / 同步默认不含 API Key。

---



## 安全与隐私

- API Key：Windows Credential Manager / macOS Keychain / Linux Secret Service
- 配置：原子写入 + 轮换备份
- 会话：不建全文库；导入导出与回收站校验根目录与符号链接
- 除连接测试、模型发现、更新检查、用户主动下载与确认的远端归档推送外，不上传本地内容

---



## 从源码开发

需要：Node.js 22+、pnpm 9+（可用 Corepack）、Rust stable。Windows 还需 VS 2022 C++ 桌面开发组件。

```powershell
pnpm install
pnpm tauri dev
# 无 MSVC 环境变量时：
scripts\tauri-msvc.bat dev
```

开发服务器端口为 **5250**（与 `tauri.conf.json` `devUrl` 一致）。更稳的热加载：`.\scripts\dev-hot.ps1`（或 `pnpm dev:hot`）。若 5250 被占用，脚本会临时改用 5251+ 并在编完还原配置。

### 构建（Windows）

脚本默认先跑完整 Rust 测试：

```powershell
pnpm build:exe              # 正式版 exe → release\AISwitcher.exe
pnpm build:exe:debug        # 调试版 → release\AISwitcher-debug.exe
pnpm build:exe:bundle       # MSI + NSIS
scripts\build-exe.bat release skip-tests   # 跳过测试
```


| 产物   | 路径                                                |
| ---- | ------------------------------------------------- |
| 正式版  | `src-tauri\target\release\AISwitcher.exe`         |
| 测试副本 | `release\AISwitcher.exe` / `AISwitcher-debug.exe` |
| 安装包  | `src-tauri\target\release\bundle\`                |


---



## 项目结构

```text
src/                      React + Ant Design + Zustand + i18next
  pages/                  业务页面（Workbench / Providers / Usage / …）
  components/layout/      左侧导航壳（AppShell / SideNav / …）
  components/v2/shell/    顶部导航壳（DesktopShell / TopNavigation）
  lib/pageRegistry.ts     页面键与懒加载
src-tauri/src/            Rust：配置、代理、Antigravity、数据库、托盘、会话
  antigravity/            AG 网关（默认 :15830）
  proxy/                  本地 Anthropic 兼容代理
  config/                 Claude / Codex / OpenCode / DSH 配置读写
  coding/pi/              Pi models.json / auth / MCP / 会话用量
  database/               SQLite（user_version）
scripts/                  Windows 开发 / 构建脚本
```

---



## 当前边界

- 产品客户端范围：Claude Code + Claude Desktop + Codex + OpenCode + Pi + DeepSeek Harness；Antigravity 网关可把 Gemini / Cloud Code 上游接到上述客户端
- Pi 不接 Plugins / Agents / Profiles / 托盘切换；Pi 的 OpenAI 兼容上游直连，不经本地代理
- DeepSeek Harness 不接 Plugins / Agents / Profiles / Skills / 托盘切换；供应商写入 YAML 后直连上游，不经本地代理
- 插件页管理本机已安装市场与插件，不是完整替代官方 CLI 商店浏览体验
- 会话「恢复」只复制命令，不自动开终端
- 不同步自动合并远端冲突，不做团队分享
- Claude Code 与 Desktop 的供应商列表与激活状态始终独立
- 不解析 Claude Desktop 私有会话格式
- Antigravity 双账号额度耗尽时上游仍可能 429（网关会降级档位并轮换，无法凭空扩额）
- Linux 预览包面向 Ubuntu 22.04 / Debian 12+；Ubuntu 18.04 因缺少 WebKitGTK 4.1 且 glibc 过旧，无法提供独立构建

---



## 参考与致谢

独立项目，与下列仓库及 Anthropic / OpenAI 均无隶属关系。表中许可证指**上游仓库自身**的许可；AI-Switcher 源码仍以本仓库 [MIT](LICENSE) 为准。引用或移植上游代码时，请同时遵守其许可证与版权声明。


| 项目                      | 参考方向                                   | 上游                                                                               |
| ----------------------- | -------------------------------------- | -------------------------------------------------------------------------------- |
| AI Toolbox              | 多工具配置、会话与桌面信息架构                        | [coulsontl/ai-toolbox](https://github.com/coulsontl/ai-toolbox) MIT              |
| Antigravity-Manager     | Antigravity / Cloud Code 反代、账号池与协议映射思路 | [lbjlaq/Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager)      |
| sub2api                 | Antigravity URL 级限流识别与端点重试机制           | [sub2api](https://github.com/sub2api)                                            |
| CLIProxyAPI             | 多协议网关与上游适配思路                           | [router-for-me/CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)        |
| cc Proxy                | Desktop 本地代理与模型替换                      | [arhsis/cc-proxy](https://github.com/arhsis/cc-proxy)                            |
| CC Switch               | 供应商切换、Tauri、会话与托盘                      | [farion1231/cc-switch](https://github.com/farion1231/cc-switch) MIT              |
| Claude Code VS Code 中文包 | 扩展定位与汉化流程                              | [zstings/claude-code-zh-cn](https://github.com/zstings/claude-code-zh-cn) MIT    |
| Claude Code 中文插件        | CLI 中文化安装与恢复                           | [taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn)      |
| Claude Desktop 中文补丁     | 安装发现与语言包                               | [javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn)    |
| Code Switch             | 本地代理、故障切换、Codex 配置                     | [daodao97/code-swtich](https://github.com/daodao97/code-swtich) Apache-2.0       |
| Codex++                 | Codex API 写入与历史会话同步                    | [BigPizzaV3/CodexPlusPlus](https://github.com/BigPizzaV3/CodexPlusPlus) AGPL-3.0 |


