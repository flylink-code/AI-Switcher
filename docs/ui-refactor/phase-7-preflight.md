# Phase 7 Preflight 预检日志 — Accounts / Antigravity 账号池与额度模块

## 1. Account Architecture Map (账号体系架构映射)

| 视角 / 职责 | 结构 / 方法位置 | 技术层级 (Layer) | 属性说明 |
|---|---|---|---|
| **Accounts Page** | `src/pages/AntigravityPage.tsx` | UI Presentation | 视图控制与管理主界面 |
| **Account list source** | `listAntigravityAccounts` (`queryKey: ["antigravity-accounts"]`) | React Query / IPC | 账号池公有数据来源 |
| **Active account source** | `AntigravityAccountPublic.isActive` | Backend / Service | 底层自动轮换/手动指定的当前激活账号 |
| **Quota source** | `AntigravityAccountPublic.quota` | Backend / Quota Sync | Gemini/Claude 5h 与 7d 额度只读快照 |
| **Quota reset source** | `QuotaSnapshot.reset_at` / `accountQuotaSummary` | Backend / Frontend Helper | 额度重置倒计时计算 |
| **Rotation state source** | `src-tauri/src/antigravity/pool.rs` (`AccountPool`) | Runtime-critical | 自动软选择、健康分、429/401 自动故障转移 |
| **OAuth entry** | `startAntigravityOauthLogin` | IPC / Tauri Command | 打开系统浏览器发起 Google OAuth |
| **OAuth callback handling** | `src-tauri/src/antigravity/oauth.rs` (`login_with_browser`) | Runtime-critical | 本地 `127.0.0.1` 临时 HTTP 回调服务接收 Token |
| **Add / Import handler** | `importAntigravityAccounts` | Service / IPC | JSON 导入 Antigravity 凭据 |
| **Remove account handler** | `removeAntigravityAccount` | Service / IPC | 移除账号并同步持久化配置 |
| **Switch account handler** | `setAntigravityActiveAccount` | Service / IPC | 强制指定激活账号并重置状态 |
| **Manual refresh handler** | `refreshAntigravityQuotas` | Service / IPC | 手动触发全量账号 Cloud Code 额度刷新 |
| **Automatic refresh mechanism** | `spawn_antigravity_quota_refresh` | Backend Tokio Task | 45s 首次刷新 + 5min 循环刷新 |
| **Tauri event listeners** | `listen("antigravity-quota-refreshed")` | Tauri Event / UI | 收到广播事件后同步 `invalidateQueries` |

---

## 2. Refresh / Polling / Event 专项审计

- **45s 首次刷新 (Initial Quota Refresh)**:
  - 代码位置: `src-tauri/src/lib.rs:649` (`spawn_antigravity_quota_refresh`)
  - 逻辑: 应用启动 45 秒后，后台 task 自动触发 `try_refresh_all_quotas().await`。
  - 保护要求: 完全保持启动时机与触发条件，UI 重构绝不篡改或新增前端秒级定时刷新。
- **5min 事件轮询 (Event Polling & Broadcast)**:
  - 代码位置: `src-tauri/src/antigravity/quota_sync.rs:10` (`QUOTA_REFRESH_INTERVAL_SECS = 300`)
  - 后端行为: 每 5 分钟轮询所有账号额度，成功后向前端发送 `antigravity-quota-refreshed` 事件。
  - 前端行为: `AntigravityPage.tsx` 在挂载期间 `listen("antigravity-quota-refreshed")`，监听到广播后自动更新缓存。
  - 前端 RefetchInterval: `useQuery(["antigravity-accounts"])` 设置了 `refetchInterval: 300_000` (5分钟) 作为保底防护。

---

## 3. Zero Duplicate Refresh & Dashboard 关系审计

- **Dashboard 消费关系**:
  - `WorkbenchPage.tsx` 仅订阅 `queryKey: ["antigravity-gateway"]` (`getAntigravityGatewayStatus`，5s 轮询)，只展示网关 Running 状态与端口。
  - Dashboard 未开辟独立账号额度轮询 Timer。
- **Accounts 页面独立性**:
  - Accounts 页面管理账号列表、Quota 快照与网关控制。
  - 数据源均来自统一的 React Query `["antigravity-accounts"]` 与 `["antigravity-gateway"]`。
- **阶段目标**:
  - `Duplicate quota polling added by Phase 7: 0`
  - `New per-card / page timer added: 0`

---

## 4. 强制业务保护区与禁止改变行为

- **禁止修改后端文件**:
  - `src-tauri/src/antigravity/gateway/`
  - `src-tauri/src/antigravity/oauth.rs`
  - `src-tauri/src/antigravity/pool.rs`
  - `src-tauri/src/antigravity/quota_sync.rs`
  - `src/services/antigravity.ts`
  - `src/services/ipc.ts`
- **禁止修改的业务行为**:
  - OAuth 登录协议与回调服务器
  - Access Token / Refresh Token 刷新与安全持久化
  - 账号轮换选择算法 (Health score / cooldown / 429 failover)
  - 额度刷新时间与区间定义

---
*Preflight 完成，准备开始 Phase 7 Accounts UI 重构。*
