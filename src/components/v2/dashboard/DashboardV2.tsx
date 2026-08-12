import React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import CloudServerOutlined from "@ant-design/icons/es/icons/CloudServerOutlined";
import LaptopOutlined from "@ant-design/icons/es/icons/LaptopOutlined";
import LineChartOutlined from "@ant-design/icons/es/icons/LineChartOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import { MetricCard } from "./MetricCard";
import { CurrentRuntimeCard } from "./CurrentRuntimeCard";
import { AttentionCard, type AttentionIssue } from "./AttentionCard";
import { UsageCalendar } from "@/components/UsageCalendar";
import { LABEL_KEYS } from "@/components/AgentTargetSwitcher";
import {
  managedAppsRuntimeStatusOptions,
  providerListOptions,
  proxyStatusOptions,
  usageDashboardOptions,
  usageTrendOptions,
} from "@/lib/appQueries";
import { useThemeStore } from "@/stores/themeStore";
import type { PageKey } from "@/lib/pageRegistry";
import type { ProviderTarget } from "@/types/backend";
import { formatCompactNumber } from "@/utils/formatCompact";

export interface DashboardV2Props {
  onNavigate: (key: PageKey) => void;
}

const PROXY_TARGETS: ProviderTarget[] = ["claude_code", "claude_desktop", "codex", "opencode"];

export const DashboardV2: React.FC<DashboardV2Props> = ({ onNavigate }) => {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const isDark = resolvedTheme === "dark";

  const runtimeQuery = useQuery(managedAppsRuntimeStatusOptions);
  const proxyQueries = [
    useQuery(proxyStatusOptions("claude_code")),
    useQuery(proxyStatusOptions("claude_desktop")),
    useQuery(proxyStatusOptions("codex")),
    useQuery(proxyStatusOptions("opencode")),
  ];
  const providerQueries = [
    useQuery(providerListOptions("claude_code")),
    useQuery(providerListOptions("claude_desktop")),
    useQuery(providerListOptions("codex")),
    useQuery(providerListOptions("opencode")),
  ];
  const dashboardQuery = useQuery(usageDashboardOptions("24h", "all"));
  const yearTrendQuery = useQuery(usageTrendOptions(365, "all"));

  const providers = providerQueries[0].data ?? [];
  const activeProvider = providers.find((p) => p.isCurrent) ?? providers[0];
  // Prefer a running agent proxy for the runtime card; fall back to Claude Code.
  const runningProxyIndex = proxyQueries.findIndex((q) => q.data?.running);
  const runtimeProxyIndex = runningProxyIndex >= 0 ? runningProxyIndex : 0;
  const proxyStatus = proxyQueries[runtimeProxyIndex].data;
  const runtimeProviders = providerQueries[runtimeProxyIndex].data ?? providers;
  const runtimeProvider =
    runtimeProviders.find((p) => p.isCurrent) ?? runtimeProviders[0] ?? activeProvider;
  const providerCount = providerQueries.reduce((sum, q) => sum + (q.data?.length ?? 0), 0);
  const providersLoaded = providerQueries.every((q) => q.data !== undefined);
  const proxyRunningCount = proxyQueries.filter((q) => q.data?.running).length;
  const appStatus = runtimeQuery.data;
  const agentRunningCount = appStatus
    ? [appStatus.claudeCode, appStatus.claudeDesktop, appStatus.codex, appStatus.opencode].filter(Boolean)
        .length
    : 0;

  const summary = dashboardQuery.data?.summary;
  const requestCount = summary?.requestCount ?? 0;
  const totalTokens =
    (summary?.inputTokens ?? 0) +
    (summary?.cacheReadInputTokens ?? 0) +
    (summary?.cacheCreationInputTokens ?? 0) +
    (summary?.outputTokens ?? 0);
  const successRate =
    requestCount > 0
      ? Number((((summary?.successfulRequestCount ?? 0) / requestCount) * 100).toFixed(1))
      : null;

  const issues: AttentionIssue[] = [];
  if (successRate != null && successRate < 95) {
    issues.push({
      id: "success-rate",
      message: t("workbench.attentionSuccessRate", {
        rate: successRate,
        defaultValue: "成功率下降至 {{rate}}%",
      }),
      level: "warning",
      page: "usage",
      action: t("workbench.viewUsageLink", { defaultValue: "查看用量" }),
    });
  }
  PROXY_TARGETS.forEach((target, index) => {
    if (proxyQueries[index].data?.phase === "error") {
      issues.push({
        id: `proxy-${target}`,
        message: t("workbench.attentionProxyError", {
          agent: t(LABEL_KEYS[target]),
          defaultValue: "{{agent}} 代理异常",
        }),
        level: "error",
        page: "proxy",
        action: t("workbench.viewProxyLink", { defaultValue: "查看代理" }),
      });
    }
  });
  if (providersLoaded && providerCount === 0) {
    issues.push({
      id: "no-providers",
      message: t("workbench.attentionNoProviders", { defaultValue: "尚未配置供应商" }),
      level: "warning",
      page: "providers",
      action: t("workbench.viewProvidersLink", { defaultValue: "查看供应商" }),
    });
  }

  const healthy = issues.length === 0;

  const handleRefresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["providers"] });
    void queryClient.invalidateQueries({ queryKey: ["proxy-status"] });
    void queryClient.invalidateQueries({ queryKey: ["managed-apps-runtime-status"] });
    void queryClient.invalidateQueries({ queryKey: ["usage-dashboard"] });
    void queryClient.invalidateQueries({ queryKey: ["usage-trend"] });
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "20px" }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          paddingBottom: "4px",
        }}
      >
        <div>
          <h1
            style={{
              fontSize: "22px",
              fontWeight: 700,
              margin: 0,
              color: isDark ? "#F2F4F7" : "#111827",
              letterSpacing: "-0.01em",
            }}
          >
            {t("workbench.overviewTitle", { defaultValue: "运行概览" })}
          </h1>
          <p
            style={{
              fontSize: "13px",
              margin: "4px 0 0 0",
              color: isDark ? "#9CA3AF" : "#6B7280",
            }}
          >
            {healthy
              ? t("workbench.stripHealthy", { defaultValue: "系统运行正常" })
              : t("workbench.stripAttention", { defaultValue: "需要关注" })}
            {" · "}
            {t("workbench.stripProxies", {
              count: proxyRunningCount,
              defaultValue: "{{count}} 个代理运行",
            })}
          </p>
        </div>

        <button
          type="button"
          onClick={handleRefresh}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: "6px",
            padding: "6px 12px",
            borderRadius: "8px",
            border: `1px solid ${isDark ? "#242D38" : "#E2E8F0"}`,
            backgroundColor: isDark ? "#1A212B" : "#FFFFFF",
            color: isDark ? "#D1D5DB" : "#475569",
            fontSize: "12px",
            fontWeight: 500,
            cursor: "pointer",
          }}
        >
          <ReloadOutlined />
          <span>{t("common.refresh", { defaultValue: "刷新状态" })}</span>
        </button>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(220px, 1fr))",
          gap: "14px",
        }}
      >
        <MetricCard
          title={t("navigation.providers", { defaultValue: "供应商" })}
          value={providersLoaded ? providerCount : "…"}
          subtitle={t("workbench.stripProviders", {
            count: providerCount,
            defaultValue: "{{count}} 个供应商",
          })}
          icon={<CloudServerOutlined />}
          statusColor="#3B82F6"
        />
        <MetricCard
          title={t("workbench.activeAgents", { defaultValue: "活跃 Agent" })}
          value={`${agentRunningCount} / 4`}
          subtitle={t("workbench.stripAgents", {
            running: agentRunningCount,
            idle: 4 - agentRunningCount,
            defaultValue: "{{running}} 运行 / {{idle}} 空闲",
          })}
          icon={<LaptopOutlined />}
          statusColor="#22C55E"
        />
        <MetricCard
          title={t("workbench.tokens24h", { defaultValue: "最近 24h Token" })}
          value={formatCompactNumber(totalTokens)}
          subtitle={t("workbench.requests24h", {
            count: formatCompactNumber(requestCount),
            defaultValue: "{{count}} 次请求",
          })}
          icon={<ThunderboltOutlined />}
          statusColor="#8B5CF6"
        />
        <MetricCard
          title={t("usage.successRate", { defaultValue: "请求成功率" })}
          value={successRate != null ? `${successRate}%` : "—"}
          subtitle={t("workbench.last24hAgg", { defaultValue: "近 24 小时聚合" })}
          icon={<LineChartOutlined />}
          statusColor={successRate != null && successRate < 95 ? "#F59E0B" : "#10B981"}
        />
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(360px, 1fr))",
          gap: "16px",
        }}
      >
        <CurrentRuntimeCard
          onNavigate={onNavigate}
          activeProviderName={runtimeProvider?.name ?? "—"}
          activeModelName={runtimeProvider?.model ?? "—"}
          proxyPort={proxyStatus?.port ?? 0}
          isRunning={proxyStatus?.running ?? false}
        />
        <AttentionCard
          issues={issues}
          providerCount={providerCount}
          onNavigate={onNavigate}
        />
      </div>

      <div
        style={{
          borderRadius: "14px",
          padding: "20px",
          backgroundColor: isDark ? "#1A212B" : "#FFFFFF",
          border: `1px solid ${isDark ? "#242D38" : "#E8ECF1"}`,
          display: "flex",
          flexDirection: "column",
          gap: "12px",
        }}
      >
        <div>
          <h3
            style={{
              margin: 0,
              fontSize: "15px",
              fontWeight: 600,
              color: isDark ? "#F2F4F7" : "#111827",
            }}
          >
            {t("workbench.yearHeatmapTitle", { defaultValue: "过去一年使用强度" })}
          </h3>
          <p style={{ margin: "2px 0 0 0", fontSize: "12px", color: isDark ? "#9CA3AF" : "#6B7280" }}>
            {t("workbench.yearHeatmapHint", {
              defaultValue: "每日 Token 消耗活跃度分布",
            })}
          </p>
        </div>

        <div style={{ width: "100%", overflowX: "auto", paddingTop: "8px" }}>
          <UsageCalendar
            data={yearTrendQuery.data?.trend ?? []}
            period={365}
            compact
            maxCellSize={14}
          />
        </div>
      </div>
    </div>
  );
};
