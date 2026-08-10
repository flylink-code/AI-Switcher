import { useEffect, useState } from "react";
import { Alert, Card, Space, Typography } from "antd";
import CalendarOutlined from "@ant-design/icons/es/icons/CalendarOutlined";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { UsageCalendar, UsageTrendBars } from "@/components/UsageCalendar";
import { UsageSourceFilterSegmented } from "@/components/UsageSourceFilterSegmented";
import { ProviderForm } from "@/components/ProviderForm";
import { ImportPreviewDialog } from "@/components/ImportPreviewDialog";
import { FloatingViewSwitcher } from "@/components/FloatingViewSwitcher";
import {
  RuntimeSnapshot,
  ProviderSnapshot,
  UsageSnapshot,
  QuickActions,
} from "@/components/dashboard";
import { Stack } from "@/components/ui";
import {
  managedAppsRuntimeStatusOptions,
  proxyStatusOptions,
  usageDashboardOptions,
  usageTrendOptions,
} from "@/lib/appQueries";
import { useProviderActions } from "@/lib/useProviderActions";
import { useProvidersStore } from "@/stores/providersStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { Provider } from "@/types/backend";
import { localDateKey } from "@/utils/usagePeriod";

const { Title } = Typography;

export default function WorkbenchPage() {
  const { t } = useTranslation();
  const store = useProvidersStore();

  const target = usePagePreferencesStore((state) => state.providersTarget);
  const heatmapSource = usePagePreferencesStore((state) => state.heatmapSource);
  const setHeatmapSource = usePagePreferencesStore((state) => state.setHeatmapSource);

  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Provider | null>(null);

  const {
    importPreview,
    importConfirming,
    setImportPreview,
    handleSubmit,
    handleConfirmImport,
  } = useProviderActions({ target, editing, closeForm: () => setFormOpen(false) });

  const officialCurrent = !store.providers.some((provider) => provider.isCurrent);
  const currentProvider = store.providers.find((provider) => provider.isCurrent) ?? null;

  useEffect(() => {
    void store.load(target);
  }, [store.load, target]);

  // Queries (100% Shared Cache Reuse)
  const runtimeQuery = useQuery(managedAppsRuntimeStatusOptions);
  const proxyQuery = useQuery(proxyStatusOptions(target));
  const dashboardQuery = useQuery(usageDashboardOptions("24h", heatmapSource));
  const trendQuery = useQuery(usageTrendOptions("24h", heatmapSource));
  const yearTrendQuery = useQuery(usageTrendOptions(365, heatmapSource));

  const proxyStatus = proxyQuery.data ?? null;
  const appRunningKey = target === "claude_code" ? "claudeCode" : target === "claude_desktop" ? "claudeDesktop" : target === "opencode" ? "opencode" : "codex";
  const isAppRunning = Boolean(runtimeQuery.data?.[appRunningKey]);

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
    <Stack gap="md" style={{ width: "100%", minWidth: 0 }}>
      {/* 1. Hero Runtime Snapshot */}
      <RuntimeSnapshot
        proxyStatus={proxyStatus}
        target={target}
        isAppRunning={isAppRunning}
      />

      {/* 2. Primary Snapshots: Provider + Usage Grid */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(340px, 1fr))",
          gap: "var(--card-gap)",
        }}
      >
        <ProviderSnapshot
          currentProvider={currentProvider}
          officialCurrent={officialCurrent}
          target={target}
        />

        <UsageSnapshot
          requestCount={summary?.requestCount ?? 0}
          totalTokens={totalTokens}
          estimatedCost={summary?.estimatedCost ?? 0}
          costCurrency={summary?.estimatedCostCurrency}
          successfulRequestCount={summary?.successfulRequestCount ?? 0}
          tokensVsYesterday={tokensVsYesterday}
        />
      </div>

      {/* 3. Trend Charts Section */}
      <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-3)" }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
          <Title level={5} style={{ margin: 0, fontSize: "var(--font-size-md)" }}>
            {t("workbench.usageSection", { defaultValue: "用量趋势概览" })}
          </Title>

          <UsageSourceFilterSegmented
            value={heatmapSource}
            onChange={setHeatmapSource}
            t={t}
            iconOnly
          />
        </div>

        <div className="workbench-charts-grid">
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

          <Card
            size="small"
            className="page-surface workbench-chart-card"
            title={
              <Space>
                <BarChartOutlined />
                {t("usage.hourlyStatistics", { defaultValue: "24 小时统计" })}
              </Space>
            }
          >
            {trendQuery.error ? (
              <Alert type="error" showIcon message={errMsg(trendQuery.error)} />
            ) : (
              <UsageTrendBars data={trendQuery.data?.trend ?? []} period="24h" compact />
            )}
          </Card>
        </div>
      </div>

      {/* 4. Quick Actions */}
      <QuickActions />

      {/* View Switcher & Modals */}
      <FloatingViewSwitcher />

      <ProviderForm
        open={formOpen}
        editing={editing}
        target={target}
        onCancel={() => setFormOpen(false)}
        onSubmit={handleSubmit}
      />

      <ImportPreviewDialog
        open={Boolean(importPreview)}
        preview={importPreview}
        confirming={importConfirming}
        onCancel={() => setImportPreview(null)}
        onConfirm={() => void handleConfirmImport()}
      />
    </Stack>
  );
}

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
