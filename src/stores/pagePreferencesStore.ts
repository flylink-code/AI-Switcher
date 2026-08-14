import { create } from "zustand";
import type { ProviderTarget, SessionProvider } from "@/types/backend";
import type { UsageSourceFilter } from "@/components/UsageSourceIcons";
import { USAGE_PERIOD_VALUES, type UsagePeriod } from "@/utils/usagePeriod";

const STORAGE_KEY = "cs.pagePreferences";

interface PersistedPagePreferences {
  /** Legacy global target; only read as a migration fallback for the
   * per-page targets below. Never written anymore. */
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
  visibleAgents?: ProviderTarget[];
}

interface PagePreferencesState {
  visibleAgents: ProviderTarget[];
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
  setVisibleAgents: (agents: ProviderTarget[]) => void;
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
  | "visibleAgents"
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
  visibleAgents: ["claude_code", "claude_desktop", "codex", "opencode", "pi", "dsh"],
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
  return value === "claude_code" || value === "claude_desktop" || value === "codex" || value === "opencode" || value === "pi" || value === "dsh";
}

function isSessionProvider(value: unknown): value is SessionProvider {
  return value === "claude_code" || value === "codex" || value === "opencode" || value === "pi";
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
  if (target === "pi") return "pi";
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
  const rawVisible = Array.isArray(stored.visibleAgents)
    ? stored.visibleAgents.filter(isProviderTarget)
    : null;
  const visibleAgents = rawVisible && rawVisible.length > 0 ? rawVisible : DEFAULTS.visibleAgents;

  const usagePeriod = isUsagePeriod(stored.usagePeriod) ? stored.usagePeriod : DEFAULTS.usagePeriod;
  const usageLogTarget = isUsageLogTarget(stored.usageLogTarget)
    ? stored.usageLogTarget
    : DEFAULTS.usageLogTarget;
  let providersTarget = isProviderTarget(stored.providersTarget)
    ? stored.providersTarget
    : isProviderTarget(stored.workspaceTarget)
      ? stored.workspaceTarget
      : DEFAULTS.providersTarget;
  if (!visibleAgents.includes(providersTarget)) {
    providersTarget = visibleAgents[0];
  }

  let workspaceTarget = isProviderTarget(stored.workspaceTarget)
    ? stored.workspaceTarget
    : providersTarget;
  if (!visibleAgents.includes(workspaceTarget)) {
    workspaceTarget = visibleAgents[0];
  }

  let proxyTarget = isProviderTarget(stored.proxyTarget)
    ? stored.proxyTarget
    : isProviderTarget(stored.workspaceTarget)
      ? stored.workspaceTarget
      : providersTarget;
  if (!visibleAgents.includes(proxyTarget)) {
    proxyTarget = visibleAgents[0];
  }

  let sessionsProvider = isSessionProvider(stored.sessionsProvider)
    ? stored.sessionsProvider
    : sessionProviderFor(providersTarget);
  if (!visibleAgents.includes(sessionsProvider as ProviderTarget)) {
    sessionsProvider = (visibleAgents.find(isSessionProvider) ?? visibleAgents[0]) as SessionProvider;
  }

  return {
    visibleAgents,
    workspaceTarget,
    providersTarget,
    proxyTarget,
    usagePeriod,
    heatmapPeriod: isUsagePeriod(stored.heatmapPeriod) ? stored.heatmapPeriod : usagePeriod,
    usageLogTarget,
    heatmapSource: isUsageLogTarget(stored.heatmapSource) ? stored.heatmapSource : usageLogTarget,
    sessionsProvider,
    workbenchView: (stored.workbenchView === "usage" ? "usage" : "providers") as "providers" | "usage",
  };
}

function persistSlice(
  state: Pick<
    PagePreferencesState,
    | "visibleAgents"
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
    visibleAgents: state.visibleAgents,
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
  setVisibleAgents: (visibleAgents) => {
    if (!visibleAgents || visibleAgents.length === 0) return;
    const current = get();
    let workspaceTarget = current.workspaceTarget;
    if (!visibleAgents.includes(workspaceTarget)) {
      workspaceTarget = visibleAgents[0];
    }
    let providersTarget = current.providersTarget;
    if (!visibleAgents.includes(providersTarget)) {
      providersTarget = visibleAgents[0];
    }
    let proxyTarget = current.proxyTarget;
    if (!visibleAgents.includes(proxyTarget)) {
      proxyTarget = visibleAgents[0];
    }
    let sessionsProvider = current.sessionsProvider;
    if (!visibleAgents.includes(sessionsProvider as ProviderTarget)) {
      sessionsProvider = (visibleAgents.find(isSessionProvider) ?? visibleAgents[0]) as SessionProvider;
    }
    set({
      visibleAgents,
      workspaceTarget,
      providersTarget,
      proxyTarget,
      sessionsProvider,
    });
    persistSlice(get());
  },
  setWorkspaceTarget: (workspaceTarget) => {
    set({ workspaceTarget });
    persistSlice(get());
  },
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
  setWorkbenchView: (workbenchView) => {
    set({ workbenchView });
    persistSlice(get());
  },
}));
