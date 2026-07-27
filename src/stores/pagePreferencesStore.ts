import { create } from "zustand";
import type { ProviderTarget } from "@/types/backend";

interface PagePreferencesState {
  providersTarget: ProviderTarget;
  proxyTarget: ProviderTarget;
  usageDays: number;
  usageLogPage: number;
  usageLogTarget: ProviderTarget | "all";
  setProvidersTarget: (target: ProviderTarget) => void;
  setProxyTarget: (target: ProviderTarget) => void;
  setUsageDays: (days: number) => void;
  setUsageLogPage: (page: number) => void;
  setUsageLogTarget: (target: ProviderTarget | "all") => void;
}

export const usePagePreferencesStore = create<PagePreferencesState>((set) => ({
  providersTarget: "claude_code",
  proxyTarget: "claude_desktop",
  usageDays: 30,
  usageLogPage: 0,
  usageLogTarget: "all",
  setProvidersTarget: (providersTarget) => set({ providersTarget }),
  setProxyTarget: (proxyTarget) => set({ proxyTarget }),
  setUsageDays: (usageDays) => set({ usageDays }),
  setUsageLogPage: (usageLogPage) => set({ usageLogPage }),
  setUsageLogTarget: (usageLogTarget) => set({ usageLogTarget }),
}));
