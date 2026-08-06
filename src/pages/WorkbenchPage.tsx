import { useState } from "react";
import {
  App as AntApp,
  Alert,
  Badge,
  Button,
  Card,
  Select,
  Space,
  Tag,
  Tooltip,
  Typography,
} from "antd";
import CalendarOutlined from "@ant-design/icons/es/icons/CalendarOutlined";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import LineChartOutlined from "@ant-design/icons/es/icons/LineChartOutlined";
import NodeIndexOutlined from "@ant-design/icons/es/icons/NodeIndexOutlined";
import SettingOutlined from "@ant-design/icons/es/icons/SettingOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { UsageCalendar, UsageTrendBars } from "@/components/UsageCalendar";
import { UsageSourceFilterSegmented } from "@/components/UsageSourceFilterSegmented";
import { usageSourceIcon } from "@/components/UsageSourceIcons";
import {
  managedAppsRuntimeStatusOptions,
  providerListOptions,
  proxyStatusOptions,
  usageDashboardOptions,
  usageTrendOptions,
} from "@/lib/appQueries";
import { useNavigatePage } from "@/lib/navigation";
import { showCodexSwitchNotice } from "@/lib/codexNotice";
import { switchProvider, switchToOfficial } from "@/services/api";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { Provider, ProviderTarget } from "@/types/backend";
import { formatCompactNumber } from "@/utils/formatCompact";

const { Text, Title } = Typography;

const OFFICIAL_VALUE = "__official__";

const WORKBENCH_APPS: Array<{
  target: ProviderTarget;
  runningKey: "claudeCode" | "claudeDesktop" | "codex";
  labelKey: string;
}> = [
  { target: "claude_code", runningKey: "claudeCode", labelKey: "workspace.claude_code" },
  { target: "claude_desktop", runningKey: "claudeDesktop", labelKey: "workspace.claude_desktop" },
  { target: "codex", runningKey: "codex", labelKey: "workspace.codex" },
];

export default function WorkbenchPage() {
  const { t } = useTranslation();
  const navigate = useNavigatePage();
  const heatmapSource = usePagePreferencesStore((state) => state.heatmapSource);
  const setHeatmapSource = usePagePreferencesStore((state) => state.setHeatmapSource);

  // Workbench uses fixed 24h for dashboard summary and hourly bars, fixed 365 for yearly heatmap.
  const dashboardQuery = useQuery(usageDashboardOptions("24h", heatmapSource));
  const trendQuery = useQuery(usageTrendOptions("24h", heatmapSource));
  const yearTrendQuery = useQuery(usageTrendOptions(365, heatmapSource));

  const summary = dashboardQuery.data?.summary;
  const totalTokens =
    (summary?.inputTokens ?? 0) +
    (summary?.cacheReadInputTokens ?? 0) +
    (summary?.cacheCreationInputTokens ?? 0) +
    (summary?.outputTokens ?? 0);

  const sourceFilterToolbar = (
    <div className="usage-filters-toolbar">
      <div className="usage-filters-segmented">
        <UsageSourceFilterSegmented
          value={heatmapSource}
          onChange={setHeatmapSource}
          t={t}
          iconOnly
        />
      </div>
      <Tooltip title={t("workbench.viewUsageDetail")}>
        <Button
          size="middle"
          icon={<LineChartOutlined />}
          onClick={() => navigate("usage")}
        />
      </Tooltip>
    </div>
  );

  return (
    <div className="workbench-layout">
      <div className="workbench-stats">
        <div className="workbench-section-header">
          <Title level={5} style={{ margin: 0 }}>
            {t("workbench.usageSection")}
          </Title>
          {sourceFilterToolbar}
        </div>

        <div className="workbench-stats-cards">
          {dashboardQuery.error ? (
            <Alert type="error" showIcon message={errMsg(dashboardQuery.error)} />
          ) : (
            <UsageSummaryGrid
              estimatedCost={summary?.estimatedCost ?? 0}
              costCurrency={summary?.estimatedCostCurrency}
              totalTokens={totalTokens}
              requestCount={summary?.requestCount ?? 0}
              successfulRequestCount={summary?.successfulRequestCount ?? 0}
            />
          )}

          <Card
            size="small"
            className="page-surface"
            title={
              <Space>
                <CalendarOutlined />
                {t("workbench.yearlyHeatmap")}
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
            className="page-surface"
            title={
              <Space>
                <BarChartOutlined />
                {t("usage.hourlyStatistics")}
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

      <div className="workbench-apps-column">
        <div className="workbench-section-header">
          <Title level={5} style={{ margin: 0 }}>
            {t("workbench.appsSection")}
          </Title>
        </div>
        <div className="workbench-apps">
          {WORKBENCH_APPS.map((app) => (
            <AppStatusCard
              key={app.target}
              target={app.target}
              runningKey={app.runningKey}
              labelKey={app.labelKey}
            />
          ))}
        </div>
      </div>
    </div>
  );
}

/**
 * Compact 2×2 KPI grid — cost is the hero (red), tokens major,
 * requests/success minor. No sub-lines; details live on the usage page.
 */
function UsageSummaryGrid({
  estimatedCost,
  costCurrency,
  totalTokens,
  requestCount,
  successfulRequestCount,
}: {
  estimatedCost: number;
  costCurrency?: string | null;
  totalTokens: number;
  requestCount: number;
  successfulRequestCount: number;
}) {
  const { t } = useTranslation();
  const rate = successRate(requestCount, successfulRequestCount);

  return (
    <div className="usage-summary-grid">
      <div className="usage-summary-cell">
        <div className="usage-cell-label">{t("usage.estimatedCost")}</div>
        <div className="usage-hero-value">
          {currencyPrefix(costCurrency)}
          {estimatedCost.toFixed(4)}
        </div>
      </div>
      <div className="usage-summary-cell">
        <div className="usage-cell-label">{t("usage.totalTokens")}</div>
        <div className="usage-major-value">{formatCompactNumber(totalTokens)}</div>
      </div>
      <div className="usage-summary-cell">
        <div className="usage-cell-label">{t("usage.requests")}</div>
        <div className="usage-minor-value">
          {t("usage.requestCountUnit", { count: requestCount })}
        </div>
      </div>
      <div className="usage-summary-cell">
        <div className="usage-cell-label">{t("usage.successRate")}</div>
        <div className="usage-minor-value">{rate}%</div>
      </div>
    </div>
  );
}

function AppStatusCard({
  target,
  runningKey,
  labelKey,
}: {
  target: ProviderTarget;
  runningKey: "claudeCode" | "claudeDesktop" | "codex";
  labelKey: string;
}) {
  const { t } = useTranslation();
  const { message } = AntApp.useApp();
  const queryClient = useQueryClient();
  const navigate = useNavigatePage();
  const [switching, setSwitching] = useState(false);

  const runtimeQuery = useQuery(managedAppsRuntimeStatusOptions);
  const providersQuery = useQuery(providerListOptions(target));
  const proxyQuery = useQuery(proxyStatusOptions(target));

  const name = t(labelKey);
  const running = Boolean(runtimeQuery.data?.[runningKey]);
  const providers = providersQuery.data ?? [];
  const current = providers.find((p) => p.isCurrent);
  const proxy = proxyQuery.data;

  const handleSwitch = async (value: string) => {
    if (switching) return;
    setSwitching(true);
    try {
      if (value === OFFICIAL_VALUE) {
        await switchToOfficial(target);
        queryClient.setQueryData<Provider[]>(
          providerListOptions(target).queryKey,
          (list = []) => list.map((p) => ({ ...p, isCurrent: false })),
        );
      } else {
        const result = await switchProvider(value);
        queryClient.setQueryData<Provider[]>(
          providerListOptions(target).queryKey,
          (list = []) => list.map((p) => ({ ...p, isCurrent: p.id === result.provider.id })),
        );
        showCodexSwitchNotice(result.codexNotice, message, t);
      }
      void message.success(t("workbench.switchSuccess", { name }));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setSwitching(false);
    }
  };

  return (
    <Card
      size="small"
      className="page-surface workbench-app-card"
      data-app={target}
      data-status={running ? "running" : "stopped"}
      title={
        <Space size={8}>
          <span className="workbench-app-icon" aria-hidden>
            {usageSourceIcon(target, { size: 15 })}
          </span>
          <span>{name}</span>
        </Space>
      }
      extra={
        <Tooltip
          title={
            running
              ? t("workspace.appRunning", { name })
              : t("workspace.appStopped", { name })
          }
        >
          <Badge
            status={running ? "success" : "default"}
            text={
              <Text
                style={{
                  fontSize: 12,
                  color: running ? undefined : "inherit",
                  opacity: running ? 1 : 0.55,
                }}
              >
                {running ? t("workbench.running") : t("workbench.stopped")}
              </Text>
            }
          />
        </Tooltip>
      }
    >
      <div className="workbench-app-body">
        <div className="workbench-app-provider">
          <Text type="secondary" className="workbench-app-provider-label">
            {t("workbench.currentProvider")}
          </Text>
          <Select
            size="middle"
            className="workbench-app-provider-select"
            loading={providersQuery.isLoading || switching}
            value={current?.id ?? OFFICIAL_VALUE}
            onChange={(value) => void handleSwitch(value)}
            options={[
              { value: OFFICIAL_VALUE, label: t("providers.officialMode") },
              ...providers.map((p) => ({ value: p.id, label: p.name })),
            ]}
          />
          <Text type="secondary" ellipsis className="workbench-app-model">
            {current ? current.model : " "}
          </Text>
        </div>
        <div className="workbench-app-actions">
          <Tag
            icon={<NodeIndexOutlined />}
            color={proxy?.running ? "green" : undefined}
            style={{ marginInlineEnd: 0, opacity: proxy?.running ? 1 : 0.65, cursor: "pointer" }}
            onClick={() => navigate("proxy")}
          >
            {proxy?.running
              ? t("workbench.proxyRunning", { port: proxy.port })
              : t("workbench.proxyStopped")}
          </Tag>
          <Button
            type="link"
            size="small"
            icon={<SettingOutlined />}
            style={{ paddingInline: 0 }}
            onClick={() => navigate("providers")}
          >
            {t("workbench.manage")}
          </Button>
        </div>
      </div>
    </Card>
  );
}

function successRate(total?: number, successful?: number) {
  return total ? Number((((successful ?? 0) / total) * 100).toFixed(1)) : 0;
}

function currencyPrefix(currency?: string | null) {
  const normalized = (currency ?? "USD").trim().toUpperCase();
  if (normalized === "CNY" || normalized === "RMB") return "¥";
  if (normalized === "EUR") return "€";
  if (normalized === "GBP") return "£";
  if (normalized === "USD" || normalized === "") return "$";
  return `${normalized} `;
}

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
