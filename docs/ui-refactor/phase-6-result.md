# Phase 6 交付报告 — Usage Analytics 用量分析与请求诊断

> 本文档记录 Phase 6 的执行结果。系统已将 Usage 页面升级为高信息密度的 Developer Analytics Workbench。

---

## A. Changed Files (修改与新增文件清单)

| 文件路径 | 类型 | 作用 / 目的 | 风险层级 |
|---|---|---|---|
| `docs/ui-refactor/phase-6-preflight.md` | 新增文档 | Phase 6 预检与统计口径审计日志 | 零风险 |
| `src/components/usage/UsageToolbar.tsx` | 新增组件 | 过滤器与包含会话同步/价格配置的工具栏组件 | 零风险 |
| `src/components/usage/index.ts` | 新增导出 | 域名组件导出 | 零风险 |
| `src/pages/UsagePage.tsx` | 重构页面 | Usage 页面 Presentation 视觉与布局重构 | 极低 |

---

## B. Usage Architecture (架构说明)

- **`UsagePage`**: 页面组合与框架排版。包含顶部提示、`UsageToolbar` 筛选器、4 核心 Metric 卡片、Recharts `UsageTrendChart` 趋势图、`UsageBreakdownCard` 明细矩阵及 `Table` 请求记录。
- **`UsageToolbar`**: 整合统计周期 (`period`)、统计来源 (`logTargetApp`)、一键增量同步/重构与模态框入口。
- **Request Explorer**: 响应式呈现请求时间、应用、供应商、模型、HTTP 状态码、错误分类、Token 占用（含 Cached 缓存说明）、耗时与流式标记。点击诊断行展开右侧 `Drawer` 诊断。

---

## C. Accounting Protection (统计口径 100% 保护证明)

- **Requests & Success Rate**: 严格保持 `(successfulRequestCount / requestCount) * 100` (%) 公式。
- **Total Tokens**: 严格保持 `inputTokens` + `cacheRead` + `cacheCreation` + `outputTokens` 的原生聚合规则。
- **Estimated Cost**: 保持 "Estimated Cost" 语义与免责 Tooltip 说明，单价与模型价格映射零篡改。
- **Timezone**: 保持 UTC/本地字符串转换语义的一致性。

---

## D. Performance & Security (性能与安全)

- **分页优化**: 请求日志采用后端分页（20 条/页），前端 `Table` 仅渲染当前页数据，消除了数万条历史日志的 DOM 爆满隐患。
- **Memoization**: `UsageTrendChart` 与 `chartCurrency` 采用 `useMemo` 优化前端趋势计算。
- **隐私脱敏**: 诊断 `Drawer` 仅展示已有 `diagnostic` 字段，不额外截获或存储敏感 Authorization/Token。

---

## E. Phase 7 Accounts / Antigravity Readiness (Phase 7 Accounts 重构准备情况)

针对下一阶段 (Phase 7 Accounts / Antigravity) 的只读审计与回答：
1. **当前 Accounts 页面文件**: `src/pages/AntigravityPage.tsx`。
2. **Account Pool 来源**: `getAntigravityAccountPool` / `getAntigravityGatewayStatus`。
3. **Active Account 来源**: 后端底层协议自动选择，前端由 `QuotaBars` / `AccountTable` 展示。
4. **Quota 数据源**: 45 秒首刷 + 5 分钟后台循环事件 `antigravity-quota-refreshed` 广播。
5. **OAuth 登录流程**: `startGoogleOauthLogin` → 浏览器鉴权 → 回调绑定。
6. **Phase 7 建议修改**: 重构卡片式账号列表，展示各个模型的 5h/7d 额度 Progress 条，抽离 Drawer 明细管理。

---

*Phase 6 交付时间: 2026-08-10*
