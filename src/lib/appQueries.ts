import { queryOptions } from "@tanstack/react-query";
import {
  getAutostartConfig,
  getClaudeCodeVersion,
  getCodexCliVersion,
  getDshCliVersion,
  getOpenCodeCliVersion,
  getOpenCodeDesktopStatus,
  getClaudeDesktopAppStatus,
  getPiCliVersion,
  getNodeRuntimeStatus,
  getCloseBehavior,
  getDbInfo,
  getDataRoot,
  getDesktopLocalizationStatus,
  getLocalizationHubStatus,
  checkLocalizationUpstream,
  getLogMaintenancePolicy,
  getPaths,
  getProxyStatus,
  getManagedAppsRuntimeStatus,
  getUsageDashboard,
  listMcpServers,
  getMcpOauthStatus,
  getMcpDesktopConflictStatus,
  listModelPricing,
  listPrompts,
  listProviders,
  listProxyRequestLogs,
  listSkills,
  scanUnmanagedSkills,
  listAgents,
  listCodexPlugins,
  listCodexPluginCatalog,
  listClaudePlugins,
  listClaudePluginCatalog,
  listClaudePluginMarketplaces,
  listCodexPluginMarketplaces,
  getSkillRepositorySnapshot,
  listSkillRepositories,
  readLivePrompt,
} from "@/services/api";
import type { PromptTarget, ProviderTarget, SkillTarget } from "@/types/backend";
import type { UsageSourceFilter } from "@/components/UsageSourceIcons";
import type { UsagePeriod } from "@/utils/usagePeriod";
import { usagePeriodToQuery } from "@/utils/usagePeriod";

export const providerListOptions = (target: ProviderTarget) =>
  queryOptions({
    queryKey: ["providers", target] as const,
    queryFn: () => listProviders(target),
    staleTime: 30_000,
  });

export const proxyStatusOptions = (target: ProviderTarget) =>
  queryOptions({
    queryKey: ["proxy-status", target] as const,
    queryFn: () => getProxyStatus(target),
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnMount: false,
  });

export const managedAppsRuntimeStatusOptions = queryOptions({
  queryKey: ["managed-apps-runtime-status"] as const,
  queryFn: getManagedAppsRuntimeStatus,
  staleTime: 2_000,
  refetchInterval: 3_000,
});

export const mcpServersOptions = queryOptions({
  queryKey: ["mcp-servers"] as const,
  queryFn: listMcpServers,
  staleTime: 30_000,
});

export const mcpOauthStatusOptions = queryOptions({
  queryKey: ["mcp-oauth-status"] as const,
  queryFn: getMcpOauthStatus,
  staleTime: 15_000,
});

export const mcpDesktopConflictOptions = queryOptions({
  queryKey: ["mcp-desktop-conflict"] as const,
  queryFn: getMcpDesktopConflictStatus,
  staleTime: 15_000,
});

export const promptsOverviewOptions = (target: PromptTarget = "claude_code") => queryOptions({
  queryKey: ["prompts-overview", target] as const,
  queryFn: async () => {
    const [items, livePrompt] = await Promise.all([listPrompts(target), readLivePrompt(target)]);
    return { items, livePrompt };
  },
  staleTime: 30_000,
});

export const skillsOptions = (target: SkillTarget = "claude_code") => queryOptions({
  queryKey: ["skills", target] as const,
  queryFn: () => listSkills(target),
  staleTime: 30_000,
});

export const unmanagedSkillsOptions = (target: SkillTarget = "claude_code") =>
  queryOptions({
    queryKey: ["unmanagedSkills", target] as const,
    queryFn: () => scanUnmanagedSkills(target),
    staleTime: 30_000,
  });

export const agentsOptions = queryOptions({
  queryKey: ["agents"] as const,
  queryFn: listAgents,
  staleTime: 30_000,
});

export const codexPluginsOptions = queryOptions({
  queryKey: ["codexPlugins"] as const,
  queryFn: listCodexPlugins,
  staleTime: 30_000,
});

export const codexPluginMarketplacesOptions = queryOptions({
  queryKey: ["codexPluginMarketplaces"] as const,
  queryFn: listCodexPluginMarketplaces,
  staleTime: 30_000,
});

export const codexPluginCatalogOptions = queryOptions({
  queryKey: ["codexPluginCatalog"] as const,
  queryFn: listCodexPluginCatalog,
  staleTime: 30_000,
});

export const claudePluginsOptions = queryOptions({
  queryKey: ["claudePlugins"] as const,
  queryFn: listClaudePlugins,
  staleTime: 30_000,
});

export const claudePluginCatalogOptions = queryOptions({
  queryKey: ["claudePluginCatalog"] as const,
  queryFn: listClaudePluginCatalog,
  staleTime: 30_000,
});

export const claudePluginMarketplacesOptions = queryOptions({
  queryKey: ["claudePluginMarketplaces"] as const,
  queryFn: listClaudePluginMarketplaces,
  staleTime: 30_000,
});

export const skillRepositoryOptions = queryOptions({
  queryKey: ["skillRepository"] as const,
  queryFn: getSkillRepositorySnapshot,
});

export const skillRepositoriesOptions = queryOptions({
  queryKey: ["skillRepositories"] as const,
  queryFn: listSkillRepositories,
  staleTime: 30_000,
});

export const usageDashboardOptions = (
  period: UsagePeriod,
  target: UsageSourceFilter,
) =>
  queryOptions({
    queryKey: ["usage-dashboard", period, target] as const,
    queryFn: () => getUsageDashboard(usagePeriodToQuery(period), target),
    staleTime: 30_000,
  });

export const usageLogsOptions = (
  period: UsagePeriod,
  logPage: number,
  target: UsageSourceFilter,
  onlyFailures?: boolean,
) =>
  queryOptions({
    queryKey: ["usage-logs", period, logPage, target, onlyFailures] as const,
    queryFn: () =>
      listProxyRequestLogs({
        ...usagePeriodToQuery(period),
        page: logPage,
        pageSize: 20,
        targetApp: target === "all" ? undefined : target,
        onlyFailures: onlyFailures || undefined,
      }),
    staleTime: 15_000,
  });

export const usageMetaOptions = queryOptions({
  queryKey: ["usage-meta"] as const,
  queryFn: async () => {
    const [pricing, maintenancePolicy] = await Promise.all([
      listModelPricing(),
      getLogMaintenancePolicy(),
    ]);
    return { pricing, maintenancePolicy };
  },
  staleTime: 60_000,
});

/** @deprecated Prefer usageDashboardOptions + usageLogsOptions + usageMetaOptions */
export const usageOverviewOptions = (
  period: UsagePeriod,
  logPage: number,
  target: UsageSourceFilter,
) =>
  queryOptions({
    queryKey: ["usage-overview", period, logPage, target] as const,
    queryFn: async () => {
      const range = usagePeriodToQuery(period);
      const [dashboard, pricing, maintenancePolicy, requestLogs] = await Promise.all([
        getUsageDashboard(range, target),
        listModelPricing(),
        getLogMaintenancePolicy(),
        listProxyRequestLogs({
          ...range,
          page: logPage,
          pageSize: 20,
          targetApp: target === "all" ? undefined : target,
        }),
      ]);
      return { dashboard, pricing, maintenancePolicy, requestLogs };
    },
    staleTime: 10_000,
  });

/** Lightweight trend-only fetch for Providers calendar (no logs/pricing). */
export const usageTrendOptions = (
  period: UsagePeriod,
  target: UsageSourceFilter = "all",
) =>
  queryOptions({
    queryKey: ["usage-trend", period, target] as const,
    queryFn: () => getUsageDashboard(usagePeriodToQuery(period), target),
    staleTime: 60_000,
    refetchOnMount: false,
  });

export const environmentOptions = queryOptions({
  queryKey: ["environment", "paths-db"] as const,
  queryFn: async () => {
    const [paths, db, dataRoot] = await Promise.all([getPaths(), getDbInfo(), getDataRoot()]);
    return { paths, db, dataRoot };
  },
  staleTime: 5 * 60_000,
});

export const autostartOptions = queryOptions({
  queryKey: ["environment", "autostart"] as const,
  queryFn: getAutostartConfig,
  staleTime: 60_000,
});

export const closeBehaviorOptions = queryOptions({
  queryKey: ["environment", "close-behavior"] as const,
  queryFn: getCloseBehavior,
  staleTime: Number.POSITIVE_INFINITY,
});

export const localizationOptions = queryOptions({
  queryKey: ["desktop-localization-status"] as const,
  queryFn: getDesktopLocalizationStatus,
  staleTime: 60_000,
});

export const localizationHubOptions = queryOptions({
  queryKey: ["localization-hub-status"] as const,
  queryFn: getLocalizationHubStatus,
  staleTime: 60_000,
});

export const localizationUpstreamOptions = queryOptions({
  queryKey: ["localization-upstream-status"] as const,
  queryFn: checkLocalizationUpstream,
  staleTime: 60_000,
  retry: false,
});

export const localClaudeVersionOptions = queryOptions({
  queryKey: ["claude-code-version", "local"] as const,
  queryFn: () => getClaudeCodeVersion(false),
  staleTime: 5 * 60_000,
});

export const claudeVersionOptions = queryOptions({
  queryKey: ["claude-code-version", "latest"] as const,
  queryFn: () => getClaudeCodeVersion(true),
  staleTime: 5 * 60_000,
});

export const claudeDesktopAppOptions = queryOptions({
  queryKey: ["claude-desktop-app"] as const,
  queryFn: getClaudeDesktopAppStatus,
  staleTime: 60_000,
});

export const localCodexCliVersionOptions = queryOptions({
  queryKey: ["codex-cli-version", "local"] as const,
  queryFn: () => getCodexCliVersion(false),
  staleTime: 5 * 60_000,
});

export const codexCliVersionOptions = queryOptions({
  queryKey: ["codex-cli-version", "latest"] as const,
  queryFn: () => getCodexCliVersion(true),
  staleTime: 5 * 60_000,
});

export const localOpenCodeCliVersionOptions = queryOptions({
  queryKey: ["opencode-cli-version", "local"] as const,
  queryFn: () => getOpenCodeCliVersion(false),
  staleTime: 5 * 60_000,
});

export const opencodeCliVersionOptions = queryOptions({
  queryKey: ["opencode-cli-version", "latest"] as const,
  queryFn: () => getOpenCodeCliVersion(true),
  staleTime: 5 * 60_000,
});

export const localPiCliVersionOptions = queryOptions({
  queryKey: ["pi-cli-version", "local"] as const,
  queryFn: () => getPiCliVersion(false),
  staleTime: 5 * 60_000,
});

export const piCliVersionOptions = queryOptions({
  queryKey: ["pi-cli-version", "latest"] as const,
  queryFn: () => getPiCliVersion(true),
  staleTime: 5 * 60_000,
});

export const localDshCliVersionOptions = queryOptions({
  queryKey: ["dsh-cli-version", "local"] as const,
  queryFn: () => getDshCliVersion(false),
  staleTime: 5 * 60_000,
});

export const dshCliVersionOptions = queryOptions({
  queryKey: ["dsh-cli-version", "latest"] as const,
  queryFn: () => getDshCliVersion(true),
  staleTime: 5 * 60_000,
});

export const opencodeDesktopStatusOptions = queryOptions({
  queryKey: ["opencode-desktop-status"] as const,
  queryFn: getOpenCodeDesktopStatus,
  staleTime: 60_000,
});

export const nodeRuntimeStatusOptions = queryOptions({
  queryKey: ["node-runtime-status"] as const,
  queryFn: getNodeRuntimeStatus,
  staleTime: 60_000,
});
