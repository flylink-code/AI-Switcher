# Phase 3 Preflight 预检日志 — Providers 模块

## 1. Provider Page 代码结构映射

| 文件路径 | 职责范围 | 包含 Client 作用域? | 包含业务逻辑? | 仅纯 UI 展示? | 重构安全性评估 |
|---|---|---|---|---|---|
| `src/pages/ProvidersPage.tsx` | 供应商页面主入口 | 是 (`target`) | 否 (委托 Hook/Store) | 组合层 | 安全，可大范围重构 Presentation |
| `src/lib/useProviderActions.ts` | 供应商操作 Hook | 是 (`target`) | 是 (处理 Switch/Test/Delete/Import/Export) | 否 | **绝对保护，禁止重载逻辑** |
| `src/stores/providersStore.ts` | 供应商 Zustand Store | 是 (`target`) | 是 (加载、排序、错误) | 否 | **绝对保护，保持不变** |
| `src/components/ProviderForm.tsx` | 供应商编辑 Modal/Form | 是 (`target`) | 否 (表单展示与格式化) | 属于 UI 表单 | 安全，保持逻辑不变 |
| `src/services/providers.ts` | 后端服务与 IPC | 是 (`target`) | 是 (Tauri IPC 接口) | 否 | **绝对保护，严禁触碰** |

---

## 2. Provider 数据模型 (Data Model)

确认当前 `Provider` 类型的真实结构（只读，禁止修改定义）：
```ts
interface Provider {
  id: string;
  name: string;
  targetApp: ProviderTarget; // "claude_code" | "claude_desktop" | "codex" | "opencode"
  baseUrl: string;
  apiKeySet: boolean;
  model: string;
  protocolType: "anthropic" | "openai" | string;
  isCurrent: boolean;
  failoverGroup: number;
  healthStatus?: "healthy" | "unhealthy" | "slow";
  healthLatencyMs?: number;
  modelMapping: Record<string, string>;
  // ...包含与 Codex/OpenCode 绑定的配置与模型缓存
}
```

---

## 3. Provider 操作与 IPC 映射

- **切换 (Switch)**: `handleSwitch(provider)` → `store.setCurrent()` → IPC `set_current_provider`
- **测试 (Test Connection)**: `handleTest(provider)` → IPC `test_provider`
- **测速 (Speedtest)**: `handleSpeedtest(provider)` → IPC `test_provider_latency`
- **删除 (Delete)**: `handleDelete(provider)` → `store.remove()` → IPC `delete_provider`
- **新增/编辑 (Add/Edit)**: `handleSubmit(values)` → `store.save()` → IPC `save_provider`
- **导入/导出 (Import/Export)**: `handleImportLive()`, `handleImportFile()`, `handleExport()`

---

## 4. 客户端切换 (Client Scope)

- 四大 App (`claude_code`, `claude_desktop`, `codex`, `opencode`) 的目标切换完全消费 `usePagePreferencesStore` 的 `providersTarget`。
- Phase 3 将移除 Providers 页面内部重复的 `WorkspaceTargetSegmented`，完全委托给 ContextHeader 的 `ClientSwitcher`。

---
*预检完成，下一步：构建 ProviderCard, ProviderToolbar 与 Provider Page 重构。*
