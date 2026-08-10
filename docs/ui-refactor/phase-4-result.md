# Phase 4 交付报告 — Proxy 控制中心重构

> 本文档记录 Phase 4 的执行结果。系统已将代理控制页面重构为直观、高效率的桌面级 Proxy Runtime Control Center。

---

## A. Changed Files (修改与新增文件清单)

| 文件路径 | 类型 | 作用 / 目的 | 风险层级 |
|---|---|---|---|
| `docs/ui-refactor/phase-4-preflight.md` | 新增文档 | Phase 4 预检与 Polling 去重审计日志 | 零风险 |
| `src/components/proxy/ProxyRuntimeCard.tsx` | 新增组件 | Hero 级代理运行状态卡片 (支持 Running/Stopped/Error/Starting 表达与 Start/Stop 核心操作) | 零风险 |
| `src/components/proxy/ResilienceSettings.tsx` | 新增组件 | Failover 开关与 HTTP 重试状态码/流式超时控制组件 | 零风险 |
| `src/components/proxy/index.ts` | 新增导出 | 域名组件导出 | 零风险 |
| `src/pages/ProxyPage.tsx` | 重构页面 | Proxy 页面 Presentation 视觉重构 (0 业务与反代引擎改动) | 极低 |

---

## B. Proxy Architecture (架构与域名组件)

- **`ProxyRuntimeCard`**: 顶层 Hero 状态容器，显式展现 Proxy 运行状态徽章 (`StatusBadge`)、目标 Provider (`targetProvider`)、监听端口与复制端点 (`endpointUrl`)。提供突出的 Start / Stop 核心按钮。
- **`ResilienceSettings`**: 归口隔离 Failover 故障自动切换开关、HTTP 重试状态码输入框以及流式断连超时秒数设置。
- **`ProxyPage`**: 组合层，移除内联的 Client 分段选择器，由 ContextHeader 全局驱动，完全消费现有业务 API。

---

## C. Polling Audit (去重审计结论)

- **`ProxyPage` 与 `StatusBar` 共享 Cache**: 两者订阅完全一致的 React Query Key `["proxy-status", target]`。
- **零额外轮询**: 无任何新增的 `setInterval` 或重复后端 Event 监听。前端页面与底栏在数据更新时自动同步更新，内存与 IPC 资源消耗最小。

---

## D. Business Logic Reuse (100% 业务逻辑复用)

- **Start Proxy**: 调用 `setProxyPort(port, target)` → `startProxy(port, target)` → 更新 React Query Cache。
- **Stop Proxy**: 调用 `stopProxy(target)` → 更新 React Query Cache。
- **Failover 开关**: 调用 `setProxyFailoverEnabled(enabled)`。
- **HTTP 状态码重试**: 调用 `setProxyRetryableStatusCodes(codes)`。
- **流式超时**: 调用 `setProxyStreamingIdleTimeoutSecs(seconds)`。

---

## E. Phase 5 Dashboard Readiness (Phase 5 Dashboard 概览页准备就绪情况)

针对下一阶段 (Phase 5 Dashboard) 的只读审计与回答：
1. **当前是否有 Dashboard 页面**: 当前由 `WorkbenchPage.tsx` 承担 Home/Dashboard 功能。
2. **Dashboard 可复用的数据源**:
   - Proxy 状态: `proxyStatusOptions(target)`
   - 当前 Provider: `providerListOptions(target)`
   - 用量汇总与趋势: `getUsageDashboard()`
   - Antigravity 额度: `antigravity-quota-refreshed` 事件与账号 Store
3. **是否存在额外 Polling 风险**: 否。Dashboard 仅作为已获取数据的只读聚合展示层，无需新增任何后端查询轮询。
4. **Phase 5 建议修改的文件**:
   - `src/pages/WorkbenchPage.tsx` (或者新建 `src/pages/DashboardPage.tsx`)
   - 新建 `src/components/dashboard/` 共享卡片与健康组件。

---

*Phase 4 交付时间: 2026-08-10*
