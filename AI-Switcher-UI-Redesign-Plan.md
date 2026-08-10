# AI-Switcher 新 UI 重构规划（详细版 / AI Coding Agent 执行稿）

> 文档用途：作为 Claude Code / Codex / Cursor / OpenCode 等 AI Coding Agent 的长期 UI 重构上下文与实施基线。  
> 目标：不是一次性“换皮”，而是在**不破坏现有代理、供应商切换、统计、账号、配置能力**的前提下，重构 AI-Switcher 的信息架构、视觉系统、组件系统和交互模型，使后续功能扩展不再持续堆叠页面。

---

## 0. 执行原则

### 0.1 核心原则

1. **先梳理信息架构，再改视觉。** 不允许只修改颜色、圆角和间距，却继续保留当前分散的页面关系。
2. **业务逻辑与 UI 解耦。** Tauri command、代理服务、供应商配置、账号认证、统计数据获取等现有逻辑优先复用，不在 UI 重构阶段随意重写。
3. **渐进式迁移。** 新旧页面可短期并存，按模块迁移，避免一次大改导致核心代理能力不可用。
4. **状态必须统一。** “当前客户端 / 当前供应商 / 代理运行状态 / 端口 / 当前账号 / 当前项目”等全局状态只允许存在一个可信来源。
5. **桌面应用优先。** 这是 Tauri Desktop App，不照搬传统后台管理系统；减少网页式大面积空白、超长表单和重复导航。
6. **高频操作前置。** 切换供应商、查看代理状态、测速、启动/停止代理、查看异常，是一级操作；高级配置才进入设置页。
7. **危险操作明确。** 删除供应商、删除账号、覆盖配置、重置统计等必须使用确认流程，不能与普通图标按钮视觉等价。
8. **每个阶段必须可运行。** Agent 每完成一个阶段都应执行 lint / typecheck / build，并手工验证关键路径。

### 0.2 非目标

本轮 UI 重构默认**不包含**：

- 重写代理协议转换核心；
- 改变已有配置文件格式；
- 大规模修改 Rust 后端 API；
- 重做 OAuth / Antigravity 登录机制；
- 引入重量级企业级 Design System；
- 为追求动画而加入复杂动画框架；
- 在没有必要时迁移现有状态管理库。

如果实施过程中发现必须修改上述内容，应先输出影响分析，再单独实施。

---

# 1. 当前 UI 问题诊断

根据现有界面，当前功能已经比较完整，但信息架构开始出现明显的“功能持续追加”痕迹。

## 1.1 首页 / 供应商服务页

当前页面同时承担：

- Claude Code / Claude Desktop / Codex / OpenCode 客户端切换；
- 代理运行状态；
- 代理端口与 AG 网关端口；
- 供应商卡片；
- 当前供应商标识；
- 延迟；
- 切换、测速、编辑、复制、删除；
- 新增供应商；
- 导入/导出；
- 进入用量统计。

问题：

- 顶部存在过多并列状态，视觉层级不明确；
- “客户端选择”和“供应商选择”虽然是两个维度，但视觉上过于接近；
- 卡片面积较大，而有效信息只有名称、模型、协议、延迟、Endpoint；
- 卡片右下角多个纯图标操作缺少语义层级；
- 当前供应商主要依赖蓝色边框 + “当前”标签，状态表达仍可加强；
- 页面底部悬浮式“供应商服务 / 用量与统计”切换与顶部客户端导航、设置导航形成第三套导航模型；
- 大屏下页面存在明显空白，信息密度不均衡。

## 1.2 用量统计页

已有：预计成本、Token、请求数、成功率、年度热力图、按小时统计。

问题：

- 顶部指标卡与下面图表关联弱；
- 年度热力图占宽较大，但缺少时间范围、客户端、供应商、模型等过滤条件；
- 图表区域之后存在大量空白；
- 缺少“最近请求 / 错误请求 / 模型占比 / 供应商占比”等可解释统计；
- 预计成本需要明确“估算依据”，否则容易被误解为账单；
- 统计页应从单纯展示升级为诊断工具。

## 1.3 运行状态 / 代理控制页

已有：状态、端口、目标供应商、代理端点、自动故障切换、HTTP 状态码、流式超时、启动/停止。

问题：

- 这是非常重要的运行控制中心，但当前视觉形态像普通设置表单；
- 状态信息与可修改配置混在一起；
- 启动/停止代理属于核心动作，却放在页面底部；
- 自动故障切换是高级能力，缺少结构化解释和候选供应商预览；
- 当前供应商与首页的当前供应商状态应完全同步；
- 缺少最近错误 / 请求状态 / 健康检查结果。

## 1.4 Antigravity / 账号页面

已有：模型说明、Google OAuth 登录、账号列表、额度、健康度、项目状态、激活、删除、JSON 导入。

问题：

- 大段说明文字占据顶部空间；
- 多种模型额度压缩在单个表格单元格中，可读性差；
- “健康度 100%”与额度条并列，但含义没有被清晰区分；
- JSON 导入属于高级/兼容功能，不应持续占据主界面；
- 账号、模型额度、OAuth 操作可以形成更清晰的卡片/详情抽屉结构。

## 1.5 设置页

当前左侧包含：项目、MCP、Prompts、Skills、Agents、Codex 插件、会话管理、中文化配置、环境信息、关于。

问题：

- “设置”实际上已经演变成第二个产品主导航；
- 项目、MCP、Prompts、Skills、Agents 并不是传统意义的设置，而是资源管理能力；
- “环境信息 / 关于”才更接近系统设置；
- 未来继续增加功能会让设置侧栏越来越长；
- 主界面和设置界面采用不同导航方式，整体产品感不统一。

---

# 2. 新产品信息架构

建议从“页面堆叠”重构为 **Workspace Shell + 一级模块 + 上下文详情**。

## 2.1 一级导航

建议统一为左侧窄侧栏：

```text
AI-Switcher

[概览]       Dashboard
[供应商]     Providers
[代理]       Proxy
[用量]       Usage

────────────

[账号]       Accounts
[工作区]     Workspace
  - Projects
  - MCP
  - Prompts
  - Skills
  - Agents
  - Codex Plugins
  - Sessions

────────────

[设置]       Settings
```

### 为什么这样拆

- **概览**：回答“现在是否正常、正在使用什么、有没有错误”。
- **供应商**：回答“有哪些 API Provider、当前选哪个、怎么管理”。
- **代理**：回答“本地代理是否运行、监听什么端口、故障切换如何工作”。
- **用量**：回答“用了多少、是否稳定、成本大概多少”。
- **账号**：集中管理 Antigravity / OAuth / 未来可能的其他账号体系。
- **工作区**：承载 Projects / MCP / Prompt / Skill / Agent 等开发资源。
- **设置**：只保留真正的应用设置。

## 2.2 客户端切换不再作为一级导航

Claude Code / Claude Desktop / Codex / OpenCode 本质上是**当前上下文 Client**，不是四个完全独立的产品页面。

建议放到顶栏：

```text
[Claude Code ▾]    当前供应商: kimi    ● Proxy Running    15821
```

切换 Client 后：

- Provider 当前选择随 Client 更新；
- Proxy 配置上下文更新；
- Usage 默认过滤到该 Client；
- Workspace 中需要 Client Scope 的资源跟随切换；
- URL / route 可保留 `?client=claude-code` 或统一 store。

---

# 3. App Shell 规划

## 3.1 整体布局

```text
┌─────────────────────────────────────────────────────────────┐
│ Title Bar / Drag Region                                     │
├────────────┬────────────────────────────────────────────────┤
│            │ Context Header                                 │
│ Sidebar    ├────────────────────────────────────────────────┤
│            │                                                │
│            │ Main Content                                   │
│            │                                                │
│            │                                                │
│            │                                                │
├────────────┴────────────────────────────────────────────────┤
│ Optional Status Bar                                         │
└─────────────────────────────────────────────────────────────┘
```

推荐尺寸：

- Sidebar expanded：`220px`；
- Sidebar collapsed：`64px`；
- 顶部标题栏：`40–44px`；
- Context Header：`56–64px`；
- 内容最大宽度：不要强制 1200px 居中，桌面宽屏应充分利用；
- 页面 padding：`20–24px`；
- 卡片 gap：`12–16px`。

## 3.2 顶部 Context Header

左侧：

- 当前页面标题；
- 可选 breadcrumb；
- Client Selector。

右侧：

- Proxy 状态胶囊；
- 当前 Provider；
- 快速测速；
- 全局搜索 / Command Palette（后续）；
- 主题；
- 语言；
- 设置。

不要把所有按钮永远显示成文字按钮。低频全局功能进入 `...` 或头像/应用菜单。

## 3.3 状态栏

可保留底部 24px 左右的 Desktop Status Bar：

```text
● Proxy Running   localhost:15821   Provider: kimi   Client: Claude Code
                                                    v1.4.0
```

用途：

- 任何页面都能确认代理是否运行；
- 点击 Proxy 状态跳转 Proxy 页面；
- 点击 Provider 跳转当前 Provider；
- 异常时状态栏变为 warning / danger，但避免整条高饱和红色。

---

# 4. Dashboard / 概览页

这是新 UI 的默认首页。

## 4.1 第一屏

```text
概览                                      [Claude Code ▾]

┌───────────────────────┐ ┌──────────────────────────────┐
│ Proxy                  │ │ Current Provider             │
│ ● Running              │ │ kimi                         │
│ localhost:15821        │ │ k3-256k · Anthropic          │
│ [Restart] [Details]    │ │ 156 ms             [Switch] │
└───────────────────────┘ └──────────────────────────────┘

┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐
│ Requests │ │ Tokens   │ │ Success  │ │ Est.Cost │
│ 1,561    │ │ 72M      │ │ 95.8%    │ │ $67.29   │
└──────────┘ └──────────┘ └──────────┘ └──────────┘
```

## 4.2 第二屏

左侧 2/3：最近 24h 请求趋势。  
右侧 1/3：Provider 健康状态。

Provider Health 示例：

```text
kimi          ● Healthy      156 ms
DeepSeek      ● Healthy      119 ms
sub2api       ● Slow        4450 ms
Antigravity   ● Degraded    1844 ms
```

## 4.3 第三屏

- Recent Errors；
- Recent Switches；
- Failover Events；
- 最近配置变更。

如果当前没有异常，显示紧凑 Empty State，不需要占满大卡片。

---

# 5. Providers / 供应商页

## 5.1 页面目标

只负责：**发现、比较、切换、编辑 Provider**。

## 5.2 Toolbar

```text
供应商
管理当前客户端可用的 API Provider

[搜索供应商...] [协议: 全部 ▾] [状态: 全部 ▾]     [测速全部] [+ 新建]
```

导入 / 导出放入 `More` 菜单：

```text
⋯
- 导入配置
- 导出配置
- 导出全部
```

## 5.3 默认使用 Compact Card / List Hybrid

不要继续使用当前超大卡片。

建议：

```text
┌──────────────────────────────────────────────────────────────────┐
│ [K] kimi                          ● Current             156 ms    │
│ k3-256k    Anthropic                                         ⋯   │
│ api.kimi.com/coding                                             │
└──────────────────────────────────────────────────────────────────┘
```

桌面宽屏可以两列，但卡片高度建议控制在 `110–140px`。

## 5.4 Provider Card 状态

必须支持：

- Current；
- Healthy；
- Slow；
- Error；
- Disabled；
- Testing；
- Unknown。

延迟颜色不要只依赖红绿：同时使用图标 / 文本。

建议阈值可配置：

```ts
< 300ms       healthy
300–1000ms    normal
1000–3000ms   slow
> 3000ms      verySlow
failed        error
```

这只是 UI 默认展示阈值，最终以现有业务规则为准。

## 5.5 Card Actions

高频：

- Switch；
- Test。

低频进入 `...`：

- Edit；
- Duplicate；
- Copy Endpoint；
- Export；
- Delete。

删除必须二次确认。

## 5.6 Provider Detail Drawer

点击卡片不要立即进入复杂独立页面，优先打开右侧 Drawer：

```text
Provider Details

Name             kimi
Protocol         Anthropic
Model            k3-256k
Endpoint         ...
Last latency     156 ms
Last tested      12 sec ago

[Configuration]
API Key          •••••••••••• [Reveal]
Headers          2 custom
Model Mapping    3 rules

[Save Changes]
```

复杂配置可从 Drawer 再进入 Full Editor。

---

# 6. Provider 新建 / 编辑体验

建议使用分组表单，而不是把所有字段平铺。

## 6.1 Basic

- Name；
- Protocol；
- Base URL；
- API Key；
- Default Model。

## 6.2 Advanced

折叠区域：

- Custom Headers；
- Model Mapping；
- Request Rewrite；
- Timeout；
- Proxy；
- Compatibility flags。

## 6.3 实时验证

- URL 格式；
- 必填项；
- 重名；
- Protocol 与模型映射兼容性。

保存前可执行：

`[Test Connection] [Save]`

Test Connection 结果应该展示结构化信息，而不是只 Toast “成功”。

---

# 7. Proxy / 代理控制中心

将当前“运行状态 + 自动故障切换 + 启动停止”重构为真正的控制中心。

## 7.1 Hero Status

```text
Proxy

● Running
Local endpoint  http://127.0.0.1:15821
Target          kimi
Uptime          2h 18m

[Restart Proxy] [Stop]
```

Stopped 时：

```text
○ Stopped
Port 15821
[Start Proxy]
```

## 7.2 Runtime 信息

独立只读区：

- Status；
- Port；
- Target Provider；
- Proxy Endpoint；
- Started At；
- Request Count；
- Last Error。

不要把只读状态伪装成 input。

## 7.3 Configuration

可编辑配置：

- Listen Port；
- Stream Idle Timeout；
- Retry HTTP Status Codes；
- Failover Enabled。

配置修改后出现：

`Unsaved changes` + `[Discard] [Apply & Restart]`

如果某配置无需重启，应显示 `[Apply]`。

## 7.4 Failover

开启后展示：

```text
Failover Chain

1. kimi             Healthy
2. DeepSeek         Healthy
3. Antigravity      Slow

[Edit Priority]
```

支持拖拽排序，但必须同时提供键盘可操作的上移/下移按钮。

明确显示：

- 哪些错误触发切换；
- 单 Provider 熔断时间；
- 当前是否处于熔断；
- 最近一次 failover 原因。

---

# 8. Usage / 用量与统计

## 8.1 页面过滤器

顶部固定 Filter Bar：

```text
[Last 7 days ▾] [Claude Code ▾] [All Providers ▾] [All Models ▾]
```

支持：Today / 24h / 7d / 30d / Custom。

## 8.2 KPI

保留现有 4 项，但强化语义：

- Requests；
- Tokens；
- Success Rate；
- Estimated Cost。

Estimated Cost 旁必须有 tooltip：

> 根据本地记录的模型 Token 与配置价格估算，不代表供应商最终账单。

## 8.3 图表布局

第一行：

- Request / Token Trend（2/3）；
- Success / Error Distribution（1/3）。

第二行：

- Provider Usage；
- Model Usage。

第三行：

- Calendar Heatmap，可折叠；
- Hourly Distribution。

第四行：

Recent Requests Table：

- Time；
- Client；
- Provider；
- Model；
- Input Token；
- Output Token；
- Latency；
- Status。

点击一行打开 Request Detail Drawer。

## 8.4 Request Detail

默认不要展示完整 prompt / response，避免 UI 误泄露敏感内容。

优先展示元数据：

- request id；
- timestamp；
- route；
- model；
- provider；
- latency；
- tokens；
- HTTP status；
- retry / failover history；
- error summary。

如产品已有请求正文存储能力，再提供显式“查看内容”。

---

# 9. Accounts / Antigravity 重构

## 9.1 页面顶部

```text
Accounts
Manage Antigravity / Google accounts and quotas.

[Sign in with Google] [+ Import]
```

长篇说明移动到：

- Info Alert；
- `Learn more` Drawer；
- 首次使用引导。

## 9.2 Account Card

```text
stone3mailbox@gmail.com      PRO     ● Active
Project: OK                         Health 100%

Gemini 5h       ███████░░ 77%
Gemini 7d       █████████ 93%
Claude-GPT 5h   ████████░ 79%
Claude-GPT 7d   █░░░░░░░░  8%

Last refreshed: 1 min ago                         [Manage]
```

多个账号使用纵向卡片或 Data Table + Expand Row，不要把全部额度挤进一个狭窄单元格。

## 9.3 Account Detail Drawer

- Account identity；
- Plan；
- Project；
- Quotas；
- Token refresh status；
- Last refresh；
- Set Active；
- Refresh Quota；
- Re-authenticate；
- Delete。

JSON 导入放在 Import Dialog 的 Advanced Tab。

---

# 10. Workspace 重构

把当前 Settings 中以下模块迁出：

- Projects；
- MCP；
- Prompts；
- Skills；
- Agents；
- Codex Plugins；
- Session Management。

## 10.1 Workspace 首页

```text
Workspace

Projects      3
MCP Servers   8
Prompts       12
Skills        6
Agents        4
Plugins       2
```

显示最近修改资源与当前项目。

## 10.2 Projects

Project Snapshot 应升级为真正的 Profile：

一个 Project 可以保存：

- Client scope；
- Provider selection；
- MCP selection；
- Skills；
- Prompts；
- Agents；
- Plugins；
- 相关环境配置。

支持：

- Create；
- Duplicate；
- Rename；
- Apply；
- Export；
- Delete。

Apply 前展示 Diff Summary，避免静默覆盖当前配置。

---

# 11. Settings 精简

Settings 只保留真正的应用级设置。

建议：

```text
Settings
├─ General
│  ├─ Language
│  ├─ Theme
│  ├─ Start behavior
│  └─ Minimize to tray
├─ Proxy Defaults
├─ Data & Privacy
├─ Localization
├─ Environment
├─ Updates
└─ About
```

如果某设置属于具体 Provider / Client / Project，则不要放进 Global Settings。

---

# 12. 视觉系统

## 12.1 风格方向

关键词：

- Desktop native-like；
- clean；
- compact；
- developer tool；
- neutral；
- information-first；
- subtle color；
- low visual noise。

不要做成：

- SaaS 营销后台；
- 大量渐变；
- 玻璃拟态；
- 每张卡片不同高饱和颜色；
- 超大标题；
- 过度圆角；
- 无意义阴影。

## 12.2 Color Tokens

建议不要在组件中散落 HEX。

```css
:root {
  --bg-app: ...;
  --bg-surface: ...;
  --bg-subtle: ...;
  --bg-hover: ...;
  --border-default: ...;
  --border-strong: ...;

  --text-primary: ...;
  --text-secondary: ...;
  --text-muted: ...;

  --accent: ...;
  --accent-hover: ...;
  --accent-subtle: ...;

  --success: ...;
  --warning: ...;
  --danger: ...;
  --info: ...;
}
```

Light / Dark 使用同一 semantic token，不让业务组件直接判断主题。

## 12.3 圆角

建议：

- Button / Input：6–8px；
- Card：8–10px；
- Dialog：10–12px；
- Pill：999px，仅用于真正的 Badge / Status。

当前页面大量“胶囊”可适当减少。

## 12.4 阴影

普通 Card 主要依赖 Border。  
Shadow 只用于：

- Popover；
- Dialog；
- Drawer；
- Floating Menu。

## 12.5 Typography

建议层级：

```text
Page title        20–24 / semibold
Section title     15–16 / semibold
Body              13–14 / regular
Label             12–13 / medium
Meta              11–12 / regular
Code / Endpoint   12–13 / monospace
```

Endpoint、Port、Model ID、Request ID 使用 monospace。

## 12.6 Spacing

基于 4px：

`4 / 8 / 12 / 16 / 20 / 24 / 32`

避免随机出现 13px、18px、27px 等无系统间距。

---

# 13. 组件系统规划

建议建立：

```text
src/components/
├─ ui/
│  ├─ Button
│  ├─ IconButton
│  ├─ Input
│  ├─ Select
│  ├─ Switch
│  ├─ Badge
│  ├─ StatusBadge
│  ├─ Tooltip
│  ├─ Popover
│  ├─ DropdownMenu
│  ├─ Dialog
│  ├─ Drawer
│  ├─ Tabs
│  ├─ Card
│  ├─ DataTable
│  ├─ EmptyState
│  ├─ Skeleton
│  ├─ Alert
│  └─ Progress
│
├─ layout/
│  ├─ AppShell
│  ├─ Sidebar
│  ├─ TitleBar
│  ├─ ContextHeader
│  ├─ PageContainer
│  └─ StatusBar
│
└─ domain/
   ├─ ClientSelector
   ├─ ProviderCard
   ├─ ProviderStatus
   ├─ ProxyStatus
   ├─ QuotaBar
   ├─ UsageMetric
   └─ RequestStatus
```

原则：

- `ui/` 不知道 Provider 是什么；
- `domain/` 可以理解业务语义；
- `pages/` 负责组合，不重复实现基础控件。

---

# 14. React 页面结构建议

```text
src/
├─ app/
│  ├─ router.tsx
│  ├─ providers.tsx
│  └─ AppShell.tsx
├─ pages/
│  ├─ dashboard/
│  ├─ providers/
│  ├─ proxy/
│  ├─ usage/
│  ├─ accounts/
│  ├─ workspace/
│  │  ├─ projects/
│  │  ├─ mcp/
│  │  ├─ prompts/
│  │  ├─ skills/
│  │  ├─ agents/
│  │  ├─ plugins/
│  │  └─ sessions/
│  └─ settings/
├─ components/
├─ features/
│  ├─ provider-switching/
│  ├─ proxy-runtime/
│  ├─ failover/
│  ├─ usage-analytics/
│  └─ account-quota/
├─ hooks/
├─ stores/
├─ services/
├─ lib/
└─ styles/
```

不要为了匹配此目录而强制移动整个项目。如果现有结构已经合理，优先增量调整。

---

# 15. 状态管理边界

至少明确以下状态：

## Global App State

```ts
interface AppContextState {
  activeClient: ClientType;
  proxyStatus: ProxyRuntimeStatus;
  activeProviderByClient: Record<ClientType, ProviderId | null>;
  theme: ThemeMode;
  locale: Locale;
}
```

## Server / Backend State

以下状态应通过 query/cache 或现有 service 获取，不要重复复制到多个 store：

- Provider list；
- latency test；
- usage metrics；
- account quotas；
- proxy runtime；
- MCP / Skills / Agents list。

## Local UI State

- Drawer open；
- Dialog open；
- Filter；
- Search；
- Sort；
- Selected row。

不要放入全局 store。

---

# 16. Tauri 集成原则

## 16.1 IPC 封装

不要在页面组件里大量直接：

```ts
invoke('some_command')
```

统一 service：

```text
services/
├─ providerService.ts
├─ proxyService.ts
├─ usageService.ts
├─ accountService.ts
└─ workspaceService.ts
```

UI 只调用语义函数：

```ts
providerService.switchProvider(...)
proxyService.restart(...)
usageService.getSummary(...)
```

## 16.2 Error Mapping

Rust/Tauri 原始错误统一转换为 UI Error：

```ts
interface AppError {
  code: string;
  title: string;
  message: string;
  recoverable: boolean;
  detail?: string;
}
```

用户默认看到可理解信息；技术详情可展开复制。

## 16.3 Event

运行状态变化建议事件驱动：

- proxy-status-changed；
- provider-switched；
- latency-test-completed；
- usage-updated；
- account-quota-updated。

避免多个页面分别轮询同一状态。

---

# 17. Loading / Empty / Error / Offline 状态

每个页面都必须设计四态，而不是只设计“有数据”。

## Loading

- 页面首次加载用 Skeleton；
- 按钮动作使用局部 spinner；
- 不要整个页面因为一个 Provider 测速而 blocking。

## Empty

例如无 Provider：

```text
No providers yet
Add your first API provider to start routing requests.
[Add Provider] [Import]
```

## Error

提供：

- 简短原因；
- Retry；
- Copy details。

## Backend / Proxy unavailable

如果 Tauri 后端正常但代理未启动，不要显示成“应用错误”。这是运行状态。

---

# 18. Toast / Notification 规范

Toast 只用于短暂结果：

- Provider switched；
- Copied；
- Saved；
- Test completed。

不要用 Toast 承载：

- 长错误堆栈；
- 需要用户决策的信息；
- OAuth 指引；
- Failover 详细说明。

严重错误用 Alert / Dialog / Error Panel。

---

# 19. Command Palette（第二阶段）

建议后续支持 `Ctrl/Cmd + K`：

```text
> Switch provider to DeepSeek
> Restart proxy
> Open Usage
> Test all providers
> Open MCP
> Change client to Codex
```

这是桌面开发工具非常适合的交互，但不要作为第一阶段阻塞项。

---

# 20. 响应式策略

目标主要是桌面：

- `>= 1440`：完整 Sidebar + 宽布局；
- `1024–1439`：标准布局；
- `800–1023`：Sidebar collapse；
- `< 800`：保证可用，但无需按移动 App 重新设计。

禁止通过固定 1600px 宽度制造横向滚动。

当前统计页出现横向 scrollbar 的区域需要重点消除或限制为图表内部滚动。

---

# 21. Accessibility

最低要求：

- 所有 IconButton 有 `aria-label` / tooltip；
- 不依赖颜色单独表达成功/失败；
- 键盘可访问菜单、Dialog、Drawer；
- Dialog focus trap；
- Escape 可关闭非破坏性浮层；
- 表单 Label 与 input 关联；
- Focus ring 不允许被全局 CSS 去掉；
- 状态变化必要时使用 aria-live；
- 图表提供文字摘要。

---

# 22. 国际化

当前已有简体中文切换，因此新 UI 禁止在 JSX 内继续散落硬编码字符串。

建议 namespace：

```text
common
navigation
dashboard
providers
proxy
usage
accounts
workspace
settings
errors
```

布局需要容忍英文比中文更长。

---

# 23. 图标规范

统一一个 icon library，不混用多套风格。

建议尺寸：

- 14px：表格小操作；
- 16px：Button / Sidebar；
- 18–20px：Section / Status；
- 24px：Empty State。

删除操作永远使用 danger semantic，而不是只有红色垃圾桶无文字解释。

---

# 24. 动画规范

只保留功能性微动画：

- Hover：100–150ms；
- Drawer：180–220ms；
- Dialog：150–180ms；
- Sidebar：180–220ms；
- Progress 更新平滑过渡。

遵循 `prefers-reduced-motion`。

不要做：卡片浮起、大幅缩放、背景粒子、持续发光。

---

# 25. 安全与隐私 UI

UI 重构时特别注意：

- API Key 默认遮挡；
- OAuth refresh/access token 不直接显示；
- JSON 导入预览中对 token 做 masking；
- Copy API Key 是显式动作；
- Usage Request Detail 默认不展示 prompt；
- 日志导出提供“脱敏”选项；
- 删除账号明确说明删除的是本地凭据还是远程账号；
- Endpoint 可以直接展示，但 credential query parameter 必须脱敏。

---

# 26. 推荐实施阶段

## Phase 0 — Audit

Agent 首先：

1. 扫描项目目录；
2. 找到 React 入口；
3. 找到 Router；
4. 找到现有 UI library；
5. 找到状态管理；
6. 找到 Tauri invoke 封装；
7. 找到主题/i18n；
8. 列出当前页面与组件；
9. 列出不能破坏的核心功能；
10. 输出迁移 mapping。

**此阶段禁止大规模改代码。**

产物：`docs/ui-audit.md`。

## Phase 1 — Design Tokens + Primitive UI

实现：

- color tokens；
- spacing；
- typography；
- Button；
- Input；
- Select；
- Badge；
- StatusBadge；
- Card；
- Tooltip；
- Dropdown；
- Dialog；
- Drawer；
- EmptyState；
- Skeleton。

要求：旧页面暂时可以继续使用。

## Phase 2 — App Shell

实现：

- Sidebar；
- TitleBar；
- ContextHeader；
- ClientSelector；
- StatusBar；
- Router layout。

迁移原有导航，但暂时不重做全部页面内容。

## Phase 3 — Providers

优先重构供应商页，因为这是核心操作。

验收：

- 列表正常；
- 当前 Provider 正确；
- Switch 正常；
- Test 正常；
- Add/Edit 正常；
- Delete 正常；
- Import/Export 正常；
- Client 切换后数据正确。

## Phase 4 — Proxy

迁移运行状态、启动/停止、端口、failover。

重点验证：

- Running / Stopped；
- Restart；
- Port conflict；
- Failover config；
- 当前 Provider 同步。

## Phase 5 — Dashboard

Dashboard 使用已经稳定的 Provider + Proxy + Usage 数据组合，不新建重复后端逻辑。

## Phase 6 — Usage

重构统计页与过滤器、图表、请求表格。

## Phase 7 — Accounts

重构 Antigravity Account / Quota。

OAuth 逻辑尽量完全复用。

## Phase 8 — Workspace

将 Projects / MCP / Prompts / Skills / Agents / Plugins / Sessions 从 Settings 迁移到 Workspace。

## Phase 9 — Settings

删除已经迁出的业务模块，只保留 App Settings。

## Phase 10 — Cleanup

- 删除废弃组件；
- 删除旧 CSS；
- 清理重复 token；
- 清理未使用 route；
- 统一 icon；
- 统一 Toast；
- 补齐 i18n；
- Accessibility review；
- Dark mode review。

---

# 27. 每阶段 Agent 工作流程

后续 AI 每次执行一个阶段时必须遵守：

```text
1. Read existing implementation
2. Identify reusable logic
3. Write short implementation plan
4. Make minimal coherent changes
5. Run formatter
6. Run lint
7. Run TypeScript typecheck
8. Run frontend build
9. Run Tauri build/check if practical
10. Verify affected user flows
11. Summarize changed files
12. Record remaining issues
```

禁止：

- 未读现有代码就重写；
- 为一个页面引入新的状态库；
- 同时更换 Router + UI Library + State Library；
- 复制后端业务逻辑到前端；
- 用 mock 数据替换已经存在的真实调用后忘记恢复；
- 为通过类型检查大量使用 `any`；
- 删除“不理解”的兼容逻辑。

---

# 28. 验收清单

## Global

- [ ] Light mode 正常
- [ ] Dark mode 正常
- [ ] 简体中文正常
- [ ] 英文布局不溢出
- [ ] Windows 缩放 100% / 125% / 150% 基本正常
- [ ] Sidebar 可折叠
- [ ] Client 切换全局同步
- [ ] Proxy 状态全局同步
- [ ] 无明显横向页面滚动

## Providers

- [ ] Add
- [ ] Edit
- [ ] Switch
- [ ] Test
- [ ] Test All
- [ ] Duplicate
- [ ] Delete
- [ ] Import
- [ ] Export
- [ ] Current state
- [ ] Error state

## Proxy

- [ ] Start
- [ ] Stop
- [ ] Restart
- [ ] Change port
- [ ] Copy endpoint
- [ ] Failover toggle
- [ ] Retry status config
- [ ] Stream timeout

## Usage

- [ ] KPI
- [ ] Date filter
- [ ] Client filter
- [ ] Provider filter
- [ ] Model filter
- [ ] Charts
- [ ] Empty state
- [ ] Error state

## Accounts

- [ ] OAuth login
- [ ] Refresh quota
- [ ] Set active
- [ ] Import
- [ ] Delete
- [ ] Quota visualization

## Workspace

- [ ] Project snapshot
- [ ] MCP
- [ ] Prompt
- [ ] Skill
- [ ] Agent
- [ ] Plugin
- [ ] Session

---

# 29. 建议的最终页面关系

```text
/
├─ /dashboard
├─ /providers
│  └─ /providers/:id              optional full editor
├─ /proxy
├─ /usage
├─ /accounts
├─ /workspace
│  ├─ /projects
│  ├─ /mcp
│  ├─ /prompts
│  ├─ /skills
│  ├─ /agents
│  ├─ /plugins
│  └─ /sessions
└─ /settings
   ├─ /general
   ├─ /proxy-defaults
   ├─ /data
   ├─ /localization
   ├─ /environment
   └─ /about
```

如果现有 Router 不适合嵌套路由，可以保持当前实现方式，但 UI 信息架构应尽量保持一致。

---

# 30. 后续 AI 扩展任务 Prompt 模板

下面这段可以直接交给 Coding Agent：

```text
你正在维护 AI-Switcher，一个 Tauri + React 桌面应用。

请先阅读：
- docs/AI-Switcher-UI-Redesign-Plan.md
- 当前项目结构
- 与本次任务相关的现有页面、组件、store/service 和 Tauri command

本次只执行 UI Redesign Plan 中的【Phase X / 模块名】。

要求：
1. 不破坏现有业务功能和配置兼容性。
2. 优先复用现有 Rust/Tauri 逻辑，不重复实现业务逻辑。
3. 先输出你发现的现有实现和修改计划，再开始修改。
4. 使用已有技术栈；除非确有必要，不新增大型依赖。
5. 新组件遵守 ui / domain / page 的分层原则。
6. 所有状态必须包含 loading / empty / error / success 情况。
7. 保持 Light / Dark Theme 和 i18n 兼容。
8. 不允许使用大量 any 绕过类型系统。
9. 完成后执行项目已有的 format / lint / typecheck / test / build。
10. 最后输出：修改文件、实现内容、验证结果、遗留问题、下一阶段建议。

如果文档规划与现有代码结构冲突，不要机械照搬目录；优先保持现有架构稳定，并说明你的适配方式。
```

---

# 31. 建议 AI 首次真正执行时的任务

不要第一次就要求 AI “完成整个新 UI”。推荐第一个任务只做：

```text
Phase 0 + Phase 1 的设计审计，不改业务页面。

1. 分析当前 React/Tauri 项目结构。
2. 找出当前主题、CSS、组件、Router、状态管理、Tauri IPC。
3. 对照 UI Redesign Plan 输出迁移矩阵。
4. 建立 semantic design tokens。
5. 建立最基础的 Button / Badge / StatusBadge / Card / Tooltip。
6. 不迁移 Providers 页面。
7. 保证现有 UI 行为完全不变。
```

这样可以先验证 AI 是否真正理解项目，再让它进入 App Shell 和 Providers 核心重构。

---

# 32. 优先级总结

**P0：必须先完成**

- Design Tokens；
- App Shell；
- Client Selector；
- Global Proxy Status；
- Providers；
- Proxy Control。

**P1：核心体验提升**

- Dashboard；
- Usage；
- Accounts；
- Workspace IA。

**P2：增强功能**

- Command Palette；
- Request Detail；
- Failover Timeline；
- Config Diff；
- Keyboard shortcuts；
- 更完整可访问性。

**P3：后续可选**

- 自定义 Dashboard；
- 多窗口；
- Provider 分组；
- Usage 导出报表；
- 更复杂的诊断中心。

---

# 33. 最终设计判断标准

新 UI 是否成功，不以“看起来更现代”为唯一标准，而看以下问题能否在 3–5 秒内回答：

1. **现在代理运行了吗？**
2. **Claude Code / Codex 当前走哪个 Provider？**
3. **Provider 是否健康、延迟多少？**
4. **如何快速切换 Provider？**
5. **最近有没有请求失败？**
6. **自动故障切换是否开启、下一候选是谁？**
7. **今天用了多少请求 / Token / 估算成本？**
8. **Antigravity 哪个账号是 Active、额度还剩多少？**
9. **MCP / Skills / Agents 在哪里管理？**
10. **全局应用设置在哪里？**

如果这些问题仍需要在多个页面之间反复寻找，则说明重构只完成了视觉换皮，没有完成产品结构重构。

---

## 文档维护规则

后续每完成一个 Phase，建议在本文档末尾追加 Change Log，而不是不断覆盖原规划：

```text
## Change Log

### 2026-xx-xx — Phase 2 App Shell
- Completed: ...
- Deviations from plan: ...
- Reason: ...
- Follow-up: ...
```

对于已经被代码实践证明不合适的规划，应修改本文档并说明原因，使它持续作为后续 AI Agent 的可信上下文，而不是一次性需求文档。
