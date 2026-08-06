import { useState } from "react";
import {
  App as AntApp,
  Alert,
  Badge,
  Button,
  Card,
  Col,
  Row,
  Select,
  Space,
  Tag,
  Tooltip,
  Typography,
} from "antd";
import CalendarOutlined from "@ant-design/icons/es/icons/CalendarOutlined";
import NodeIndexOutlined from "@ant-design/icons/es/icons/NodeIndexOutlined";
import SettingOutlined from "@ant-design/icons/es/icons/SettingOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { UsageCalendar } from "@/components/UsageCalendar";
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
import {
  USAGE_PERIOD_VALUES,
  usagePeriodLabelKey,
} from "@/utils/usagePeriod";

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
  const heatmapPeriod = usePagePreferencesStore((state) => state.heatmapPeriod);
  const setHeatmapPeriod = usePagePreferencesStore((state) => state.setHeatmapPeriod);
  const heatmapSource = usePagePreferencesStore((state) => state.heatmapSource);
  const setHeatmapSource = usePagePreferencesStore((state) => state.setHeatmapSource);

  const dashboardQuery = useQuery(usageDashboardOptions(heatmapPeriod, heatmapSource));
  const trendQuery = useQuery(usageTrendOptions(heatmapPeriod, heatmapSource));

  const summary = dashboardQuery.data?.summary;
  const totalTokens =
    (summary?.inputTokens ?? 0) +
    (summary?.cacheReadInputTokens ?? 0) +
    (summary?.cacheCreationInputTokens ?? 0) +
    (summary?.outputTokens ?? 0);

  const periodSourceFilters = (
    <Space wrap size={8} align="center">
      <Select
        size="middle"
        value={heatmapPeriod}
        style={{ width: 160 }}
        options={USAGE_PERIOD_VALUES.map((value) => ({
          value,
          label:
            typeof value === "number"
              ? t("usage.lastDays", { days: value })
              : t(usagePeriodLabelKey(value)),
        }))}
        onChange={setHeatmapPeriod}
      />
      <UsageSourceFilterSegmented
        value={heatmapSource}
        onChange={setHeatmapSource}
        t={t}
      />
    </Space>
  );

  return (
    <Space direction="vertical" size={16} style={{ width: "100%" }}>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: 12,
          alignItems: "flex-start",
          justifyContent: "space-between",
        }}
      >
        <Title level={4} style={{ margin: 0 }}>
          {t("workbench.title")}
        </Title>
      </div>

      <Row gutter={[16, 16]}>
        {WORKBENCH_APPS.map((app) => (
          <Col xs={24} md={12} xl={8} key={app.target}>
            <AppStatusCard
              target={app.target}
              runningKey={app.runningKey}
              labelKey={app.labelKey}
            />
          </Col>
        ))}
      </Row>

      <div className="usage-section">
        <div
          style={{
            display: "flex",
            flexWrap: "wrap",
            gap: 12,
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <Space size={12} align="center">
            <Title level={5} style={{ margin: 0 }}>
              {t("workbench.usageSection")}
            </Title>
            <UsageDetailLink />
          </Space>
          {periodSourceFilters}
        </div>

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
              {t("usage.dailyStatistics")}
            </Space>
          }
        >
          {trendQuery.error ? (
            <Alert type="error" showIcon message={errMsg(trendQuery.error)} />
          ) : (
            <UsageCalendar data={trendQuery.data?.trend ?? []} period={heatmapPeriod} />
          )}
        </Card>
      </div>
    </Space>
  );
}

/**
 * Guide §3.2: 2×2 grid — cost is the hero (28px, red), total tokens major
 * (24px), requests/success minor (16px secondary) with sub-lines.
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
  const failedCount = Math.max(requestCount - successfulRequestCount, 0);
  const avgTokens = requestCount > 0 ? Math.round(totalTokens / requestCount) : 0;
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
        <div className="usage-cell-sub">
          {t("usage.avgTokensPerRequest", { value: formatCompactNumber(avgTokens) })}
        </div>
      </div>
      <div className="usage-summary-cell">
        <div className="usage-cell-label">{t("usage.successRate")}</div>
        <div className="usage-minor-value">{rate}%</div>
        <div className="usage-cell-sub">
          {t("usage.failedCount", { count: failedCount })}
        </div>
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
      <Space direction="vertical" size={10} style={{ width: "100%" }}>
        <div>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("workbench.currentProvider")}
          </Text>
          <Select
            size="middle"
            style={{ width: "100%", marginTop: 4 }}
            loading={providersQuery.isLoading || switching}
            value={current?.id ?? OFFICIAL_VALUE}
            onChange={(value) => void handleSwitch(value)}
            options={[
              { value: OFFICIAL_VALUE, label: t("providers.officialMode") },
              ...providers.map((p) => ({ value: p.id, label: p.name })),
            ]}
          />
          <Text
            type="secondary"
            ellipsis
            style={{ fontSize: 12, display: "block", marginTop: 4, minHeight: 20 }}
          >
            {current ? current.model : " "}
          </Text>
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 8,
          }}
        >
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
      </Space>
    </Card>
  );
}

function UsageDetailLink() {
  const { t } = useTranslation();
  const navigate = useNavigatePage();
  return (
    <Button
      type="link"
      size="small"
      style={{ paddingInline: 0 }}
      onClick={() => navigate("usage")}
    >
      {t("workbench.viewUsageDetail")}
    </Button>
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
