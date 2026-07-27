import { queryOptions } from "@tanstack/react-query";
import {
  getAutostartConfig,
  getClaudeCodeVersion,
  getDbInfo,
  getDesktopLocalizationStatus,
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
  readLivePrompt,
} from "@/services/api";
import type { ProviderTarget } from "@/types/backend";

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

export const promptsOverviewOptions = queryOptions({
  queryKey: ["prompts-overview"] as const,
  queryFn: async () => {
    const [items, livePrompt] = await Promise.all([listPrompts(), readLivePrompt()]);
    return { items, livePrompt };
  },
  staleTime: 30_000,
});

export const skillsOptions = queryOptions({
  queryKey: ["skills"] as const,
  queryFn: listSkills,
  staleTime: 30_000,
});

export const usageOverviewOptions = (
  days: number,
  logPage: number,
  target: ProviderTarget | "all",
) =>
  queryOptions({
    queryKey: ["usage-overview", days, logPage, target] as const,
    queryFn: async () => {
      const [dashboard, pricing, maintenancePolicy, requestLogs] = await Promise.all([
        getUsageDashboard(days),
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

export const environmentOptions = queryOptions({
  queryKey: ["environment", "paths-db"] as const,
  queryFn: async () => {
    const [paths, db] = await Promise.all([getPaths(), getDbInfo()]);
    return { paths, db };
  },
  staleTime: 5 * 60_000,
});

export const autostartOptions = queryOptions({
  queryKey: ["environment", "autostart"] as const,
  queryFn: getAutostartConfig,
  staleTime: 60_000,
});

export const localizationOptions = queryOptions({
  queryKey: ["desktop-localization-status"] as const,
  queryFn: getDesktopLocalizationStatus,
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
