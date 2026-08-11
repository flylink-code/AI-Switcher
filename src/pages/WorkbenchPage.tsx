import { useEffect } from "react";
import { Alert, Card, Space, Typography } from "antd";
import CalendarOutlined from "@ant-design/icons/es/icons/CalendarOutlined";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { UsageCalendar, UsageTrendBars } from "@/components/UsageCalendar";
import { UsageSourceFilterSelect } from "@/components/UsageSourceFilterSelect";
import { TARGET_OPTIONS } from "@/components/AgentTargetSwitcher";
import { AgentRuntimeCell, ProviderSnapshot, UsageSnapshot } from "@/components/dashboard";
import {
  managedAppsRuntimeStatusOptions,
  usageDashboardOptions,
  usageTrendOptions,
} from "@/lib/appQueries";
import { errMsg } from "@/lib/useProviderActions";
import { useProvidersStore } from "@/stores/providersStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { localDateKey } from "@/utils/usagePeriod";
import type { ProviderTarget } from "@/types/backend";

const { Text } = Typography;

const APP_RUNNING_KEYS: Record<ProviderTarget, "claudeCode" | "claudeDesktop" | "codex" | "opencode"> = {
  claude_code: "claudeCode",
  claude_desktop: "claudeDesktop",
  codex: "codex",
  opencode: "opencode",
};

/**
 * Overview — Runtime Command Center.
 * ① all 4 agents' proxy runtime in one grid (no agent switching here)
 * ② current provider ③ usage metrics + 24h trend + yearly heatmap, all
 * driven by one shared source filter (all agents / single agent).
 */
export default function WorkbenchPage() {
  const { t } = useTranslation();
  const store = useProvidersStore();

  // The provider card mirrors the providers page's target (switched there,
  // not here). The usage source filter is shared by all three data sections.
  const providerTarget = usePagePreferencesStore((state) => state.providersTarget);
  const heatmapSource = usePagePreferencesStore((state) => state.heatmapSource);
  const setHeatmapSource = usePagePreferencesStore((state) => state.setHeatmapSource);

  useEffect(() => {
    void store.load(providerTarget);
  }, [store.load, providerTarget]);

  const runtimeQuery = useQuery(managedAppsRuntimeStatusOptions);

  // Current Provider information
  const currentProvider = store.providers.find((p) => p.isCurrent) || null;
  const officialCurrent = !store.providers.some((p) => p.isCurrent);

  // Usage queries — all three sections share the same source filter
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
      {/* 1. All 4 agents' runtime at a glance — status + route + start/stop */}
      <div
        className="dashboard-agent-grid"
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(260px, 1fr))",
          gap: "var(--space-4)",
        }}
      >
        {TARGET_OPTIONS.map((target) => (
          <AgentRuntimeCell
            key={target}
            target={target}
            appRunning={
              runtimeQuery.data ? Boolean(runtimeQuery.data[APP_RUNNING_KEYS[target]]) : undefined
            }
          />
        ))}
      </div>

      {/* 2. Current route (provider for the providers-page target) */}
      <ProviderSnapshot
        currentProvider={currentProvider}
        officialCurrent={officialCurrent}
        target={providerTarget}
      />

      {/* 3. Usage — one shared source filter drives metrics + trend + heatmap */}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          flexWrap: "wrap",
          gap: "var(--space-3)",
        }}
      >
        <Text strong style={{ fontSize: "var(--font-size-md)" }}>
          {t("navigation.usage", { defaultValue: "用量统计" })}
        </Text>
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

      <UsageSnapshot
        requestCount={summary?.requestCount ?? 0}
        totalTokens={totalTokens}
        estimatedCost={summary?.estimatedCost ?? 0}
        costCurrency={summary?.estimatedCostCurrency}
        successfulRequestCount={summary?.successfulRequestCount ?? 0}
        tokensVsYesterday={tokensVsYesterday}
      />

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
  );
}
