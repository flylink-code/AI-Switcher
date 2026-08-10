# Phase 4 Preflight 预检日志 — Proxy 模块

## 1. Proxy Page 代码与状态映射表

| 文件路径 / 数据源 | 职责与能力 | 包含业务逻辑? | 仅纯 UI 展示? | 重构安全性评估 |
|---|---|---|---|---|
| `src/pages/ProxyPage.tsx` | 代理控制中心主入口 | 否 (委托 Service) | 组合层 | 安全，可重构 UI/UX |
| `src/services/proxy.ts` | 代理 Tauri IPC API | 是 (`startProxy`, `stopProxy`, `setProxyPort` 等) | 否 | **绝对保护，禁止重写算法与接口** |
| `src/lib/appQueries.ts` | React Query 声明 | 否 (定义 `proxyStatusOptions`) | 否 | 保持现有的 `["proxy-status", target]` Key |
| `src/components/layout/StatusBar.tsx` | 底部运行状态栏 | 否 | 是 | 与 ProxyPage 共享 Query Cache，零重复轮询 |

---

## 2. Polling 去重审计 (Polling Audit)

- **当前状态获取**: `ProxyPage` 与 `StatusBar` 均统一调用 `useQuery(proxyStatusOptions(target))`。
- **去重验证**: 两者共享相同的 Query Key `["proxy-status", target]`。React Query 自动管理多组件订阅与 StaleTime，在页面切换或刷新时零冗余 `setInterval` 或重复事件监听。
- **审计结论**: **不存在重复 Polling**，可以直接安全复用现有 React Query Cache。

---

## 3. 业务动作与 IPC 命令映射 (Actions Mapping)

- **启动代理 (Start Proxy)**: `handleStart` → `setProxyPort(port, target)` → `startProxy(port, target)` → `queryClient.setQueryData`
- **停止代理 (Stop Proxy)**: `handleStop` → `stopProxy(target)` → `queryClient.setQueryData`
- **开关 Failover (Toggle Failover)**: `handleFailoverChange(enabled)` → `setProxyFailoverEnabled(enabled)`
- **修改重试状态码 (Retry Codes)**: `handleRetryCodesSave()` → `setProxyRetryableStatusCodes(codes)`
- **修改流超时 (Idle Timeout)**: `handleIdleTimeoutSave()` → `setProxyStreamingIdleTimeoutSecs(secs)`

---

## 4. OpenCode 特殊逻辑

OpenCode 客户端属于直连模式，不启动本地反向代理服务（后端端口写死/停用，UI 显示 `proxy.opencodeDirectHint` 直连提示）。UI 需优雅降级该场景。

---
*预检完成，下一步：构建 Proxy Domain 组件与 ProxyPage 重构。*
