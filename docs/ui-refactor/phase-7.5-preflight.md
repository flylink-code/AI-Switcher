# Phase 7.5 Preflight 预检日志 — Visual Transformation & Design System Enforcement

## 1. Phase 7.5 定位与目标

Phase 7.5 是 AI-Switcher UI 重构的**正式视觉重构阶段**。
目标：将 AI-Switcher 从 **Generic Ant Design Admin Dashboard** 推进为 **Modern Desktop Developer Control Center**。

关键词：Desktop-first, Developer-oriented, Compact, Calm, Precise, Operational, High information density, Clear hierarchy, Native-feeling, Consistent.

## 2. Visual Inventory Audit (视觉资产与样式问题审计)

### 现有 Token 结构
- `src/styles/tokens/primitive.css`: 包含色阶 (gray, slate, blue, green, red, amber)、字体、字号、间距与圆角定义。
- `src/styles/tokens/semantic.css`: 定义 Light & Dark 主题下的基础背景色、边框色、文本色与品牌色。
- `src/styles/tokens/components.css`: 提供 `.ui-surface`, `.ui-card`, `.ui-badge`, `.ui-status-dot` 等共享基础类。
- `src/styles.css`: 全局系统层及部分页面特定 CSS Override。

### 识别出的主要视觉痛点 (Visual Pain Points)
1. **Ant Design Admin 后台感明显**: 大量白底卡片 (`#ffffff`) 放置在浅灰背景 (`#f5f5f5` / `gray-50`) 上，卡片感（Card Wall）严重，缺乏层次感。
2. **文本层级弱 (Weak Typography Hierarchy)**: 各页面 Heading、Section Title、Label、Body 及 Monospace 字体缺少统一等级控制。
3. **控件密度与高度不统一 (Inconsistent Desktop Density)**: Control Height、Table 行高、Card 内边距在各页面呈现混乱。
4. **底层浅色与深色对比度不足**: Dark 模式下部分 border 与 background 对比低，Light 模式下无明确深浅对比层级。
5. **导航与 Chrome 层级冗余**: 底部 Floating View Switcher 与侧边栏 (Sidebar) 导航在部分业务场景下存在冗余或层级重叠。
6. **页面组件样式不一致**: ProxyPage 类似于 Settings Form、ProvidersPage 像 Admin Card 集合，而 Workbench 保持了部分独立样式，跨页面体验不一致。

## 3. Scope of Pages & Components for Phase 7.5

在不影响任何后端业务逻辑的前提下，以下 Presentation Layer 将接受全面视觉升级：

| 模块 / 页面 | 核心目标 |
|---|---|
| **Design Tokens & Theme** | 强化 Semantic Tokens，重构 Light/Dark 色彩与 Surface 层级；重载 antd Theme Token |
| **App Shell & Chrome** | 统一 Sidebar, ContextHeader, ClientSwitcher, StatusBar 视觉风格；清理/收敛 Bottom Floating Navigation |
| **ProvidersPage** | 重组 Active/Available 层次，精简 ProviderCard 的 Admin 感觉，突出 Health/Model/Latency 状态 |
| **ProxyPage** | 从 Settings Form 转为 **Runtime Control Center** 风格 (Functional Hero + Compact Settings Rows) |
| **WorkbenchPage / Dashboard** | 强化 Operational Overview 感受，规范 KPI 与 Chart 区域视觉，消除同质白卡 |
| **UsagePage** | 保持数据架构不变，重构 Toolbar, Metric Display, Recharts Chart 样式及 Data Tables |
| **AntigravityPage** | 优化 Account Card 与 Gateway 卡片样式，统一与 Desktop Tool 保持一致的 Developer 视觉 |
| **SettingsPage / Projects** | 清理 Legacy Admin 风格，采用 Section Header + Divider + Row 规范布局 |

## 4. 强制 Runtime 保护承诺

在 Phase 7.5 实施期间：
- **Provider Switching / Testing / Delete Runtime**: 无修改
- **Proxy Engine & Failover Algorithm**: 无修改
- **Usage Aggregation & Cost Formula**: 无修改
- **OAuth, Token, Antigravity Rotation & Quota Sync**: 无修改
- **新增 Timer / Polling / Refetch / Event Listener**: `0`

---
*Preflight 预检完成，即将进入 Phase 7.5 设计系统强化与 App Shell 视觉提质。*
