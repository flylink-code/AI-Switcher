import { createContext, useContext } from "react";
import type { PageKey } from "@/lib/pageRegistry";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";

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
  const { setProvidersTab, setReverseTab, setWorkspaceTab } = usePagePreferencesStore.getState();
  if (key === "gateway" || key === "proxy") {
    setProvidersTab("smart");
    return "providers";
  }
  if (key === "antigravity") {
    setProvidersTab("reverse");
    setReverseTab("antigravity");
    return "providers";
  }
  if (key === "sessions") {
    setWorkspaceTab("sessions");
    return "workspace";
  }
  if (key === "agentTools") {
    return "agentHub";
  }
  return key;
}
