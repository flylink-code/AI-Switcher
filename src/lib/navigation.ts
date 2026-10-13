import { createContext, useContext } from "react";
import type { PageKey } from "@/lib/pageRegistry";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { useProviderAccountsStore } from "@/stores/providerAccountsStore";

/** Lets any page trigger shell navigation without prop drilling. */
export const NavigationContext = createContext<(key: PageKey) => void>(() => {});

export function useNavigatePage(): (key: PageKey) => void {
  return useContext(NavigationContext);
}

/**
 * Maps retired primary keys onto the current shell and selects the matching
 * section. Callers still pass the old key; the active page is the resolved one.
 */
export function resolvePageNavigation(key: PageKey): PageKey {
  const { setWorkspaceTab } = usePagePreferencesStore.getState();
  if (key === "gateway" || key === "proxy") {
    return "providers";
  }
  if (key === "antigravity") {
    // The reverse-proxy account pool is now a drawer on the providers page.
    useProviderAccountsStore.getState().openDrawer("antigravity");
    return "providers";
  }
  if (key === "sessions") {
    setWorkspaceTab("sessions");
    return "workspace";
  }
  // 中文化已迁入 Agent 页对应卡的「高级设置」；旧 `localization` 键仍有效，落到 Agent 页。
  if (key === "agentTools" || key === "localization") {
    return "agentHub";
  }
  return key;
}
