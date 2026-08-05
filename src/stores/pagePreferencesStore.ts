import { create } from "zustand";
import type { ProviderTarget, SessionProvider } from "@/types/backend";
import { USAGE_PERIOD_VALUES, type UsagePeriod } from "@/utils/usagePeriod";

const STORAGE_KEY = "cs.pagePreferences";

interface PersistedPagePreferences {
  /** Global workspace context (Code / Desktop / Codex). */
  workspaceTarget?: ProviderTarget;
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
  workspaceTarget: ProviderTarget;
  providersTarget: ProviderTarget;
  proxyTarget: ProviderTarget;
  usagePeriod: UsagePeriod;
  heatmapPeriod: UsagePeriod;
  usageLogPage: number;
  usageLogTarget: ProviderTarget | "all";
  heatmapSource: ProviderTarget | "all";
  sessionsProvider: SessionProvider;
  setWorkspaceTarget: (target: ProviderTarget) => void;
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
  | "workspaceTarget"
  | "providersTarget"
  | "proxyTarget"
  | "usagePeriod"
  | "heatmapPeriod"
  | "usageLogTarget"
  | "heatmapSource"
  | "sessionsProvider"
> = {
  workspaceTarget: "claude_code",
  providersTarget: "claude_code",
  proxyTarget: "claude_code",
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

function sessionProviderFor(target: ProviderTarget): SessionProvider {
  return target === "codex" ? "codex" : "claude_code";
}

function skillCompatibleTarget(target: ProviderTarget): "claude_code" | "codex" {
  return target === "codex" ? "codex" : "claude_code";
}

export { skillCompatibleTarget };

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
  const providersTarget = isProviderTarget(stored.providersTarget)
    ? stored.providersTarget
    : DEFAULTS.providersTarget;
  const workspaceTarget = isProviderTarget(stored.workspaceTarget)
    ? stored.workspaceTarget
    : providersTarget;
  return {
    workspaceTarget,
    providersTarget,
    proxyTarget: isProviderTarget(stored.proxyTarget) ? stored.proxyTarget : workspaceTarget,
    usagePeriod,
    heatmapPeriod: isUsagePeriod(stored.heatmapPeriod) ? stored.heatmapPeriod : usagePeriod,
    usageLogTarget,
    heatmapSource: isUsageLogTarget(stored.heatmapSource) ? stored.heatmapSource : usageLogTarget,
    sessionsProvider: isSessionProvider(stored.sessionsProvider)
      ? stored.sessionsProvider
      : sessionProviderFor(workspaceTarget),
  };
}

function persistSlice(
  state: Pick<
    PagePreferencesState,
    | "workspaceTarget"
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
    workspaceTarget: state.workspaceTarget,
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
  setWorkspaceTarget: (workspaceTarget) => {
    set({
      workspaceTarget,
      providersTarget: workspaceTarget,
      proxyTarget: workspaceTarget,
    });
    persistSlice(get());
  },
  setProvidersTarget: (providersTarget) => {
    set({ providersTarget, workspaceTarget: providersTarget });
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
