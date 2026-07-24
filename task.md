# Claude Desktop / Claude Code 第三方 API 配置工具 — 规划

> 目标：开发一款桌面软件，统一管理 Claude Desktop 和 Claude Code 的第三方 API 配置，可视化添加供应商、一键切换，无需手动编辑配置文件。
>
> 技术栈：**Tauri 2 + React 19 + TypeScript**（与 cc-switch / ai-toolbox 同栈，示例代码可直接参考）
> 范围：全功能版，**不含**云同步、多供应商故障转移

---

## 一、examples 目录四个参考工具的功能分析

### 1. cc-switch（Tauri 2 + React + Rust，功能最全，主要参考对象）

- **供应商管理**：支持 8 种 AI 工具（Claude Code、Claude Desktop、Codex、Gemini CLI 等），50+ 内置供应商预设，一键导入当前配置、切换、拖拽排序、导入导出
- **Claude Code 切换原理**：写 `~/.claude/settings.json` 的 `env.ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_MODEL`，热切换无需重启
- **Claude Desktop 切换原理**：支持「本地代理模式」和「直连模式」（见 `src-tauri/src/claude_desktop_config.rs`）
- **本地代理**：格式转换、熔断器、供应商健康监控、按应用（Claude/Codex/Gemini）独立代理接管
- **MCP / Prompts / Skills 统一管理**：统一面板 + 双向同步；Prompts 用 Markdown 编辑器同步到 CLAUDE.md；Skills 从 GitHub/ZIP 一键安装
- **用量与成本追踪**：跨供应商支出/请求数/Token 统计、趋势图、请求日志、自定义模型定价
- **系统能力**：托盘快速切换、Deep Link（`ccswitch://`）导入、深浅色主题、开机自启、自动更新、SQLite 存储 + 原子写入 + 自动备份轮换

### 2. cc-proxy（Python，最小可用实现，Claude Desktop 机制的最佳参考）

- 专做 **Claude Desktop → 第三方 API** 的本地代理，代码量极小，适合理解核心机制
- **Claude Desktop 配置机制**（`claude_config.py`）：
  - 配置目录：Windows 为 `%LOCALAPPDATA%\Claude\configLibrary\`，macOS 为 `~/Library/Application Support/Claude/configLibrary\`
  - 写入一个 `<id>.json`：`{ inferenceProvider: "gateway", inferenceGatewayBaseUrl: "http://127.0.0.1:<port>", inferenceGatewayApiKey, inferenceModels: [{name, supports1m}], disableDeploymentModeChooser: true }`
  - 在 `_meta.json` 中登记 entry 并设置 `appliedId` 使其生效
- **代理机制**（`proxy.py`）：本地 HTTP 服务接收 Claude Desktop 的 Anthropic 格式请求 → 改写 body 中的 `model` 字段 → 替换为真实 API Key → 转发到第三方 API → 透传响应（含流式）
- 供应商增删改、端口可配、SQLite 存储

### 3. ai-toolbox（Tauri 2 + React 19 + Ant Design 6 + Zustand，工程结构参考）

- 与 cc-switch 功能高度重叠（供应商管理、MCP、Skills、会话管理、本机代理网关、用量统计、托盘、备份）
- 独有能力（本项目不做）：WSL 同步、SSH 同步、Image 工作台
- **前端工程结构清晰**（`web/features/` 按业务模块划分），可作为本项目目录结构模板
- 技术栈参考：Ant Design 6 + Zustand + i18next + SQLite + Vite + pnpm

### 4. code-switch（Wails 3 + Go，代理架构参考）

- **统一本地代理架构**：启动时在 `:18100` 起 HTTP 代理，自动把 Claude Code / Codex 配置指向 `http://127.0.0.1:18100`，CLI 只看到固定本地地址，真实请求按当前激活供应商透明路由
- 只暴露关键端点（`/v1/messages` → Claude 供应商，`/responses` → Codex 供应商）
- 切换供应商 = 改代理内部路由目标，**完全不用改 CLI 配置、不用重启**——这是最优雅的切换方式
- 请求级用量统计、MCP 双平台管理、Skill 仓库安装

### 机制总结（两种配置 Claude Desktop 的方式）

| 方式 | 原理 | 优点 | 缺点 |
|------|------|------|------|
| **本地代理**（cc-proxy / cc-switch 代理模式） | configLibrary 指向 `127.0.0.1:port`，代理改写模型名 + 注入 Key 转发 | 支持只兼容 OpenAI/自定义协议的第三方；可做用量统计 | 代理必须常驻运行 |
| **直连**（cc-switch 直连模式） | configLibrary 直接填第三方 Anthropic 兼容地址 + Key | 无需代理常驻 | 第三方必须完整兼容 Anthropic 协议且模型名匹配 |

Claude Code 同理：settings.json 可直接填第三方地址，也可指向本地代理。

---

## 二、本项目功能定义（V1 全功能版）

### 核心功能

1. **供应商管理**
   - 供应商 CRUD：名称、API 地址、API Key、模型名、备注、协议类型（Anthropic 兼容 / 需代理转换）
   - 内置常用第三方预设（Kimi、DeepSeek、小米 MiMo、智谱等，持续扩充）
   - 首次启动自动导入 Claude Code / Claude Desktop 当前配置作为默认供应商
   - 一键启用/切换、拖拽排序、配置导入导出（JSON）
2. **Claude Code 配置切换**
   - 写 `~/.claude/settings.json`（`env.ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_MODEL`）
   - 支持「直连第三方」和「走本地代理」两种模式
   - 切换前自动备份原配置；保留官方登录供应商预设
3. **Claude Desktop 配置切换**
   - 写 `%LOCALAPPDATA%\Claude\configLibrary\` 的 gateway 配置 + `_meta.json` appliedId（参考 cc-proxy）
   - 支持直连模式（Anthropic 兼容供应商）和代理模式
   - 路径自动探测（Claude / ClaudeZhCN 等候选目录）
4. **本地代理（Rust 实现，随应用启动）**
   - 接收 Anthropic `/v1/messages` 请求 → 模型名映射 → 注入真实 Key → 转发 → 透传流式响应
   - 端口可配置、启停控制、运行状态指示
   - 请求日志（时间、供应商、模型、状态码、Token 用量）
5. **MCP 服务器管理**
   - 统一面板管理 Claude Code（`~/.claude.json` 的 `mcpServers`）和 Claude Desktop（`claude_desktop_config.json` 的 `mcpServers`）
   - 双向同步：从现有配置导入，编辑后写回各应用
6. **Prompts 管理**
   - Markdown 编辑器管理 CLAUDE.md 预设，一键激活写入 live 文件，回填保护
7. **Skills 管理**
   - 从 GitHub 仓库 / ZIP 一键安装 Skill，按应用启停
8. **用量统计仪表盘**
   - 基于代理请求日志：按供应商/模型统计请求数、Token 用量、趋势图表
   - 自定义模型定价，估算成本
9. **系统能力**
   - 系统托盘快速切换供应商（无需开主窗口）
   - 深色/浅色/跟随系统主题、中英文界面、开机自启
   - SQLite 存储、原子写入、自动备份轮换（保留最近 10 份）

### 明确不做（V1）

- 云同步（WebDAV / Dropbox 等）
- 多供应商自动故障转移 / 熔断
- Claude Desktop / Claude Code 以外的工具（Codex、Gemini 等，架构预留扩展点）
- Deep Link 导入、会话管理器

---

## 三、架构设计

```
┌─────────────────────────────────────────────┐
│  React 前端（Ant Design 6 + Zustand + i18next）│
│  供应商管理 │ MCP │ Prompts │ Skills │ 用量   │
└──────────────────┬──────────────────────────┘
                   │ Tauri IPC (commands)
┌──────────────────┴──────────────────────────┐
│  Rust 后端                                   │
│  ├─ config/   配置读写（原子写入 + 备份）     │
│  │   ├─ claude_code.rs    settings.json     │
│  │   └─ claude_desktop.rs configLibrary     │
│  ├─ proxy/    本地 HTTP 代理（axum/tokio）   │
│  │   ├─ 模型名映射、Key 注入、流式透传        │
│  │   └─ 请求日志 → SQLite                   │
│  ├─ mcp/ prompts/ skills/  各应用文件同步    │
│  ├─ db/       SQLite（供应商、日志、设置）   │
│  └─ tray/     系统托盘快速切换               │
└─────────────────────────────────────────────┘
```

### 关键设计决策

1. **切换方式：配置文件直写 + 可选本地代理**
   - Claude Code 默认直写 settings.json（热切换，简单可靠）
   - Claude Desktop 默认走本地代理（兼容性最好）；Anthropic 兼容的供应商可选直连模式
   - 不采用 code-switch 的「强制全部走代理」方案，避免代理成为单点依赖
2. **数据存储**：`~/.<app-name>/app.db`（SQLite）+ `settings.json`（设备级 UI 偏好）+ `backups/` 自动轮换 — 直接沿用 cc-switch 的存储布局
3. **写入安全**：所有配置文件写入采用「写临时文件 → 原子 rename」，写前自动备份
4. **前端结构**：参照 ai-toolbox 的 `web/features/` 按业务模块划分

---

## 四、开发阶段规划

| 阶段 | 内容 | 产出 |
|------|------|------|
| **P0 脚手架** | Tauri 2 + React + Vite + AntD 工程初始化；SQLite 层；配置目录探测；原子写入 + 备份工具 | 可运行的空壳 + 基础库 |
| **P1 供应商 + Claude Code** | 供应商 CRUD + 预设；settings.json 读写切换；导入现有配置 | Claude Code 一键切换可用 |
| **P2 Claude Desktop + 代理** | configLibrary 写入/生效；Rust 本地代理（模型映射、Key 注入、流式转发）；端口管理 | Claude Desktop 走通第三方 API |
| **P3 MCP + Prompts** | MCP 统一面板 + 双向同步；Prompts Markdown 编辑器 + 激活 | 配置生态完整 |
| **P4 用量统计** | 代理请求日志落库；仪表盘（请求数/Token/趋势/成本）；自定义定价 | 用量可视化 |
| **P5 Skills + 收尾** | Skills 安装/启停；托盘快速切换；主题/i18n/自启；Windows 打包（msi/exe） | V1 发布 |

### 各阶段参考代码

- P1：`examples/cc-switch-main/src-tauri/src/`（settings.json 处理、供应商模型）
- P2：`examples/cc-proxy-master/claude_config.py` + `proxy.py`（机制最简实现，用 Rust 重写）；`cc-switch/src-tauri/src/claude_desktop_config.rs`（直连模式校验逻辑）
- P3/P5：`examples/cc-switch-main/`（MCP 同步、托盘）；`examples/ai-toolbox-main/web/`（前端模块结构）
- 代理架构取舍：`examples/code-switch-main/`（统一代理思路，仅借鉴不照搬）

---

## 五、待确认事项

- [ ] 应用名称（决定 `~/.<app-name>` 数据目录和包名）
- [ ] UI 组件库最终选型：Ant Design 6（推荐，表单/表格类界面开发快）vs shadcn/ui + Tailwind
- [ ] 内置供应商预设清单（第一版内置哪几家）
- [ ] 是否需要 macOS 打包（当前开发环境为 Windows，可先只做 Windows）
