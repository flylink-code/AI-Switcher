# Phase 1 Preflight 预检日志

## 文件级映射清单

1. **React 应用入口文件**: `src/main.tsx`
2. **当前 App 根组件**: `src/App.tsx`
3. **当前主导航 / activeKey 控制位置**: `src/App.tsx` (状态 `activeKey` + `handleNavigate` 统一回调，在 `AppLayout.tsx` / `FloatingViewSwitcher.tsx` / `TitleBar.tsx` 中驱动)
4. **`pageRegistry.ts` 的具体页面注册结构**: `src/lib/pageRegistry.ts` (`PAGE_KEYS`, `pageLoaders` 按需 import 列表, `preloadPage`, `getLoadedPage`)
5. **Zustand store 所在文件**:
   - `src/stores/appStore.ts` (语言 & 后端可达状态)
   - `src/stores/themeStore.ts` (主题模式 `light` \| `dark` \| `system`)
   - `src/stores/providersStore.ts` (供应商健康度事件与状态)
   - `src/stores/pagePreferencesStore.ts` (页面视图偏好)
6. **React Query Provider 初始化位置**: `src/main.tsx` (使用 `src/lib/queryClient.ts` 中定义的 `queryClient` 实例化 `QueryClientProvider`)
7. **antd ConfigProvider / ThemeProvider 当前所在位置**: `src/App.tsx` (通过 `themeConfig` useMemo 计算 Ant Design 的 Token、组件 Token 与 Theme 算法)
8. **全局 CSS / reset / variables 文件**: `src/styles.css`
9. **当前共享 UI 组件目录**: `src/components/`
10. **重复/散落组件位置**:
    - Provider Card: `src/pages/WorkbenchPage.tsx`, `src/pages/ProvidersPage.tsx`
    - Provider Form: `src/components/ProviderForm.tsx`
    - Modal / Dialog: `src/components/ImportPreviewDialog.tsx`, `src/App.tsx`
    - 状态 Badge/Tag: 各页面独立内联样式标签
11. **常见的 Hard-code 样式值**:
    - Spacing: `16px`, `20px`, `24px`
    - Radius: `6px`, `8px`, `10px`, `12px`
    - Colors: `#007aff` (Primary Light), `#58a6ff` (Primary Dark), `#f5f5f7`, `#0f0f0f`, `#1a1a1a`, `#3d3d3d`, `#86868b`
12. **Dark Mode 状态**:
    - 已存在 `themeStore.ts` 和 `html[data-theme="dark"]` 属性绑定。

---
*预检完成，下一步：构建 Token Architecture 与 Primitive UI 组件。*
