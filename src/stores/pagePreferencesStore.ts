import { create } from "zustand";
import type { ProviderTarget } from "@/types/backend";
import type { UsagePeriod } from "@/utils/usagePeriod";

interface PagePreferencesState {
  providersTarget: ProviderTarget;
  proxyTarget: ProviderTarget;
  usagePeriod: UsagePeriod;
  usageLogPage: number;
  usageLogTarget: ProviderTarget | "all";
  setProvidersTarget: (target: ProviderTarget) => void;
  setProxyTarget: (target: ProviderTarget) => void;
  setUsagePeriod: (period: UsagePeriod) => void;
  setUsageLogPage: (page: number) => void;
  setUsageLogTarget: (target: ProviderTarget | "all") => void;
}

export const usePagePreferencesStore = create<PagePreferencesState>((set) => ({
  providersTarget: "claude_code",
  proxyTarget: "claude_desktop",
  usagePeriod: 365,
  usageLogPage: 0,
  usageLogTarget: "all",
  setProvidersTarget: (providersTarget) => set({ providersTarget }),
  setProxyTarget: (proxyTarget) => set({ proxyTarget }),
  setUsagePeriod: (usagePeriod) => set({ usagePeriod }),
  setUsageLogPage: (usageLogPage) => set({ usageLogPage }),
  setUsageLogTarget: (usageLogTarget) => set({ usageLogTarget }),
}));
