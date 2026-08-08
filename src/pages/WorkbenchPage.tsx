import { useEffect, useState } from "react";
import {
  Alert,
  App as AntApp,
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
import LineChartOutlined from "@ant-design/icons/es/icons/LineChartOutlined";
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
import { usageSourceIcon } from "@/components/UsageSourceIcons";
import {
  managedAppsRuntimeStatusOptions,
  proxyStatusOptions,
  usageDashboardOptions,
  usageTrendOptions,
} from "@/lib/appQueries";
import { useNavigatePage } from "@/lib/navigation";
import { showCodexSwitchNotice } from "@/lib/codexNotice";
import { useProvidersStore } from "@/stores/providersStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { ImportPreview, Provider, ProviderInput } from "@/types/backend";
import {
  buildProviderDeeplink,
  confirmImportPreview,
  exportProviders,
  getAntigravityGatewayStatus,
  previewImportText,
  speedtestProviderEndpoint,
  testProviderConnection,
} from "@/services/api";
import { formatCompactNumber } from "@/utils/formatCompact";

const { Text, Title } = Typography;

export default function WorkbenchPage() {
  const { t } = useTranslation();
  const { message } = AntApp.useApp();
  const navigate = useNavigatePage();
  const store = useProvidersStore();

  const target = usePagePreferencesStore((state) => state.providersTarget);
  const setTarget = usePagePreferencesStore((state) => state.setProvidersTarget);
  const heatmapSource = usePagePreferencesStore((state) => state.heatmapSource);
  const setHeatmapSource = usePagePreferencesStore((state) => state.setHeatmapSource);

  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Provider | null>(null);
  const [busy, setBusy] = useState(false);
  const [switchingId, setSwitchingId] = useState<string | null>(null);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [batchTesting, setBatchTesting] = useState(false);
  const [importPreview, setImportPreview] = useState<ImportPreview | null>(null);
  const [importConfirming, setImportConfirming] = useState(false);

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

  const openCreate = () => {
    setEditing(null);
    setFormOpen(true);
  };

  const openEdit = (provider: Provider) => {
    setEditing(provider);
    setFormOpen(true);
  };

  const handleSubmit = async (input: ProviderInput) => {
    // Do not flip the shared card `busy` flag — that made every Switch button spin.
    try {
      if (editing) {
        await store.update(input);
        void message.success(t("providers.updated"));
      } else {
        await store.create(input);
        void message.success(t("providers.created"));
      }
      setFormOpen(false);
    } catch (e) {
      void message.error(errMsg(e));
      throw e;
    }
  };

  const handleSwitch = async (provider: Provider) => {
    if (!provider.apiKeySet) {
      void message.warning(t("providers.missingKey"));
      return;
    }
    setSwitchingId(provider.id);
    try {
      const result = await store.switchTo(provider.id);
      void message.success(t("providers.switched", { name: provider.name }));
      showCodexSwitchNotice(result.codexNotice, message, t);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setSwitchingId(null);
    }
  };

  const handleOfficial = async () => {
    setSwitchingId("official");
    try {
      await store.useOfficial();
      void message.success(t("providers.switchedOfficial"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setSwitchingId(null);
    }
  };

  const handleTest = async (provider: Provider) => {
    setTestingId(provider.id);
    try {
      const result = await testProviderConnection(provider.id);
      const notify = result.ok ? message.success : message.error;
      void notify(
        result.latencyMs != null
          ? `${result.message} · ${t("providers.latencyMs", { ms: result.latencyMs })}`
          : result.message,
      );
      useProvidersStore.setState((state) => ({
        providers: state.providers.map((item) =>
          item.id === provider.id
            ? {
                ...item,
                healthStatus: result.ok ? "healthy" : "error",
                healthCheckedAt: result.checkedAt,
                healthLatencyMs: result.latencyMs ?? null,
              }
            : item,
        ),
      }));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setTestingId(null);
    }
  };

  const handleSpeedtestAll = async () => {
    setBatchTesting(true);
    try {
      let count = 0;
      for (const p of store.providers) {
        try {
          const res = await speedtestProviderEndpoint(p.id);
          if (res.ok) count++;
          useProvidersStore.setState((state) => ({
            providers: state.providers.map((item) =>
              item.id === p.id
                ? {
                    ...item,
                    healthLatencyMs: res.latencyMs ?? item.healthLatencyMs ?? null,
                  }
                : item,
            ),
          }));
        } catch {
          // ignore individual errors during batch test
        }
      }
      void message.success(
        t("providers.speedtestAllDone", {
          defaultValue: `已完成 ${store.providers.length} 个供应商测速（成功 ${count}）`,
          total: store.providers.length,
          ok: count,
        }),
      );
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBatchTesting(false);
    }
  };

  const handleShareLink = async (provider: Provider) => {
    try {
      const link = await buildProviderDeeplink(provider.id);
      await navigator.clipboard.writeText(link);
      void message.success(t("deeplink.linkCopied"));
    } catch (e) {
      void message.error(errMsg(e));
    }
  };

  const handleDelete = async (provider: Provider) => {
    setBusy(true);
    try {
      await store.remove(provider.id);
      void message.success(t("providers.deleted"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleExport = async () => {
    try {
      const json = await exportProviders(target);
      const url = URL.createObjectURL(new Blob([json], { type: "application/json" }));
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `claude-switcher-providers-${target}.json`;
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      void message.error(errMsg(e));
    }
  };

  const handleSyncOpenCodeLive = async () => {
    setBusy(true);
    try {
      await store.importLive();
      void message.success(t("providers.syncOpenCodeLiveDone"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleImportClipboard = async () => {
    setBusy(true);
    try {
      const text = await navigator.clipboard.readText();
      const preview = await previewImportText(text);
      setImportPreview(preview);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleConfirmImport = async () => {
    if (!importPreview) return;
    setImportConfirming(true);
    try {
      const result = await confirmImportPreview(importPreview);
      void message.success(
        t("providers.importSummary", { imported: result.imported, skipped: result.skipped }),
      );
      setImportPreview(null);
      await store.load(target);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setImportConfirming(false);
    }
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
    <div className="cc-workbench-container">
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
            color={proxy?.running ? "green" : undefined}
            style={{ cursor: "pointer", margin: 0 }}
            onClick={() => navigate("proxy")}
          >
            {proxy?.running ? t("workbench.proxyRunning", { port: proxy.port }) : t("workbench.proxyStopped")}
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
              onClick={() => void handleSyncOpenCodeLive()}
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
        {/* Official Provider Card — OpenCode 多供应商并存，无需官方/切换 */}
        {target !== "opencode" && (
        <div className={`cc-provider-card ${officialCurrent ? "cc-provider-card-active" : ""}`}>
          <div className="cc-provider-card-body">
            <div className="cc-provider-main">
              <div className="cc-provider-icon">
                {usageSourceIcon(target, { size: 22 })}
              </div>
              <div className="cc-provider-info">
                <div className="cc-provider-title-row">
                  <span className="cc-provider-name">{t("providers.officialMode")}</span>
                  {officialCurrent && (
                    <Tag color="success" style={{ margin: 0, borderRadius: 999, paddingInline: 10, fontSize: 12 }}>
                      {t("providers.current")}
                    </Tag>
                  )}
                </div>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {t("providers.officialModeHint", { defaultValue: "使用官方原生 API Endpoint / 账号凭据" })}
                </Text>
              </div>
            </div>
            <div className="cc-provider-actions">
              {!officialCurrent && (
                <Button type="primary" size="middle" style={{ borderRadius: 8 }} loading={switchingId === "official"} onClick={() => void handleOfficial()}>
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
                <div className="cc-provider-main">
                  <div className="cc-provider-icon">
                    {usageSourceIcon(provider.targetApp, { size: 22 })}
                  </div>
                  <div className="cc-provider-info">
                    <div className="cc-provider-title-row">
                      <span className="cc-provider-name">{provider.name}</span>
                      {target !== "opencode" && isCurrent && (
                        <Tag color="success" style={{ margin: 0, borderRadius: 999, paddingInline: 10, fontSize: 12 }}>
                          {t("providers.current")}
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
                    <div className="cc-provider-meta">
                      <Text type="secondary">{provider.model}</Text>
                      <Text type="secondary" ellipsis style={{ maxWidth: 280 }}>
                        {provider.baseUrl}
                      </Text>
                      <Tag color={provider.protocolType === "anthropic" ? "processing" : "warning"} style={{ borderRadius: 6, fontSize: 11, margin: 0 }}>
                        {provider.protocolType}
                      </Tag>
                    </div>
                  </div>
                </div>
                <div className="cc-provider-actions">
                  {target !== "opencode" && !isCurrent && (
                    <Button
                      type="primary"
                      size="middle"
                      style={{ borderRadius: 8 }}
                      loading={switchingId === provider.id}
                      onClick={() => void handleSwitch(provider)}
                    >
                      {t("providers.switchTo")}
                    </Button>
                  )}
                  <Space size={4}>
                    <Tooltip title={t("providers.testConnection")}>
                      <Button
                        size="middle"
                        type="text"
                        loading={testingId === provider.id}
                        icon={<ThunderboltOutlined />}
                        onClick={() => void handleTest(provider)}
                      />
                    </Tooltip>
                    <Tooltip title={t("common.edit")}>
                      <Button
                        size="middle"
                        type="text"
                        icon={<EditOutlined />}
                        onClick={() => openEdit(provider)}
                      />
                    </Tooltip>
                    <Tooltip title={t("deeplink.copyLink")}>
                      <Button
                        size="middle"
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
                        <Button size="middle" type="text" danger icon={<DeleteOutlined />} />
                      </Tooltip>
                    </Popconfirm>
                  </Space>
                </div>
              </div>
            </div>
          );
        })}
      </div>

      {/* Bottom Section: Usage Analytics */}
      <div className="workbench-usage-section">
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

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
