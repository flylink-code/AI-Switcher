import { create } from "zustand";
import type { ProviderTarget } from "@/types/backend";
import { USAGE_PERIOD_VALUES, type UsagePeriod } from "@/utils/usagePeriod";

const STORAGE_KEY = "cs.pagePreferences";

interface PersistedPagePreferences {
  providersTarget?: ProviderTarget;
  proxyTarget?: ProviderTarget;
  usagePeriod?: UsagePeriod;
  usageLogTarget?: ProviderTarget | "all";
}

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

const DEFAULTS: Pick<
  PagePreferencesState,
  "providersTarget" | "proxyTarget" | "usagePeriod" | "usageLogTarget"
> = {
  providersTarget: "claude_code",
  proxyTarget: "claude_desktop",
  usagePeriod: 365,
  usageLogTarget: "all",
};

function isProviderTarget(value: unknown): value is ProviderTarget {
  return value === "claude_code" || value === "claude_desktop" || value === "codex";
}

function isUsagePeriod(value: unknown): value is UsagePeriod {
  return USAGE_PERIOD_VALUES.some((period) => period === value);
}

function isUsageLogTarget(value: unknown): value is ProviderTarget | "all" {
  return value === "all" || isProviderTarget(value);
}

function readPersisted(): PersistedPagePreferences {
  if (typeof localStorage === "undefined") return {};
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return parsed as PersistedPagePreferences;
  } catch {
    return {};
  }
}

function writePersisted(state: PersistedPagePreferences) {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // Ignore quota / private-mode write failures.
  }
}

function initialState() {
  const stored = readPersisted();
  return {
    providersTarget: isProviderTarget(stored.providersTarget)
      ? stored.providersTarget
      : DEFAULTS.providersTarget,
    proxyTarget: isProviderTarget(stored.proxyTarget)
      ? stored.proxyTarget
      : DEFAULTS.proxyTarget,
    usagePeriod: isUsagePeriod(stored.usagePeriod) ? stored.usagePeriod : DEFAULTS.usagePeriod,
    usageLogTarget: isUsageLogTarget(stored.usageLogTarget)
      ? stored.usageLogTarget
      : DEFAULTS.usageLogTarget,
  };
}

function persistSlice(
  state: Pick<
    PagePreferencesState,
    "providersTarget" | "proxyTarget" | "usagePeriod" | "usageLogTarget"
  >,
) {
  writePersisted({
    providersTarget: state.providersTarget,
    proxyTarget: state.proxyTarget,
    usagePeriod: state.usagePeriod,
    usageLogTarget: state.usageLogTarget,
  });
}

export const usePagePreferencesStore = create<PagePreferencesState>((set, get) => ({
  ...initialState(),
  usageLogPage: 0,
  setProvidersTarget: (providersTarget) => {
    set({ providersTarget });
    persistSlice(get());
  },
  setProxyTarget: (proxyTarget) => {
    set({ proxyTarget });
    persistSlice(get());
  },
  setUsagePeriod: (usagePeriod) => {
    set({ usagePeriod });
    persistSlice(get());
  },
  setUsageLogPage: (usageLogPage) => set({ usageLogPage }),
  setUsageLogTarget: (usageLogTarget) => {
    set({ usageLogTarget });
    persistSlice(get());
  },
}));
