import { useEffect, useState } from "react";
import {
  Alert,
  Badge,
  Button,
  Card,
  Dropdown,
  Popconfirm,
  Space,
  Tag,
  Tooltip,
  Typography,
  type MenuProps,
} from "antd";
import CalendarOutlined from "@ant-design/icons/es/icons/CalendarOutlined";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import EditOutlined from "@ant-design/icons/es/icons/EditOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import NodeIndexOutlined from "@ant-design/icons/es/icons/NodeIndexOutlined";
import PayCircleOutlined from "@ant-design/icons/es/icons/PayCircleOutlined";
import CheckCircleOutlined from "@ant-design/icons/es/icons/CheckCircleOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { UsageCalendar, UsageTrendBars } from "@/components/UsageCalendar";
import { UsageSourceFilterSegmented } from "@/components/UsageSourceFilterSegmented";
import { WorkspaceTargetSegmented } from "@/components/WorkspaceTargetSegmented";
import { ProviderForm } from "@/components/ProviderForm";
import { ImportPreviewDialog } from "@/components/ImportPreviewDialog";
import { ProviderBrandIcon } from "@/components/ProviderBrandIcon";
import { FloatingViewSwitcher } from "@/components/FloatingViewSwitcher";
import { usageSourceIcon } from "@/components/UsageSourceIcons";
import {
  managedAppsRuntimeStatusOptions,
  proxyStatusOptions,
  usageDashboardOptions,
  usageTrendOptions,
} from "@/lib/appQueries";
import { useNavigatePage } from "@/lib/navigation";
import { errMsg, useProviderActions } from "@/lib/useProviderActions";
import { useProvidersStore } from "@/stores/providersStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { Provider } from "@/types/backend";
import { getAntigravityGatewayStatus } from "@/services/api";
import { formatCompactNumber } from "@/utils/formatCompact";
import { localDateKey } from "@/utils/usagePeriod";

const { Text, Title } = Typography;

export default function WorkbenchPage() {
  const { t } = useTranslation();
  const navigate = useNavigatePage();
  const store = useProvidersStore();

  const target = usePagePreferencesStore((state) => state.providersTarget);
  const setTarget = usePagePreferencesStore((state) => state.setProvidersTarget);
  const workbenchView = usePagePreferencesStore((state) => state.workbenchView);
  const heatmapSource = usePagePreferencesStore((state) => state.heatmapSource);
  const setHeatmapSource = usePagePreferencesStore((state) => state.setHeatmapSource);

  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Provider | null>(null);
  const {
    busy,
    switchingId,
    testingId,
    batchTesting,
    importPreview,
    importConfirming,
    setImportPreview,
    handleSubmit,
    handleSwitch,
    handleOfficial,
    handleTest,
    handleSpeedtestAll,
    handleShareLink,
    handleDelete,
    handleExport,
    handleImportLive,
    handleImportClipboard,
    handleConfirmImport,
  } = useProviderActions({ target, editing, closeForm: () => setFormOpen(false) });

  const officialCurrent = !store.providers.some((provider) => provider.isCurrent);

  useEffect(() => {
    void store.load(target);
  }, [store.load, target]);

  const runtimeQuery = useQuery(managedAppsRuntimeStatusOptions);
  const proxyQuery = useQuery(proxyStatusOptions(target));
  const proxy = proxyQuery.data;
  const antigravityQuery = useQuery({
    queryKey: ["antigravity-gateway"],
    queryFn: getAntigravityGatewayStatus,
    refetchInterval: 5_000,
  });
  const antigravity = antigravityQuery.data;

  const appRunningKey = target === "claude_code" ? "claudeCode" : target === "claude_desktop" ? "claudeDesktop" : target === "opencode" ? "opencode" : "codex";
  const isAppRunning = Boolean(runtimeQuery.data?.[appRunningKey]);

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

  // Day-over-day token delta: yesterday vs the day before (from the 24h trend rows).
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

  const openCreate = () => {
    setEditing(null);
    setFormOpen(true);
  };

  const openEdit = (provider: Provider) => {
    setEditing(provider);
    setFormOpen(true);
  };

  const importExportItems: MenuProps["items"] = [
    {
      key: "importClipboard",
      label: t("providers.importClipboard"),
      onClick: () => void handleImportClipboard(),
    },
    {
      key: "exportJson",
      label: t("providers.exportJson"),
      onClick: () => void handleExport(),
    },
  ];

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
      <Button
        type="link"
        size="small"
        onClick={() => navigate("usage")}
        style={{ fontSize: 13, paddingRight: 0 }}
      >
        {t("workbench.viewUsageDetail", "查看用量详情")} →
      </Button>
    </div>
  );

  return (
    <div className="cc-workbench-container">
      {workbenchView === "providers" ? (
        <>
          {/* CC Switch Header: Target Switcher & Actions */}
          <div className="cc-workbench-header">
            <div className="cc-header-left">
              <WorkspaceTargetSegmented value={target} onChange={setTarget} t={t} />
              <Badge
                status={isAppRunning ? "success" : "default"}
                text={isAppRunning ? t("workbench.running") : t("workbench.stopped")}
              />
              <Tag
                icon={<NodeIndexOutlined />}
                color={target === "opencode" ? "blue" : proxy?.running ? "green" : undefined}
                style={{ cursor: "pointer", margin: 0 }}
                onClick={() => navigate("proxy")}
              >
                {target === "opencode"
                  ? t("workbench.proxyDirect")
                  : proxy?.running ? t("workbench.proxyRunning", { port: proxy.port }) : t("workbench.proxyStopped")}
              </Tag>
              <Tag
                color={antigravity?.running ? "purple" : undefined}
                style={{ cursor: "pointer", margin: 0 }}
                onClick={() => navigate("antigravity")}
              >
                {antigravity?.running
                  ? t("workbench.antigravityRunning", { port: antigravity.port })
                  : t("workbench.antigravityStopped")}
              </Tag>
            </div>
            <div className="cc-header-right">
              <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
                {t("providers.create")}
              </Button>
              {target === "opencode" && (
                <Button
                  icon={<ImportOutlined />}
                  loading={busy}
                  onClick={() => void handleImportLive()}
                >
                  {t("providers.syncOpenCodeLive")}
                </Button>
              )}
              <Button
                icon={<ThunderboltOutlined />}
                loading={batchTesting}
                onClick={() => void handleSpeedtestAll()}
              >
                {t("providers.speedtestAll")}
              </Button>
              <Dropdown menu={{ items: importExportItems }}>
                <Button icon={<ImportOutlined />}>
                  {t("providers.importExport")}
                </Button>
              </Dropdown>
            </div>
          </div>

          {/* Main CC Switch Provider Card List */}
          <div className="cc-provider-list">
            {/* Official Provider Card */}
            {target !== "opencode" && (
            <div className={`cc-provider-card ${officialCurrent ? "cc-provider-card-active" : ""}`}>
              <div className="cc-provider-card-body">
                <div className="cc-provider-card-header">
                  <div className="cc-provider-main">
                    <div className="cc-provider-icon" style={{ width: 36, height: 36, borderRadius: 8 }}>
                      {usageSourceIcon(target, { size: 20 })}
                    </div>
                    <div className="cc-provider-info">
                      <span className="cc-provider-name">{t("providers.officialMode")}</span>
                    </div>
                  </div>
                  {officialCurrent && (
                    <Tag color="success" style={{ margin: 0, borderRadius: 999, paddingInline: 10, fontSize: 11 }}>
                      🟢 {t("providers.current")}
                    </Tag>
                  )}
                </div>
                <div className="cc-provider-card-footer" style={{ borderTop: "none", paddingTop: 0 }}>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {t("providers.officialModeHint", { defaultValue: "使用官方原生 API Endpoint / 账号凭据" })}
                  </Text>
                  {!officialCurrent && (
                    <Button type="primary" size="small" style={{ borderRadius: 6, fontSize: 12 }} loading={switchingId === "official"} onClick={() => void handleOfficial()}>
                      {t("providers.switchTo")}
                    </Button>
                  )}
                </div>
              </div>
            </div>
            )}

            {/* Custom Provider Cards */}
            {store.providers.map((provider) => {
              const isCurrent = provider.isCurrent;
              return (
                <div
                  key={provider.id}
                  className={`cc-provider-card ${target !== "opencode" && isCurrent ? "cc-provider-card-active" : ""}`}
                >
                  <div className="cc-provider-card-body">
                    {/* Header Row */}
                    <div className="cc-provider-card-header">
                      <div className="cc-provider-main">
                        <ProviderBrandIcon provider={provider} size={36} />
                        <div className="cc-provider-info">
                          <span className="cc-provider-name">{provider.name}</span>
                        </div>
                      </div>
                      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                        {target !== "opencode" && isCurrent && (
                          <Tag color="success" style={{ margin: 0, borderRadius: 999, paddingInline: 10, fontSize: 11 }}>
                            🟢 {t("providers.current")}
                          </Tag>
                        )}
                        {provider.healthStatus && provider.healthLatencyMs != null && (
                          <Tag
                            color={provider.healthStatus === "healthy" ? "success" : "error"}
                            style={{ borderRadius: 6, fontSize: 11, margin: 0 }}
                          >
                            {provider.healthLatencyMs}ms
                          </Tag>
                        )}
                      </div>
                    </div>

                    {/* Metrics Rail */}
                    <div className="cc-provider-card-metrics">
                      <div className="cc-metric-item">
                        <span className="cc-metric-label">Model</span>
                        <span className="cc-metric-value" style={{ fontSize: 12 }}>{provider.model || "Default"}</span>
                      </div>
                      <div className="cc-metric-item">
                        <span className="cc-metric-label">Protocol</span>
                        <span className="cc-metric-value" style={{ fontSize: 12 }}>{provider.protocolType}</span>
                      </div>
                      <div className="cc-metric-item">
                        <span className="cc-metric-label">Latency</span>
                        <span className="cc-metric-value" style={{ fontSize: 12 }}>
                          {provider.healthLatencyMs != null ? `${provider.healthLatencyMs}ms` : "--"}
                        </span>
                      </div>
                    </div>

                    {/* Footer Row */}
                    <div className="cc-provider-card-footer">
                      <Text type="secondary" ellipsis style={{ maxWidth: 220, fontSize: 11 }}>
                        {provider.baseUrl}
                      </Text>
                      <div className="cc-provider-actions" style={{ display: "flex", alignItems: "center", gap: 6 }}>
                        {target !== "opencode" && !isCurrent && (
                          <Button
                            type="primary"
                            size="small"
                            style={{ borderRadius: 6, fontSize: 12 }}
                            loading={switchingId === provider.id}
                            onClick={() => void handleSwitch(provider)}
                          >
                            {t("providers.switchTo")}
                          </Button>
                        )}
                        <Space size={2}>
                          <Tooltip title={t("providers.testConnection")}>
                            <Button
                              size="small"
                              type="text"
                              loading={testingId === provider.id}
                              icon={<ThunderboltOutlined />}
                              onClick={() => void handleTest(provider)}
                            />
                          </Tooltip>
                          <Tooltip title={t("common.edit")}>
                            <Button
                              size="small"
                              type="text"
                              icon={<EditOutlined />}
                              onClick={() => openEdit(provider)}
                            />
                          </Tooltip>
                          <Tooltip title={t("deeplink.copyLink")}>
                            <Button
                              size="small"
                              type="text"
                              icon={<CopyOutlined />}
                              onClick={() => void handleShareLink(provider)}
                            />
                          </Tooltip>
                          <Popconfirm
                            title={t("providers.deleteConfirmTitle")}
                            description={t("providers.deleteConfirmDesc")}
                            onConfirm={() => void handleDelete(provider)}
                            okText={t("common.delete")}
                            cancelText={t("common.cancel")}
                          >
                            <Tooltip title={t("common.delete")}>
                              <Button size="small" type="text" danger icon={<DeleteOutlined />} />
                            </Tooltip>
                          </Popconfirm>
                        </Space>
                      </div>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </>
      ) : (
        /* Usage Analytics Home View */
        <div className="workbench-usage-section" style={{ borderTop: "none", paddingTop: 0, overflowY: "auto" }}>
          <div className="workbench-section-header">
            <Title level={5} style={{ margin: 0 }}>
              {t("workbench.usageSection")}
            </Title>
            {sourceFilterToolbar}
          </div>

          {dashboardQuery.error ? (
            <Alert type="error" showIcon message={errMsg(dashboardQuery.error)} />
          ) : (
            <UsageSummaryGrid
              estimatedCost={summary?.estimatedCost ?? 0}
              costCurrency={summary?.estimatedCostCurrency}
              totalTokens={totalTokens}
              tokensVsYesterday={tokensVsYesterday}
              requestCount={summary?.requestCount ?? 0}
              successfulRequestCount={summary?.successfulRequestCount ?? 0}
            />
          )}

          <div className="workbench-charts-grid">
            <Card
              size="small"
              className="page-surface workbench-chart-card"
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
              className="page-surface workbench-chart-card"
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
      )}

      {/* Floating View Switcher Button */}
      <FloatingViewSwitcher />

      {/* Modals for Create/Edit and Import Preview */}
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
    </div>
  );
}

function UsageSummaryGrid({
  estimatedCost,
  costCurrency,
  totalTokens,
  tokensVsYesterday,
  requestCount,
  successfulRequestCount,
}: {
  estimatedCost: number;
  costCurrency?: string | null;
  totalTokens: number;
  tokensVsYesterday?: number | null;
  requestCount: number;
  successfulRequestCount: number;
}) {
  const { t } = useTranslation();
  const rate = successRate(requestCount, successfulRequestCount);

  return (
    <div className="usage-summary-grid">
      <div className="usage-summary-cell">
        <div className="usage-cell-icon usage-cell-icon-cost">
          <PayCircleOutlined />
        </div>
        <div className="usage-cell-content">
          <div className="usage-cell-label">{t("usage.estimatedCost")}</div>
          <div className="usage-hero-value">
            {currencyPrefix(costCurrency)}
            {estimatedCost.toFixed(4)}
          </div>
        </div>
      </div>
      <div className="usage-summary-cell">
        <div className="usage-cell-icon usage-cell-icon-tokens">
          <ThunderboltOutlined />
        </div>
        <div className="usage-cell-content">
          <div className="usage-cell-label">{t("usage.totalTokens")}</div>
          <div className="usage-hero-value usage-value-tokens">
            {formatCompactNumber(totalTokens)}
          </div>
          {tokensVsYesterday != null && (
            <div className="usage-hero-sub">
              {t(
                tokensVsYesterday >= 0
                  ? "usage.totalTokensVsYesterdayUp"
                  : "usage.totalTokensVsYesterdayDown",
                { pct: Math.abs(tokensVsYesterday) },
              )}
            </div>
          )}
        </div>
      </div>
      <div className="usage-summary-cell">
        <div className="usage-cell-icon usage-cell-icon-requests">
          <BarChartOutlined />
        </div>
        <div className="usage-cell-content">
          <div className="usage-cell-label">{t("usage.requests")}</div>
          <div className="usage-hero-value usage-value-requests">
            {t("usage.requestCountUnit", { count: requestCount })}
          </div>
        </div>
      </div>
      <div className="usage-summary-cell">
        <div className="usage-cell-icon usage-cell-icon-rate">
          <CheckCircleOutlined />
        </div>
        <div className="usage-cell-content">
          <div className="usage-cell-label">{t("usage.successRate")}</div>
          <div className="usage-hero-value usage-value-rate">{rate}%</div>
        </div>
      </div>
    </div>
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
