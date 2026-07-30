import { queryOptions } from "@tanstack/react-query";
import {
  getAutostartConfig,
  getClaudeCodeVersion,
  getCloseBehavior,
  getDbInfo,
  getDataRoot,
  getDesktopLocalizationStatus,
  getLocalizationHubStatus,
  getLogMaintenancePolicy,
  getPaths,
  getProxyStatus,
  getUsageDashboard,
  listMcpServers,
  listModelPricing,
  listPrompts,
  listProviders,
  listProxyRequestLogs,
  listSkills,
  getSkillRepositorySnapshot,
  readLivePrompt,
} from "@/services/api";
import type { PromptTarget, ProviderTarget, SkillTarget } from "@/types/backend";

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

export const mcpServersOptions = queryOptions({
  queryKey: ["mcp-servers"] as const,
  queryFn: listMcpServers,
  staleTime: 30_000,
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

export const skillRepositoryOptions = queryOptions({
  queryKey: ["skillRepository"] as const,
  queryFn: getSkillRepositorySnapshot,
});

export const usageDashboardOptions = (
  days: number,
  target: ProviderTarget | "all",
) =>
  queryOptions({
    queryKey: ["usage-dashboard", days, target] as const,
    queryFn: () => getUsageDashboard(days, target),
    staleTime: 30_000,
  });

export const usageLogsOptions = (
  days: number,
  logPage: number,
  target: ProviderTarget | "all",
) =>
  queryOptions({
    queryKey: ["usage-logs", days, logPage, target] as const,
    queryFn: () =>
      listProxyRequestLogs({
        days,
        page: logPage,
        pageSize: 20,
        targetApp: target === "all" ? undefined : target,
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
  days: number,
  logPage: number,
  target: ProviderTarget | "all",
) =>
  queryOptions({
    queryKey: ["usage-overview", days, logPage, target] as const,
    queryFn: async () => {
      const [dashboard, pricing, maintenancePolicy, requestLogs] = await Promise.all([
        getUsageDashboard(days, target),
        listModelPricing(),
        getLogMaintenancePolicy(),
        listProxyRequestLogs({
          days,
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
export const usageTrendOptions = (days: number) =>
  queryOptions({
    queryKey: ["usage-trend", days] as const,
    queryFn: () => getUsageDashboard(days, "all"),
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
