import { create } from "zustand";
import type { ProviderTarget, SessionProvider } from "@/types/backend";
import { USAGE_PERIOD_VALUES, type UsagePeriod } from "@/utils/usagePeriod";

const STORAGE_KEY = "cs.pagePreferences";

interface PersistedPagePreferences {
  providersTarget?: ProviderTarget;
  proxyTarget?: ProviderTarget;
  usagePeriod?: UsagePeriod;
  /** Providers heatmap period; falls back to usagePeriod on first load. */
  heatmapPeriod?: UsagePeriod;
  usageLogTarget?: ProviderTarget | "all";
  heatmapSource?: ProviderTarget | "all";
  sessionsProvider?: SessionProvider;
}

interface PagePreferencesState {
  providersTarget: ProviderTarget;
  proxyTarget: ProviderTarget;
  usagePeriod: UsagePeriod;
  heatmapPeriod: UsagePeriod;
  usageLogPage: number;
  usageLogTarget: ProviderTarget | "all";
  heatmapSource: ProviderTarget | "all";
  sessionsProvider: SessionProvider;
  setProvidersTarget: (target: ProviderTarget) => void;
  setProxyTarget: (target: ProviderTarget) => void;
  setUsagePeriod: (period: UsagePeriod) => void;
  setHeatmapPeriod: (period: UsagePeriod) => void;
  setUsageLogPage: (page: number) => void;
  setUsageLogTarget: (target: ProviderTarget | "all") => void;
  setHeatmapSource: (target: ProviderTarget | "all") => void;
  setSessionsProvider: (provider: SessionProvider) => void;
}

const DEFAULTS: Pick<
  PagePreferencesState,
  | "providersTarget"
  | "proxyTarget"
  | "usagePeriod"
  | "heatmapPeriod"
  | "usageLogTarget"
  | "heatmapSource"
  | "sessionsProvider"
> = {
  providersTarget: "claude_code",
  proxyTarget: "claude_desktop",
  usagePeriod: 365,
  heatmapPeriod: 365,
  usageLogTarget: "all",
  heatmapSource: "all",
  sessionsProvider: "claude_code",
};

function isProviderTarget(value: unknown): value is ProviderTarget {
  return value === "claude_code" || value === "claude_desktop" || value === "codex";
}

function isSessionProvider(value: unknown): value is SessionProvider {
  return value === "claude_code" || value === "codex";
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
  const usagePeriod = isUsagePeriod(stored.usagePeriod) ? stored.usagePeriod : DEFAULTS.usagePeriod;
  const usageLogTarget = isUsageLogTarget(stored.usageLogTarget)
    ? stored.usageLogTarget
    : DEFAULTS.usageLogTarget;
  return {
    providersTarget: isProviderTarget(stored.providersTarget)
      ? stored.providersTarget
      : DEFAULTS.providersTarget,
    proxyTarget: isProviderTarget(stored.proxyTarget)
      ? stored.proxyTarget
      : DEFAULTS.proxyTarget,
    usagePeriod,
    heatmapPeriod: isUsagePeriod(stored.heatmapPeriod) ? stored.heatmapPeriod : usagePeriod,
    usageLogTarget,
    heatmapSource: isUsageLogTarget(stored.heatmapSource) ? stored.heatmapSource : usageLogTarget,
    sessionsProvider: isSessionProvider(stored.sessionsProvider)
      ? stored.sessionsProvider
      : DEFAULTS.sessionsProvider,
  };
}

function persistSlice(
  state: Pick<
    PagePreferencesState,
    | "providersTarget"
    | "proxyTarget"
    | "usagePeriod"
    | "heatmapPeriod"
    | "usageLogTarget"
    | "heatmapSource"
    | "sessionsProvider"
  >,
) {
  writePersisted({
    providersTarget: state.providersTarget,
    proxyTarget: state.proxyTarget,
    usagePeriod: state.usagePeriod,
    heatmapPeriod: state.heatmapPeriod,
    usageLogTarget: state.usageLogTarget,
    heatmapSource: state.heatmapSource,
    sessionsProvider: state.sessionsProvider,
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
  setHeatmapPeriod: (heatmapPeriod) => {
    set({ heatmapPeriod });
    persistSlice(get());
  },
  setUsageLogPage: (usageLogPage) => set({ usageLogPage }),
  setUsageLogTarget: (usageLogTarget) => {
    set({ usageLogTarget });
    persistSlice(get());
  },
  setHeatmapSource: (heatmapSource) => {
    set({ heatmapSource });
    persistSlice(get());
  },
  setSessionsProvider: (sessionsProvider) => {
    set({ sessionsProvider });
    persistSlice(get());
  },
}));
