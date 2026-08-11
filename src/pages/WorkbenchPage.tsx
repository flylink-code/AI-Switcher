import { useEffect } from "react";
import { Alert, Card, Space } from "antd";
import CalendarOutlined from "@ant-design/icons/es/icons/CalendarOutlined";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { UsageCalendar, UsageTrendBars } from "@/components/UsageCalendar";
import { UsageSourceFilterSelect } from "@/components/UsageSourceFilterSelect";
import { AgentTargetSwitcher } from "@/components/AgentTargetSwitcher";
import {
  RuntimeSnapshot,
  ProviderSnapshot,
  UsageSnapshot,
  QuickActions,
} from "@/components/dashboard";
import {
  managedAppsRuntimeStatusOptions,
  proxyStatusOptions,
  usageDashboardOptions,
  usageTrendOptions,
} from "@/lib/appQueries";
import { errMsg } from "@/lib/useProviderActions";
import { useProvidersStore } from "@/stores/providersStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { localDateKey } from "@/utils/usagePeriod";

export default function WorkbenchPage() {
  const { t } = useTranslation();
  const store = useProvidersStore();

  // Card-level Agent contexts: the runtime card mirrors the proxy page's
  // target, the provider card mirrors the providers page's target. Each is
  // switched independently on its own card.
  const providerTarget = usePagePreferencesStore((state) => state.providersTarget);
  const setProvidersTarget = usePagePreferencesStore((state) => state.setProvidersTarget);
  const runtimeTarget = usePagePreferencesStore((state) => state.proxyTarget);
  const setProxyTarget = usePagePreferencesStore((state) => state.setProxyTarget);
  const heatmapSource = usePagePreferencesStore((state) => state.heatmapSource);
  const setHeatmapSource = usePagePreferencesStore((state) => state.setHeatmapSource);

  useEffect(() => {
    void store.load(providerTarget);
  }, [store.load, providerTarget]);

  const runtimeQuery = useQuery(managedAppsRuntimeStatusOptions);
  const proxyQuery = useQuery(proxyStatusOptions(runtimeTarget));
  const proxy = proxyQuery.data;

  const appRunningKey =
    runtimeTarget === "claude_code"
      ? "claudeCode"
      : runtimeTarget === "claude_desktop"
      ? "claudeDesktop"
      : runtimeTarget === "opencode"
      ? "opencode"
      : "codex";
  const isAppRunning = Boolean(runtimeQuery.data?.[appRunningKey]);

  // Current Provider information
  const currentProvider = store.providers.find((p) => p.isCurrent) || null;
  const officialCurrent = !store.providers.some((p) => p.isCurrent);

  // Usage queries
  const dashboardQuery = useQuery(usageDashboardOptions("24h", heatmapSource));
  const trendQuery = useQuery(usageTrendOptions("24h", heatmapSource));
  const yearTrendQuery = useQuery(usageTrendOptions(365, heatmapSource));

  const summary = dashboardQuery.data?.summary;
  const totalTokens =
    (summary?.inputTokens ?? 0) +
    (summary?.cacheReadInputTokens ?? 0) +
    (summary?.cacheCreationInputTokens ?? 0) +
    (summary?.outputTokens ?? 0);

  // Day-over-day token delta
  const trendByDate = new Map((trendQuery.data?.trend ?? []).map((row) => [row.date, row]));
  const dayTokens = (offset: number) => {
    const date = new Date();
    date.setHours(0, 0, 0, 0);
    date.setDate(date.getDate() - offset);
    const row = trendByDate.get(localDateKey(date));
    return row
      ? row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens + row.outputTokens
      : 0;
  };
  const yesterdayTokens = dayTokens(1);
  const dayBeforeTokens = dayTokens(2);
  const tokensVsYesterday =
    yesterdayTokens > 0 && dayBeforeTokens > 0
      ? Math.round(((yesterdayTokens - dayBeforeTokens) / dayBeforeTokens) * 100)
      : null;

  return (
    <div
      className="dashboard-container"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-4)",
        maxWidth: 1400,
        margin: "0 auto",
        width: "100%",
      }}
    >
      {/* Top Section: Hero Grid (2x2 Snapshots) */}
      <div
        className="dashboard-hero-grid"
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))",
          gap: "var(--space-4)",
        }}
      >
        <RuntimeSnapshot
          proxyStatus={proxy || null}
          target={runtimeTarget}
          isAppRunning={isAppRunning}
          headerExtra={
            <AgentTargetSwitcher iconOnly value={runtimeTarget} onChange={setProxyTarget} />
          }
        />
        <ProviderSnapshot
          currentProvider={currentProvider}
          officialCurrent={officialCurrent}
          target={providerTarget}
          headerExtra={
            <AgentTargetSwitcher iconOnly value={providerTarget} onChange={setProvidersTarget} />
          }
        />
        <UsageSnapshot
          requestCount={summary?.requestCount ?? 0}
          totalTokens={totalTokens}
          estimatedCost={summary?.estimatedCost ?? 0}
          costCurrency={summary?.estimatedCostCurrency}
          successfulRequestCount={summary?.successfulRequestCount ?? 0}
          tokensVsYesterday={tokensVsYesterday}
        />
        <QuickActions />
      </div>

      {/* Middle Section: Trends & Filter Toolbar */}
      <div
        className="dashboard-filter-bar"
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          flexWrap: "wrap",
          gap: "var(--space-3)",
        }}
      >
        <span style={{ display: "inline-flex", alignItems: "center", gap: "var(--space-2)" }}>
          <span style={{ fontSize: "var(--font-size-xs)", color: "var(--color-text-secondary)" }}>
            {t("usage.sourceLabel", { defaultValue: "数据源" })}:
          </span>
          <UsageSourceFilterSelect
            value={heatmapSource}
            onChange={setHeatmapSource}
            t={t}
          />
        </span>
      </div>

      {/* Analytics Section: Charts Grid */}
      <div
        className="dashboard-charts-grid"
        style={{
          display: "grid",
          gridTemplateColumns: "1fr",
          gap: "var(--space-4)",
        }}
      >
        <Card
          size="small"
          className="page-surface workbench-chart-card"
          title={
            <Space>
              <BarChartOutlined />
              {t("usage.hourlyStatistics", { defaultValue: "最近 24 小时请求趋势" })}
            </Space>
          }
        >
          {trendQuery.error ? (
            <Alert type="error" showIcon message={errMsg(trendQuery.error)} />
          ) : (
            <UsageTrendBars data={trendQuery.data?.trend ?? []} period="24h" compact />
          )}
        </Card>

        <Card
          size="small"
          className="page-surface workbench-chart-card"
          title={
            <Space>
              <CalendarOutlined />
              {t("workbench.yearlyHeatmap", { defaultValue: "年度用量热力图" })}
            </Space>
          }
        >
          {yearTrendQuery.error ? (
            <Alert type="error" showIcon message={errMsg(yearTrendQuery.error)} />
          ) : (
            <UsageCalendar
              data={yearTrendQuery.data?.trend ?? []}
              period={365}
              compact
            />
          )}
        </Card>
      </div>
    </div>
  );
}
