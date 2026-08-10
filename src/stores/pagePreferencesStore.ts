import { create } from "zustand";
import type { ProviderTarget, SessionProvider } from "@/types/backend";
import type { UsageSourceFilter } from "@/components/UsageSourceIcons";
import { USAGE_PERIOD_VALUES, type UsagePeriod } from "@/utils/usagePeriod";

const STORAGE_KEY = "cs.pagePreferences";

interface PersistedPagePreferences {
  /** Last providers-page target (also mirrored as workspaceTarget for legacy). */
  workspaceTarget?: ProviderTarget;
  providersTarget?: ProviderTarget;
  /** Independent proxy-page target. */
  proxyTarget?: ProviderTarget;
  usagePeriod?: UsagePeriod;
  /** Providers heatmap period; falls back to usagePeriod on first load. */
  heatmapPeriod?: UsagePeriod;
  usageLogTarget?: UsageSourceFilter;
  heatmapSource?: UsageSourceFilter;
  sessionsProvider?: SessionProvider;
  workbenchView?: "providers" | "usage";
}

interface PagePreferencesState {
  workspaceTarget: ProviderTarget;
  providersTarget: ProviderTarget;
  proxyTarget: ProviderTarget;
  usagePeriod: UsagePeriod;
  heatmapPeriod: UsagePeriod;
  usageLogPage: number;
  usageLogTarget: UsageSourceFilter;
  heatmapSource: UsageSourceFilter;
  sessionsProvider: SessionProvider;
  workbenchView: "providers" | "usage";
  setWorkspaceTarget: (target: ProviderTarget) => void;
  setProvidersTarget: (target: ProviderTarget) => void;
  setProxyTarget: (target: ProviderTarget) => void;
  setUsagePeriod: (period: UsagePeriod) => void;
  setHeatmapPeriod: (period: UsagePeriod) => void;
  setUsageLogPage: (page: number) => void;
  setUsageLogTarget: (target: UsageSourceFilter) => void;
  setHeatmapSource: (target: UsageSourceFilter) => void;
  setSessionsProvider: (provider: SessionProvider) => void;
  setWorkbenchView: (view: "providers" | "usage") => void;
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
  | "workbenchView"
> = {
  workspaceTarget: "claude_code",
  providersTarget: "claude_code",
  proxyTarget: "claude_code",
  usagePeriod: 365,
  heatmapPeriod: 365,
  usageLogTarget: "all",
  heatmapSource: "all",
  sessionsProvider: "claude_code",
  workbenchView: "providers",
};

function isProviderTarget(value: unknown): value is ProviderTarget {
  return value === "claude_code" || value === "claude_desktop" || value === "codex" || value === "opencode";
}

function isSessionProvider(value: unknown): value is SessionProvider {
  return value === "claude_code" || value === "codex" || value === "opencode";
}

function isUsagePeriod(value: unknown): value is UsagePeriod {
  return USAGE_PERIOD_VALUES.some((period) => period === value);
}

function isUsageLogTarget(value: unknown): value is UsageSourceFilter {
  return value === "all" || value === "antigravity" || isProviderTarget(value);
}

function sessionProviderFor(target: ProviderTarget): SessionProvider {
  if (target === "codex") return "codex";
  if (target === "opencode") return "opencode";
  return "claude_code";
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
    workbenchView: (stored.workbenchView === "usage" ? "usage" : "providers") as "providers" | "usage",
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
    | "workbenchView"
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
    workbenchView: state.workbenchView,
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
    // Keep workspaceTarget aligned for any leftover readers / header migration.
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
  setWorkbenchView: (workbenchView) => {
    set({ workbenchView });
    persistSlice(get());
  },
}));
