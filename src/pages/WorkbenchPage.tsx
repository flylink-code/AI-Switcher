import { useEffect } from "react";
import { Alert, Card, Typography } from "antd";
import CalendarOutlined from "@ant-design/icons/es/icons/CalendarOutlined";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
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

const { Text } = Typography;

/**
 * Overview — Multi-Agent Runtime Command Center.
 * ① AgentRuntimeRail: All 4 agents' runtime in one unified Surface rail
 * ② Two-column row: CurrentRoute (55%) + UsageSnapshot 24h Summary (45%)
 * ③ 24h Hourly trend with unified source filter
 * ④ Yearly heatmap (过去一年)
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
        gap: "20px",
        maxWidth: 1360,
        margin: "0 auto",
        width: "100%",
      }}
    >
      {/* 1. Unified Agent Runtime Rail */}
      <AgentRuntimeRail appRunningStatus={runtimeQuery.data} />

      {/* 2. Two-column split: Current Route (55%) + 24h Summary (45%) */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "55% 45%",
          gap: "16px",
          alignItems: "stretch",
        }}
      >
        <CurrentRoute />

        <UsageSnapshot
          requestCount={summary?.requestCount ?? 0}
          totalTokens={totalTokens}
          estimatedCost={summary?.estimatedCost ?? 0}
          costCurrency={summary?.estimatedCostCurrency}
          successfulRequestCount={summary?.successfulRequestCount ?? 0}
          tokensVsYesterday={tokensVsYesterday}
        />
      </div>

      {/* 3. 24h Hourly Statistics */}
      <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            flexWrap: "wrap",
            gap: "12px",
          }}
        >
          <Text strong style={{ fontSize: "14px", color: "var(--color-text-primary)" }}>
            {t("workbench.past24hTitle", { defaultValue: "过去 24 小时" })}
          </Text>
          <span style={{ display: "inline-flex", alignItems: "center", gap: "8px" }}>
            <span style={{ fontSize: "12px", color: "var(--color-text-secondary)" }}>
              {t("usage.sourceLabel", { defaultValue: "数据来源" })}:
            </span>
            <UsageSourceFilterSelect
              value={heatmapSource}
              onChange={setHeatmapSource}
              t={t}
            />
          </span>
        </div>

        <Card
          size="small"
          className="page-surface workbench-chart-card"
          title={
            <span style={{ display: "inline-flex", alignItems: "center", gap: "6px", fontSize: "13px" }}>
              <BarChartOutlined />
              {t("usage.hourlyStatistics", { defaultValue: "最近 24 小时请求趋势" })}
            </span>
          }
        >
          {trendQuery.error ? (
            <Alert type="error" showIcon message={errMsg(trendQuery.error)} />
          ) : (
            <div style={{ height: 210 }}>
              <UsageTrendBars data={trendQuery.data?.trend ?? []} period="24h" compact />
            </div>
          )}
        </Card>
      </div>

      {/* 4. Yearly Heatmap */}
      <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
        <Text strong style={{ fontSize: "14px", color: "var(--color-text-primary)" }}>
          {t("workbench.pastYearTitle", { defaultValue: "过去一年" })}
        </Text>

        <Card
          size="small"
          className="page-surface workbench-chart-card"
          title={
            <span style={{ display: "inline-flex", alignItems: "center", gap: "6px", fontSize: "13px" }}>
              <CalendarOutlined />
              {t("workbench.yearlyHeatmap", { defaultValue: "年度用量热力图" })}
            </span>
          }
        >
          {yearTrendQuery.error ? (
            <Alert type="error" showIcon message={errMsg(yearTrendQuery.error)} />
          ) : (
            <div style={{ width: "100%", overflowX: "auto" }}>
              <UsageCalendar
                data={yearTrendQuery.data?.trend ?? []}
                period={365}
                compact
              />
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}
