# Phase 5 交付报告 — Dashboard 概览页与 Read Model 架构

> 本文档记录 Phase 5 的执行结果。系统已建立高性能、轻量级、只读观察模式的 Operational Overview Dashboard。

---

## A. Changed Files (修改与新增文件清单)

| 文件路径 | 类型 | 作用 / 目的 | 风险层级 |
|---|---|---|---|
| `docs/ui-refactor/phase-5-preflight.md` | 新增文档 | Phase 5 预检与 Zero Polling 审计日志 | 零风险 |
| `src/components/dashboard/RuntimeSnapshot.tsx` | 新增组件 | 代理与系统运行状态快照组件 | 零风险 |
| `src/components/dashboard/ProviderSnapshot.tsx` | 新增组件 | 当前激活 Provider / 官方模式状态快照 | 零风险 |
| `src/components/dashboard/UsageSnapshot.tsx` | 新增组件 | 最近 24h 用量、Token 趋势与预估成本快照 | 零风险 |
| `src/components/dashboard/QuickActions.tsx` | 新增组件 | 快捷导航组件 | 零风险 |
| `src/components/dashboard/index.ts` | 新增导出 | 域名组件导出 | 零风险 |
| `src/pages/WorkbenchPage.tsx` | 重构页面 | Dashboard 概览页 Presentation 重构 (0 业务与服务变动) | 极低 |
| `src/i18n/locales/zh-CN.json` | 修改 i18n | 补充 `dashboard` 多语言命名空间 | 零风险 |
| `src/i18n/locales/en-US.json` | 修改 i18n | 补充 `dashboard` 英文命名空间 | 零风险 |

---

## B. Dashboard Architecture ( Read Model 架构)

- **Read Model 模式**: Dashboard 仅作为系统的只读聚合观察层。只负责读取、汇总、呈现与导航，绝不侵入修改或重载底层的配置保存与反代业务逻辑。
- **界面分层 (Operational Overview)**:
  1. **Runtime Overview**: 运行状态概览 (`RuntimeSnapshot`)
  2. **Primary Snapshots**: 供应商快照 (`ProviderSnapshot`) + 24h 用量快照 (`UsageSnapshot`)
  3. **Trend Charts**: 年度热力图 (`UsageCalendar`) + 24h 小时图 (`UsageTrendBars`)
  4. **Quick Actions**: 快捷导航矩阵 (`QuickActions`)

---

## C. Data Sources (共享数据源引用矩阵)

- **Runtime Snapshot**: 消费 `proxyStatusOptions(target)` (`["proxy-status", target]`) & `managedAppsRuntimeStatusOptions`
- **Provider Snapshot**: 消费 `store.providers` (`["providers", target]`)
- **Usage Snapshot**: 消费 `usageDashboardOptions("24h", heatmapSource)` & `usageTrendOptions`
- **Antigravity / Account**: 消费 `antigravity-gateway`

---

## D. Zero Polling Audit (零新增轮询确认)

- **新增 Polling 数量**: **0**
- **结论**: 彻底贯彻了硬性约束。Dashboard 页面全量复用了共享层的 React Query 缓存，没有在页面初始化或 Mount 时建立任何额外的 `setInterval`、`setTimeout` 或独立 Polling 定时器。

---

## E. Partial Failure (局部故障隔离)

- 每个 Snapshot (Runtime, Provider, Usage) 均拥有独立的状态守卫。
- 即使 Usage 接口失败（展现局部 Error Alert），Runtime 与 Provider 模块仍可正常呈现；即使某 Provider 健康度未知，也不阻塞整个 Dashboard 渲染。

---

## F. Phase 6 Usage Analytics Readiness (Phase 6 用量与统计准备情况)

针对下一阶段 (Phase 6 Usage Analytics) 的只读审计与回答：
1. **当前 Usage 页面文件**: `src/pages/UsagePage.tsx`。
2. **数据源**: `usageDashboardOptions`, `usageTrendOptions`, `listProxyRequestLogs`。
3. **统计周期**: 支持 `today`, `24h`, `7d`, `30d`, `365d` 等窗口。
4. **图表技术栈**: 基于 `Recharts`。
5. **Phase 6 重构建议**: 保留完整的过滤算子与统计算法，将图表与 Request Table 迁移为标准桌面级 2x2 网格与 Drawer 明细表现。

---

*Phase 5 交付时间: 2026-08-10*
