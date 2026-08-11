import { useEffect } from "react";
import { Alert, Card } from "antd";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { UsageCalendar, UsageTrendBars } from "@/components/UsageCalendar";
import { UsageSourceFilterSelect } from "@/components/UsageSourceFilterSelect";
import { AgentRuntimeRail, CurrentRoute, UsageSnapshot } from "@/components/dashboard";
import {
  managedAppsRuntimeStatusOptions,
  usageDashboardOptions,
  usageTrendOptions,
} from "@/lib/appQueries";
import { errMsg } from "@/lib/useProviderActions";
import { useProvidersStore } from "@/stores/providersStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { localDateKey } from "@/utils/usagePeriod";

/**
 * Overview — Multi-Agent Runtime Command Center.
 * Structure: Page Header → Agent Runtime Rail → Current Route + 24h Summary →
 * 24h Trend Section → Yearly Heatmap Section.
 */
export default function WorkbenchPage() {
  const { t } = useTranslation();
  const store = useProvidersStore();

  const providerTarget = usePagePreferencesStore((state) => state.providersTarget);
  const heatmapSource = usePagePreferencesStore((state) => state.heatmapSource);
  const setHeatmapSource = usePagePreferencesStore((state) => state.setHeatmapSource);

  useEffect(() => {
    void store.load(providerTarget);
  }, [store.load, providerTarget]);

  const runtimeQuery = useQuery(managedAppsRuntimeStatusOptions);

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
        gap: "14px",
        maxWidth: 1360,
        margin: "0 auto",
        width: "100%",
        boxSizing: "border-box",
      }}
    >
      {/* Page Header */}
      <div style={{ display: "flex", flexDirection: "column", gap: "2px" }}>
        <span style={{ fontSize: "18px", fontWeight: 600, lineHeight: 1.3 }}>
          {t("workbench.pageTitle", { defaultValue: "概览" })}
        </span>
        <span style={{ fontSize: "12px", color: "var(--color-text-secondary)" }}>
          {t("workbench.pageSubtitle", { defaultValue: "运行状态与使用情况" })}
        </span>
      </div>

      {/* 1. Unified Agent Runtime Rail */}
      <AgentRuntimeRail appRunningStatus={runtimeQuery.data} />

      {/* 2. Two-column row: Current Route (55%) + 24h Usage Summary (45%) */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "55% 45%",
          gap: "14px",
          alignItems: "stretch",
        }}
      >
        <CurrentRoute style={{ height: "100%", minHeight: 0 }} />

        <UsageSnapshot
          requestCount={summary?.requestCount ?? 0}
          totalTokens={totalTokens}
          estimatedCost={summary?.estimatedCost ?? 0}
          costCurrency={summary?.estimatedCostCurrency}
          successfulRequestCount={summary?.successfulRequestCount ?? 0}
          tokensVsYesterday={tokensVsYesterday}
          style={{ height: "100%", minHeight: 0 }}
        />
      </div>

      {/* 3. Past 24 Hours — section header outside the card */}
      <section style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
          <span style={{ fontSize: "14px", fontWeight: 600 }}>
            {t("workbench.last24h", { defaultValue: "过去 24 小时" })}
          </span>
          <div style={{ display: "inline-flex", alignItems: "center", gap: "6px" }}>
            <span style={{ fontSize: "11px", color: "var(--color-text-secondary)" }}>
              {t("usage.sourceLabel", { defaultValue: "数据源" })}:
            </span>
            <UsageSourceFilterSelect
              value={heatmapSource}
              onChange={setHeatmapSource}
              t={t}
            />
          </div>
        </div>
        <Card
          size="small"
          className="page-surface workbench-chart-card"
          style={{ marginBottom: 0 }}
          styles={{ body: { padding: "10px 12px" } }}
        >
          {trendQuery.error ? (
            <Alert type="error" showIcon message={errMsg(trendQuery.error)} />
          ) : (
            <div style={{ height: 190 }}>
              <UsageTrendBars data={trendQuery.data?.trend ?? []} period="24h" compact />
            </div>
          )}
        </Card>
      </section>

      {/* 4. Past Year — heatmap uses full available width */}
      <section style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
        <span style={{ fontSize: "14px", fontWeight: 600 }}>
          {t("workbench.pastYear", { defaultValue: "过去一年" })}
        </span>
        <Card
          size="small"
          className="page-surface workbench-chart-card"
          style={{ marginBottom: 0 }}
          styles={{ body: { padding: "10px 12px" } }}
        >
          {yearTrendQuery.error ? (
            <Alert type="error" showIcon message={errMsg(yearTrendQuery.error)} />
          ) : (
            <div style={{ width: "100%", overflow: "hidden" }}>
              <UsageCalendar
                data={yearTrendQuery.data?.trend ?? []}
                period={365}
                compact
              />
            </div>
          )}
        </Card>
      </section>
    </div>
  );
}

