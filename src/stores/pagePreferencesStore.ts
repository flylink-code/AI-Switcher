import { create } from "zustand";
import type { ProviderTarget, SessionProvider } from "@/types/backend";
import type { UsageSourceFilter } from "@/components/UsageSourceIcons";
import { coerceUiAgent, filterUiAgents, isAgentUiEnabled } from "@/lib/agentVisibility";
import { USAGE_PERIOD_VALUES, type UsagePeriod } from "@/utils/usagePeriod";

const STORAGE_KEY = "cs.pagePreferences";

export type GatewaySection = "service" | "routing" | "upstreams" | "logs";

const GATEWAY_SECTIONS: GatewaySection[] = ["service", "routing", "upstreams", "logs"];

interface PersistedPagePreferences {
  /** Legacy global target; only read as a migration fallback for the
   * per-page targets below. Never written anymore. */
  workspaceTarget?: ProviderTarget;
  providersTarget?: ProviderTarget;
  /** Independent proxy-page target. */
  proxyTarget?: ProviderTarget;
  gatewayTab?: "smart" | "antigravity";
  gatewaySection?: GatewaySection;
  gatewayProfileId?: string;
  gatewaySnippetVisible?: boolean;
  usagePeriod?: UsagePeriod;
  /** Providers heatmap period; falls back to usagePeriod on first load. */
  heatmapPeriod?: UsagePeriod;
  usageLogTarget?: UsageSourceFilter;
  heatmapSource?: UsageSourceFilter;
  sessionsProvider?: SessionProvider;
  workbenchView?: "providers" | "usage";
  visibleAgents?: ProviderTarget[];
  agQuotaViewMode?: "all" | "5h" | "7d";
}

interface PagePreferencesState {
  visibleAgents: ProviderTarget[];
  workspaceTarget: ProviderTarget;
  providersTarget: ProviderTarget;
  proxyTarget: ProviderTarget;
  gatewayTab: "smart" | "antigravity";
  gatewaySection: GatewaySection;
  gatewayProfileId: string;
  gatewaySnippetVisible: boolean;
  usagePeriod: UsagePeriod;
  heatmapPeriod: UsagePeriod;
  usageLogPage: number;
  usageLogTarget: UsageSourceFilter;
  heatmapSource: UsageSourceFilter;
  sessionsProvider: SessionProvider;
  workbenchView: "providers" | "usage";
  agQuotaViewMode: "all" | "5h" | "7d";
  setVisibleAgents: (agents: ProviderTarget[]) => void;
  setWorkspaceTarget: (target: ProviderTarget) => void;
  setProvidersTarget: (target: ProviderTarget) => void;
  setProxyTarget: (target: ProviderTarget) => void;
  setGatewayTab: (tab: "smart" | "antigravity") => void;
  setGatewaySection: (section: GatewaySection) => void;
  setGatewayProfileId: (profileId: string) => void;
  setGatewaySnippetVisible: (visible: boolean) => void;
  setUsagePeriod: (period: UsagePeriod) => void;
  setHeatmapPeriod: (period: UsagePeriod) => void;
  setUsageLogPage: (page: number) => void;
  setUsageLogTarget: (target: UsageSourceFilter) => void;
  setHeatmapSource: (target: UsageSourceFilter) => void;
  setSessionsProvider: (provider: SessionProvider) => void;
  setWorkbenchView: (view: "providers" | "usage") => void;
  setAgQuotaViewMode: (mode: "all" | "5h" | "7d") => void;
}

const DEFAULTS: Pick<
  PagePreferencesState,
  | "visibleAgents"
  | "workspaceTarget"
  | "providersTarget"
  | "proxyTarget"
  | "gatewayTab"
  | "gatewaySection"
  | "gatewayProfileId"
  | "gatewaySnippetVisible"
  | "usagePeriod"
  | "heatmapPeriod"
  | "usageLogTarget"
  | "heatmapSource"
  | "sessionsProvider"
  | "workbenchView"
  | "agQuotaViewMode"
> = {
  visibleAgents: ["claude_code", "codex", "opencode", "pi", "cline"],
  workspaceTarget: "claude_code",
  providersTarget: "claude_code",
  proxyTarget: "claude_code",
  gatewayTab: "smart",
  gatewaySection: "service",
  gatewayProfileId: "gprof_shared",
  gatewaySnippetVisible: true,
  usagePeriod: 365,
  heatmapPeriod: 365,
  usageLogTarget: "all",
  heatmapSource: "all",
  sessionsProvider: "claude_code",
  workbenchView: "providers",
  agQuotaViewMode: "all",
};

function isProviderTarget(value: unknown): value is ProviderTarget {
  return value === "claude_code" || value === "claude_desktop" || value === "codex" || value === "opencode" || value === "pi" || value === "dsh" || value === "cline";
}

function isSessionProvider(value: unknown): value is SessionProvider {
  return value === "claude_code" || value === "codex" || value === "opencode" || value === "pi" || value === "dsh" || value === "cline";
}

function isUsagePeriod(value: unknown): value is UsagePeriod {
  return USAGE_PERIOD_VALUES.some((period) => period === value);
}

function isGatewaySection(value: unknown): value is GatewaySection {
  return GATEWAY_SECTIONS.some((section) => section === value);
}

function isUsageLogTarget(value: unknown): value is UsageSourceFilter {
  return value === "all" || value === "antigravity" || isProviderTarget(value);
}

function coerceUsageFilter(
  value: UsageSourceFilter,
  visible: readonly ProviderTarget[],
): UsageSourceFilter {
  if (value === "all" || value === "antigravity") {
    return value;
  }
  if (!isAgentUiEnabled(value) || !visible.includes(value)) {
    return coerceUiAgent("claude_code", visible);
  }
  return value;
}

function sessionProviderFor(target: ProviderTarget): SessionProvider {
  if (target === "codex") return "codex";
  if (target === "opencode") return "opencode";
  if (target === "pi") return "pi";
  if (target === "dsh") return "dsh";
  if (target === "cline") return "cline";
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
  const filteredVisible = filterUiAgents(
    rawVisible && rawVisible.length > 0 ? rawVisible : DEFAULTS.visibleAgents,
  );
  const visibleAgents = filteredVisible.length > 0 ? filteredVisible : [...DEFAULTS.visibleAgents];

  const usagePeriod = isUsagePeriod(stored.usagePeriod) ? stored.usagePeriod : DEFAULTS.usagePeriod;
  const usageLogTarget = coerceUsageFilter(
    isUsageLogTarget(stored.usageLogTarget) ? stored.usageLogTarget : DEFAULTS.usageLogTarget,
    visibleAgents,
  );
  let providersTarget = isProviderTarget(stored.providersTarget)
    ? stored.providersTarget
    : isProviderTarget(stored.workspaceTarget)
      ? stored.workspaceTarget
      : DEFAULTS.providersTarget;
  providersTarget = coerceUiAgent(providersTarget, visibleAgents);

  let workspaceTarget = isProviderTarget(stored.workspaceTarget)
    ? stored.workspaceTarget
    : providersTarget;
  workspaceTarget = coerceUiAgent(workspaceTarget, visibleAgents);

  let proxyTarget = isProviderTarget(stored.proxyTarget)
    ? stored.proxyTarget
    : isProviderTarget(stored.workspaceTarget)
      ? stored.workspaceTarget
      : providersTarget;
  proxyTarget = coerceUiAgent(proxyTarget, visibleAgents);

  let sessionsProvider = isSessionProvider(stored.sessionsProvider)
    ? stored.sessionsProvider
    : sessionProviderFor(providersTarget);
  if (!visibleAgents.includes(sessionsProvider as ProviderTarget)) {
    sessionsProvider = (visibleAgents.find(isSessionProvider) ?? visibleAgents[0]) as SessionProvider;
  }

  const agQuotaViewMode =
    stored.agQuotaViewMode === "5h" || stored.agQuotaViewMode === "7d" || stored.agQuotaViewMode === "all"
      ? stored.agQuotaViewMode
      : DEFAULTS.agQuotaViewMode;

  const gatewayTab: "smart" | "antigravity" =
    stored.gatewayTab === "antigravity" ? "antigravity" : "smart";
  const gatewaySection = isGatewaySection(stored.gatewaySection)
    ? stored.gatewaySection
    : DEFAULTS.gatewaySection;
  const gatewaySnippetVisible = stored.gatewaySnippetVisible === false
    ? false
    : DEFAULTS.gatewaySnippetVisible;
  const gatewayProfileId =
    typeof stored.gatewayProfileId === "string" && stored.gatewayProfileId.trim()
      ? stored.gatewayProfileId.trim()
      : DEFAULTS.gatewayProfileId;

  return {
    visibleAgents,
    workspaceTarget,
    providersTarget,
    proxyTarget,
    gatewayTab,
    gatewaySection,
    gatewayProfileId,
    gatewaySnippetVisible,
    usagePeriod,
    heatmapPeriod: isUsagePeriod(stored.heatmapPeriod) ? stored.heatmapPeriod : usagePeriod,
    usageLogTarget,
    heatmapSource: coerceUsageFilter(
      isUsageLogTarget(stored.heatmapSource) ? stored.heatmapSource : usageLogTarget,
      visibleAgents,
    ),
    sessionsProvider,
    workbenchView: (stored.workbenchView === "usage" ? "usage" : "providers") as "providers" | "usage",
    agQuotaViewMode,
  };
}

function persistSlice(
  state: Pick<
    PagePreferencesState,
    | "visibleAgents"
    | "workspaceTarget"
    | "providersTarget"
    | "proxyTarget"
    | "gatewayTab"
    | "gatewaySection"
    | "gatewayProfileId"
    | "gatewaySnippetVisible"
    | "usagePeriod"
    | "heatmapPeriod"
    | "usageLogTarget"
    | "heatmapSource"
    | "sessionsProvider"
    | "workbenchView"
    | "agQuotaViewMode"
  >,
) {
  writePersisted({
    visibleAgents: state.visibleAgents,
    workspaceTarget: state.workspaceTarget,
    providersTarget: state.providersTarget,
    proxyTarget: state.proxyTarget,
    gatewayTab: state.gatewayTab,
    gatewaySection: state.gatewaySection,
    gatewayProfileId: state.gatewayProfileId,
    gatewaySnippetVisible: state.gatewaySnippetVisible,
    usagePeriod: state.usagePeriod,
    heatmapPeriod: state.heatmapPeriod,
    usageLogTarget: state.usageLogTarget,
    heatmapSource: state.heatmapSource,
    sessionsProvider: state.sessionsProvider,
    workbenchView: state.workbenchView,
    agQuotaViewMode: state.agQuotaViewMode,
  });
}

export const usePagePreferencesStore = create<PagePreferencesState>((set, get) => ({
  ...initialState(),
  usageLogPage: 0,
  setVisibleAgents: (visibleAgents) => {
    const nextVisible = filterUiAgents(visibleAgents);
    if (nextVisible.length === 0) return;
    const current = get();
    const workspaceTarget = coerceUiAgent(current.workspaceTarget, nextVisible);
    const providersTarget = coerceUiAgent(current.providersTarget, nextVisible);
    const proxyTarget = coerceUiAgent(current.proxyTarget, nextVisible);
    let sessionsProvider = current.sessionsProvider;
    if (!nextVisible.includes(sessionsProvider as ProviderTarget)) {
      sessionsProvider = (nextVisible.find(isSessionProvider) ?? nextVisible[0]) as SessionProvider;
    }
    set({
      visibleAgents: nextVisible,
      workspaceTarget,
      providersTarget,
      proxyTarget,
      sessionsProvider,
      usageLogTarget: coerceUsageFilter(current.usageLogTarget, nextVisible),
      heatmapSource: coerceUsageFilter(current.heatmapSource, nextVisible),
    });
    persistSlice(get());
  },
  setWorkspaceTarget: (workspaceTarget) => {
    const visible = get().visibleAgents;
    set({ workspaceTarget: coerceUiAgent(workspaceTarget, visible) });
    persistSlice(get());
  },
  setProvidersTarget: (providersTarget) => {
    const visible = get().visibleAgents;
    set({ providersTarget: coerceUiAgent(providersTarget, visible) });
    persistSlice(get());
  },
  setProxyTarget: (proxyTarget) => {
    const visible = get().visibleAgents;
    set({ proxyTarget: coerceUiAgent(proxyTarget, visible) });
    persistSlice(get());
  },
  setGatewayTab: (gatewayTab) => {
    set({ gatewayTab });
    persistSlice(get());
  },
  setGatewaySection: (gatewaySection) => {
    set({ gatewaySection });
    persistSlice(get());
  },
  setGatewayProfileId: (gatewayProfileId) => {
    set({ gatewayProfileId });
    persistSlice(get());
  },
  setGatewaySnippetVisible: (gatewaySnippetVisible) => {
    set({ gatewaySnippetVisible });
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
    set({ usageLogTarget: coerceUsageFilter(usageLogTarget, get().visibleAgents) });
    persistSlice(get());
  },
  setHeatmapSource: (heatmapSource) => {
    set({ heatmapSource: coerceUsageFilter(heatmapSource, get().visibleAgents) });
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
  setAgQuotaViewMode: (agQuotaViewMode) => {
    set({ agQuotaViewMode });
    persistSlice(get());
  },
}));
