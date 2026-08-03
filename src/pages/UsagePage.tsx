import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  Alert,
  Button,
  Card,
  Col,
  Descriptions,
  Drawer,
  Empty,
  Form,
  Input,
  InputNumber,
  Modal,
  Row,
  Select,
  Space,
  Statistic,
  Switch,
  Table,
  Tag,
  Typography,
  message,
  theme,
} from "antd";
import DollarOutlined from "@ant-design/icons/es/icons/DollarOutlined";
import ExpandOutlined from "@ant-design/icons/es/icons/ExpandOutlined";
import LineChartOutlined from "@ant-design/icons/es/icons/LineChartOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import UnorderedListOutlined from "@ant-design/icons/es/icons/UnorderedListOutlined";
import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  Area,
  AreaChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip as RechartsTooltip,
  XAxis,
  YAxis,
} from "recharts";
import type {
  LogMaintenancePolicy,
  LogMaintenancePreview,
  ModelPricing,
  ModelPricingInput,
  PricingImportPreview,
  PaginatedProxyLogs,
  UsageDashboard,
} from "@/types/backend";
import {
  deleteModelPricing,
  exportModelPricingXlsx,
  importModelPricingXlsx,
  maintainProxyLogs,
  previewProxyLogMaintenance,
  previewModelPricingXlsx,
  rebuildCodexSessionUsage,
  rebuildClaudeCodeSessionUsage,
  saveModelPricing,
  saveLogMaintenancePolicy,
  syncCodexSessionUsage,
  syncClaudeCodeSessionUsage,
} from "@/services/api";
import { usageDashboardOptions, usageLogsOptions, usageMetaOptions } from "@/lib/appQueries";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { OnboardingTip } from "@/components/OnboardingTip";
import { UsageSourceFilterSegmented } from "@/components/UsageSourceFilterSegmented";
import { formatCompactNumber } from "@/utils/formatCompact";
import { USAGE_PERIOD_VALUES, usagePeriodGranularity, usagePeriodHourKeys, usagePeriodLabelKey, trendBucketLabel } from "@/utils/usagePeriod";
import type { UsagePeriod } from "@/utils/usagePeriod";

const { Text } = Typography;

function invalidateUsageQueries(queryClient: ReturnType<typeof useQueryClient>) {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: ["usage-dashboard"] }),
    queryClient.invalidateQueries({ queryKey: ["usage-logs"] }),
    queryClient.invalidateQueries({ queryKey: ["usage-meta"] }),
    queryClient.invalidateQueries({ queryKey: ["usage-trend"] }),
  ]);
}

export default function UsagePage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const period = usePagePreferencesStore((state) => state.usagePeriod);
  const setPeriod = usePagePreferencesStore((state) => state.setUsagePeriod);
  const logPage = usePagePreferencesStore((state) => state.usageLogPage);
  const setLogPage = usePagePreferencesStore((state) => state.setUsageLogPage);
  const logTargetApp = usePagePreferencesStore((state) => state.usageLogTarget);
  const setLogTargetApp = usePagePreferencesStore((state) => state.setUsageLogTarget);
  const [pricingManagerOpen, setPricingManagerOpen] = useState(false);
  const [pricingFormOpen, setPricingFormOpen] = useState(false);
  const [pricingImportPath, setPricingImportPath] = useState<string | null>(null);
  const [pricingImportPreview, setPricingImportPreview] = useState<PricingImportPreview | null>(null);
  const [pricingImportOpen, setPricingImportOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [maintaining, setMaintaining] = useState(false);
  const [maintenanceOpen, setMaintenanceOpen] = useState(false);
  const [maintenancePolicy, setMaintenancePolicy] = useState<LogMaintenancePolicy | null>(null);
  const [maintenancePreview, setMaintenancePreview] = useState<LogMaintenancePreview | null>(null);
  const [form] = Form.useForm<ModelPricingInput>();
  const [detailDiagnostic, setDetailDiagnostic] = useState<string | null>(null);
  const [trendExpanded, setTrendExpanded] = useState(false);

  const dashboardQuery = useQuery({
    ...usageDashboardOptions(period, logTargetApp),
    placeholderData: keepPreviousData,
  });
  const logsQuery = useQuery({
    ...usageLogsOptions(period, logPage, logTargetApp),
    placeholderData: keepPreviousData,
  });
  const metaQuery = useQuery(usageMetaOptions);

  const dashboard = dashboardQuery.data ?? null;
  const pricing = metaQuery.data?.pricing ?? [];
  const requestLogs = logsQuery.data ?? null;
  const pageError = dashboardQuery.error ?? logsQuery.error ?? metaQuery.error;

  useEffect(() => {
    if (!maintenanceOpen && metaQuery.data?.maintenancePolicy) {
      setMaintenancePolicy(metaQuery.data.maintenancePolicy);
    }
  }, [maintenanceOpen, metaQuery.data?.maintenancePolicy]);

  useEffect(() => {
    if (!maintenanceOpen || !maintenancePolicy) return;
    void previewProxyLogMaintenance(maintenancePolicy)
      .then(setMaintenancePreview)
      .catch((e) => void message.error(errMsg(e)));
  }, [maintenanceOpen, maintenancePolicy]);

  useEffect(() => {
    const interval = window.setInterval(() => {
      if (
        document.visibilityState === "visible" &&
        !saving &&
        !maintaining
      ) {
        void dashboardQuery.refetch();
        void logsQuery.refetch();
      }
    }, 60_000);
    return () => window.clearInterval(interval);
  }, [dashboardQuery.refetch, logsQuery.refetch, maintaining, saving]);

  const savePricing = async () => {
    try {
      const values = await form.validateFields();
      setSaving(true);
      await saveModelPricing(values);
      setPricingFormOpen(false);
      form.resetFields();
      void message.success(t("usage.pricingSaved"));
      await invalidateUsageQueries(queryClient);
    } catch (e) {
      if (e instanceof Error) void message.error(errMsg(e));
    } finally {
      setSaving(false);
    }
  };

  const removePricing = async (model: string) => {
    try {
      await deleteModelPricing(model);
      void message.success(t("usage.pricingDeleted"));
      await invalidateUsageQueries(queryClient);
    } catch (e) {
      void message.error(errMsg(e));
    }
  };

  const exportPricing = async () => {
    try {
      const destination = await save({
        defaultPath: "AI-Switcher-model-pricing.xlsx",
        filters: [{ name: "Excel", extensions: ["xlsx"] }],
      });
      if (!destination) return;
      const path = await exportModelPricingXlsx(destination);
      void message.success(t("usage.pricingExported", { path }));
    } catch (error) {
      void message.error(errMsg(error));
    }
  };

  const selectPricingImport = async () => {
    try {
      const selected = await open({ multiple: false, filters: [{ name: "Excel", extensions: ["xlsx"] }] });
      if (!selected || Array.isArray(selected)) return;
      const preview = await previewModelPricingXlsx(selected);
      setPricingImportPath(selected);
      setPricingImportPreview(preview);
      setPricingImportOpen(true);
    } catch (error) {
      void message.error(errMsg(error));
    }
  };

  const applyPricingImport = async () => {
    if (!pricingImportPath || !pricingImportPreview || pricingImportPreview.errors.length) return;
    setSaving(true);
    try {
      const result = await importModelPricingXlsx(pricingImportPath);
      setPricingImportOpen(false);
      setPricingImportPath(null);
      setPricingImportPreview(null);
      void message.success(t("usage.pricingImported", { count: result.validRows }));
      await invalidateUsageQueries(queryClient);
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setSaving(false);
    }
  };

  const openMaintenance = async () => {
    try {
      const preview = await previewProxyLogMaintenance(maintenancePolicy ?? undefined);
      setMaintenancePreview(preview);
      setMaintenanceOpen(true);
    } catch (e) { void message.error(errMsg(e)); }
  };

  const maintainLogs = async () => {
    if (!maintenancePolicy) return;
    setMaintaining(true);
    try {
      await saveLogMaintenancePolicy(maintenancePolicy);
      const result = await maintainProxyLogs(true);
      void message.success(t("usage.logsMaintained", { deleted: result.deleted }));
      if (!result.integrityOk) void message.error(t("usage.integrityFailed"));
      setMaintenanceOpen(false);
      await invalidateUsageQueries(queryClient);
    } catch (e) { void message.error(errMsg(e)); }
    finally { setMaintaining(false); }
  };

  const refreshOverview = async () => {
    setRefreshing(true);
    try {
      await Promise.all([dashboardQuery.refetch(), logsQuery.refetch(), metaQuery.refetch()]);
    } finally {
      setRefreshing(false);
    }
  };

  const summary = dashboard?.summary;
  const totalTokens =
    (summary?.inputTokens ?? 0) +
    (summary?.cacheReadInputTokens ?? 0) +
    (summary?.cacheCreationInputTokens ?? 0) +
    (summary?.outputTokens ?? 0);
  const includesCodex = logTargetApp === "all" || logTargetApp === "codex";
  const isCodexOnly = logTargetApp === "codex";
  const includesClaudeCode = logTargetApp === "all" || logTargetApp === "claude_code";
  const isClaudeCodeOnly = logTargetApp === "claude_code";
  const localClaude = dashboard?.localClaudeCode;
  const emptyClaudeCode = includesClaudeCode && (summary?.requestCount ?? 0) === 0;

  const syncCodexSessions = async () => {
    setRefreshing(true);
    try {
      const result = await syncCodexSessionUsage();
      void message.success(
        t("usage.codexSyncDone", {
          inserted: result.insertedRows,
          scanned: result.scannedFiles,
        }),
      );
      await invalidateUsageQueries(queryClient);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setRefreshing(false);
    }
  };

  const rebuildCodexSessions = async () => {
    setRefreshing(true);
    try {
      const result = await rebuildCodexSessionUsage();
      void message.success(
        t("usage.codexRebuildDone", {
          inserted: result.insertedRows,
          scanned: result.scannedFiles,
        }),
      );
      await invalidateUsageQueries(queryClient);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setRefreshing(false);
    }
  };

  const syncClaudeCodeSessions = async () => {
    setRefreshing(true);
    try {
      const result = await syncClaudeCodeSessionUsage();
      void message.success(
        t("usage.claudeCodeSyncDone", {
          inserted: result.insertedRows,
          scanned: result.scannedFiles,
        }),
      );
      await invalidateUsageQueries(queryClient);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setRefreshing(false);
    }
  };

  const rebuildClaudeCodeSessions = async () => {
    setRefreshing(true);
    try {
      const result = await rebuildClaudeCodeSessionUsage();
      void message.success(
        t("usage.claudeCodeRebuildDone", {
          inserted: result.insertedRows,
          scanned: result.scannedFiles,
        }),
      );
      await invalidateUsageQueries(queryClient);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        {pageError && <Alert type="error" showIcon message={errMsg(pageError)} />}
        <OnboardingTip tipKey="usage" message={t("usage.title")} description={t("usage.description")} />
        {includesCodex && (
          <OnboardingTip
            tipKey="usage_codex_local"
            type={dashboard?.localCodex.available ? "info" : isCodexOnly ? "warning" : "info"}
            message={t("usage.codexLocalTitle")}
            description={dashboard?.localCodex.available
              ? t("usage.codexLocalAvailable", { events: dashboard.localCodex.eventCount, sessions: dashboard.localCodex.sessionCount })
              : t("usage.codexLocalUnavailable")}
          />
        )}
        {includesClaudeCode && (
          <OnboardingTip
            tipKey="usage_claude_code_local"
            type={localClaude?.available ? "info" : isClaudeCodeOnly ? "warning" : "info"}
            message={t("usage.claudeCodeLocalTitle")}
            description={localClaude?.available
              ? t("usage.claudeCodeLocalAvailable", { events: localClaude.eventCount, sessions: localClaude.sessionCount })
              : t("usage.claudeCodeLocalUnavailable")}
          />
        )}
        {emptyClaudeCode && (
          <Alert
            type="info"
            showIcon
            message={t("usage.claudeCodeEmptyTitle")}
            description={t("usage.claudeCodeEmptyHint")}
            action={
              <Button size="small" loading={refreshing} onClick={() => void syncClaudeCodeSessions()}>
                {t("usage.syncClaudeCodeSessions")}
              </Button>
            }
          />
        )}
        {includesCodex && !dashboard?.localCodex.available && (
          <Alert
            type={isCodexOnly ? "warning" : "info"}
            showIcon
            message={t("usage.codexEmptyTitle")}
            description={t("usage.codexEmptyHint")}
            action={
              <Button size="small" loading={refreshing} onClick={() => void syncCodexSessions()}>
                {t("usage.syncCodexSessions")}
              </Button>
            }
          />
        )}
        <OnboardingTip tipKey="usage_currency" type="info" message={t("usage.currencyLimit")} />
        <OnboardingTip tipKey="usage_cache_pricing" message={t("usage.cachePricingIncluded")} />

        <div
          style={{
            display: "flex",
            flexWrap: "wrap",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 12,
            width: "100%",
          }}
        >
          <Space wrap size={[12, 8]} align="center">
            <Space size={8} align="center">
              <Text type="secondary">{t("usage.period")}</Text>
              <Select
                size="middle"
                value={period}
                style={{ width: 160 }}
                options={USAGE_PERIOD_VALUES.map((value) => ({
                  value,
                  label:
                    typeof value === "number"
                      ? t("usage.lastDays", { days: value })
                      : t(usagePeriodLabelKey(value)),
                }))}
                onChange={(value) => {
                  setPeriod(value);
                  setLogPage(0);
                }}
              />
            </Space>
            <Space size={8} align="center">
              <Text type="secondary">{t("usage.statsSource")}</Text>
              <UsageSourceFilterSegmented
                value={logTargetApp}
                onChange={(value) => {
                  setLogTargetApp(value);
                  setLogPage(0);
                }}
                t={t}
              />
            </Space>
            <Button
              size="middle"
              icon={<ReloadOutlined />}
              loading={refreshing}
              onClick={() => void refreshOverview()}
            >
              {t("common.refresh")}
            </Button>
          </Space>
          <Space wrap size={8}>
            {includesCodex && (
              <>
                <Button loading={refreshing} onClick={() => void syncCodexSessions()}>
                  {t("usage.syncCodexSessions")}
                </Button>
                <Button loading={refreshing} onClick={() => void rebuildCodexSessions()}>
                  {t("usage.rebuildCodexSessions")}
                </Button>
              </>
            )}
            {includesClaudeCode && (
              <>
                <Button loading={refreshing} onClick={() => void syncClaudeCodeSessions()}>
                  {t("usage.syncClaudeCodeSessions")}
                </Button>
                <Button loading={refreshing} onClick={() => void rebuildClaudeCodeSessions()}>
                  {t("usage.rebuildClaudeCodeSessions")}
                </Button>
              </>
            )}
            <Button icon={<DollarOutlined />} onClick={() => setPricingManagerOpen(true)}>
              {t("usage.configurePricing")}
            </Button>
            <Button loading={maintaining} onClick={() => void openMaintenance()}>
              {t("usage.maintainLogs")}
            </Button>
          </Space>
        </div>

        <Row gutter={[16, 16]}>
          <Metric title={t("usage.requests")} value={summary?.requestCount ?? 0} icon={<ThunderboltOutlined />} />
          <Metric title={t("usage.successRate")} value={successRate(summary?.requestCount, summary?.successfulRequestCount)} suffix="%" />
          <Metric title={t("usage.totalTokens")} value={totalTokens} compact />
          <Metric
            title={t("usage.estimatedCost")}
            value={summary?.estimatedCost ?? 0}
            precision={4}
            prefix={currencyPrefix(summary?.estimatedCostCurrency)}
            icon={<DollarOutlined />}
          />
        </Row>
        {(summary?.estimatedCostsByCurrency?.length ?? 0) > 1 && (
          <Alert
            type="info"
            showIcon
            message={t("usage.multiCurrencyTitle")}
            description={summary?.estimatedCostsByCurrency
              .map((entry) => `${currencyPrefix(entry.currency)}${entry.amount.toFixed(4)} (${entry.currency})`)
              .join(" · ")}
          />
        )}

        <Card
          size="small"
          title={<Space><LineChartOutlined />{t("usage.trendChart")}</Space>}
          extra={<Button size="small" icon={<ExpandOutlined />} onClick={() => setTrendExpanded(true)}>{t("usage.expandChart")}</Button>}
        >
          <UsageTrendChart data={dashboard?.trend ?? []} period={period} granularity={dashboard?.trendGranularity ?? usagePeriodGranularity(period)} t={t} />
        </Card>

        <Row gutter={[16, 16]}>
          <Col xs={24} lg={12}>
            <BreakdownCard title={t("usage.byProvider")} data={dashboard?.byProvider ?? []} t={t} />
          </Col>
          <Col xs={24} lg={12}>
            <BreakdownCard title={t("usage.byModel")} data={dashboard?.byModel ?? []} t={t} />
          </Col>
        </Row>

        <Card
          size="small"
          title={<Space><UnorderedListOutlined />{t("usage.requestLogs")}</Space>}
        >
          {isCodexOnly && (
            <Alert
              type="info"
              showIcon
              style={{ marginBottom: 12 }}
              message={t("usage.codexRequestLogsHint")}
            />
          )}
          <Table
            size="small"
            rowKey="id"
            locale={{ emptyText: t("usage.noData") }}
            dataSource={requestLogs?.data ?? []}
            loading={logsQuery.isPending && !logsQuery.data}
            pagination={{
              current: (requestLogs?.page ?? 0) + 1,
              pageSize: requestLogs?.pageSize ?? 20,
              total: requestLogs?.total ?? 0,
              showSizeChanger: false,
              onChange: (page) => setLogPage(page - 1),
            }}
            onRow={(row) => ({
              onClick: () => {
                if (row.diagnostic) setDetailDiagnostic(row.diagnostic);
              },
              style: { cursor: row.diagnostic ? "pointer" : "default" },
            })}
            columns={[
              {
                title: t("usage.logTime"),
                dataIndex: "createdAt",
                width: 170,
                render: (v: number) => new Date(v).toLocaleString(),
              },
              {
                title: t("usage.logApp"),
                dataIndex: "targetApp",
                width: 120,
                render: (v: string | null) => v ?? "—",
              },
              {
                title: t("usage.logProvider"),
                dataIndex: "providerName",
                ellipsis: true,
                render: (v: string | null, row: PaginatedProxyLogs["data"][number]) =>
                  row.dataSource === "codex_session"
                    ? t("usage.codexSessionSource")
                    : (v ?? "—"),
              },
              {
                title: t("usage.model"),
                dataIndex: "model",
                ellipsis: true,
                render: (v: string | null) => v ?? "—",
              },
              {
                title: t("usage.logStatus"),
                dataIndex: "statusCode",
                width: 80,
                render: (v: number | null) => {
                  if (v === null) return "—";
                  const color = v >= 200 && v < 300 ? "green" : "red";
                  return <Tag color={color}>{v}</Tag>;
                },
              },
              {
                title: t("usage.errorSource"),
                dataIndex: "errorCategory",
                width: 105,
                render: (value: string | null) =>
                  value ? <Tag color={value === "upstream" ? "orange" : "red"}>{value}</Tag> : "—",
              },
              {
                title: t("usage.logTokens"),
                render: (_: unknown, row: PaginatedProxyLogs["data"][number]) =>
                  row.usageAvailable
                    ? `${formatCompactNumber(row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens)} / ${formatCompactNumber(row.outputTokens)}${row.cacheReadInputTokens ? ` (${t("usage.cached")}: ${formatCompactNumber(row.cacheReadInputTokens)})` : ""}`
                    : <Text type="secondary">{t("usage.usageUnavailable")}</Text>,
              },
              {
                title: t("usage.logDuration"),
                dataIndex: "durationMs",
                width: 90,
                render: (v: number) => `${v}ms`,
              },
              {
                title: t("usage.logStream"),
                dataIndex: "isStream",
                width: 70,
                render: (v: boolean) => (v ? t("common.enabled") : "—"),
              },
            ]}
          />
        </Card>

      </Space>

      <Modal
        title={t("usage.pricing")}
        open={pricingManagerOpen}
        width="min(1100px, calc(100vw - 32px))"
        footer={<Button onClick={() => setPricingManagerOpen(false)}>{t("common.cancel")}</Button>}
        onCancel={() => { setPricingManagerOpen(false); setPricingFormOpen(false); form.resetFields(); }}
      >
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <Space style={{ justifyContent: "space-between", width: "100%" }}>
            <Text type="secondary">{t("usage.cachePricingIncluded")}</Text>
            <Space wrap>
              <Button onClick={() => void exportPricing()}>{t("usage.exportPricing")}</Button>
              <Button onClick={() => void selectPricingImport()}>{t("usage.importPricing")}</Button>
              <Button type="primary" icon={<PlusOutlined />} onClick={() => setPricingFormOpen(true)}>{t("usage.addPricing")}</Button>
            </Space>
          </Space>
          {pricingFormOpen && <Card size="small" title={t("usage.addPricing")}>
            <Form form={form} layout="vertical" initialValues={{ currency: "USD", inputPricePerMillion: 0, cacheReadPricePerMillion: 0, cacheWritePricePerMillion: 0, outputPricePerMillion: 0, batchInputPricePerMillion: 0, batchOutputPricePerMillion: 0 }}>
              <Row gutter={12}>
                <Col xs={24} md={12}><Form.Item name="model" label={t("usage.model")} rules={[{ required: true, message: t("usage.requiredModel") }]}><Input placeholder="claude-sonnet-4" /></Form.Item></Col>
                <Col xs={24} md={12}><Form.Item name="provider" label={t("usage.pricingProvider")}><Input placeholder="Anthropic" /></Form.Item></Col>
                <Col xs={24} md={12}><Form.Item name="inputPricePerMillion" label={t("usage.inputPrice")} rules={[{ required: true }]}><InputNumber min={0} precision={6} style={{ width: "100%" }} /></Form.Item></Col>
                <Col xs={24} md={12}><Form.Item name="outputPricePerMillion" label={t("usage.outputPrice")} rules={[{ required: true }]}><InputNumber min={0} precision={6} style={{ width: "100%" }} /></Form.Item></Col>
                <Col xs={24} md={12}><Form.Item name="cacheReadPricePerMillion" label={t("usage.cacheReadPrice")}><InputNumber min={0} precision={6} style={{ width: "100%" }} /></Form.Item></Col>
                <Col xs={24} md={12}><Form.Item name="cacheWritePricePerMillion" label={t("usage.cacheWritePrice")}><InputNumber min={0} precision={6} style={{ width: "100%" }} /></Form.Item></Col>
                <Col xs={24} md={12}><Form.Item name="batchInputPricePerMillion" label={t("usage.batchInputPrice")}><InputNumber min={0} precision={6} style={{ width: "100%" }} /></Form.Item></Col>
                <Col xs={24} md={12}><Form.Item name="batchOutputPricePerMillion" label={t("usage.batchOutputPrice")}><InputNumber min={0} precision={6} style={{ width: "100%" }} /></Form.Item></Col>
                <Col xs={24} md={12}><Form.Item name="currency" label={t("usage.currency")} rules={[{ required: true }]}><Input maxLength={12} /></Form.Item></Col>
              </Row>
              <Space>
                <Button type="primary" loading={saving} onClick={() => void savePricing()}>{t("common.save")}</Button>
                <Button onClick={() => { setPricingFormOpen(false); form.resetFields(); }}>{t("common.cancel")}</Button>
              </Space>
            </Form>
          </Card>}
          <Table
            size="small"
            rowKey="model"
            pagination={false}
            scroll={{ x: 1050 }}
            locale={{ emptyText: t("usage.noPricing") }}
            dataSource={pricing}
            loading={metaQuery.isPending && !metaQuery.data}
            columns={[
              { title: t("usage.model"), dataIndex: "model" },
              { title: t("usage.pricingProvider"), dataIndex: "provider", render: (v: string) => v || "-" },
              { title: t("usage.inputPrice"), dataIndex: "inputPricePerMillion", render: (v: number, row) => formatCost(v, row.currency) },
              { title: t("usage.cacheReadPrice"), dataIndex: "cacheReadPricePerMillion", render: (v: number, row) => formatCost(v, row.currency) },
              { title: t("usage.cacheWritePrice"), dataIndex: "cacheWritePricePerMillion", render: (v: number, row) => formatCost(v, row.currency) },
              { title: t("usage.outputPrice"), dataIndex: "outputPricePerMillion", render: (v: number, row) => formatCost(v, row.currency) },
              { title: t("usage.batchInputPrice"), dataIndex: "batchInputPricePerMillion", render: (v: number, row) => formatCost(v, row.currency) },
              { title: t("usage.batchOutputPrice"), dataIndex: "batchOutputPricePerMillion", render: (v: number, row) => formatCost(v, row.currency) },
              { title: t("usage.currency"), dataIndex: "currency", render: (v: string) => <Tag>{v}</Tag> },
              { title: t("usage.priceSource"), dataIndex: "effectiveDate", render: (v: string, row: ModelPricing) => row.sourceUrl ? <a href={row.sourceUrl} target="_blank" rel="noreferrer">{v || t("usage.priceSource")}</a> : "-" },
              { title: t("usage.actions"), render: (_, row: ModelPricing) => <Button danger type="link" onClick={() => void removePricing(row.model)}>{t("usage.delete")}</Button> },
            ]}
          />
        </Space>
      </Modal>

      <Modal
        title={t("usage.importPricing")}
        open={pricingImportOpen}
        okText={t("usage.confirmImportPricing")}
        okButtonProps={{ disabled: !pricingImportPreview || pricingImportPreview.errors.length > 0 }}
        confirmLoading={saving}
        onOk={() => void applyPricingImport()}
        onCancel={() => { setPricingImportOpen(false); setPricingImportPath(null); setPricingImportPreview(null); }}
      >
        <Space direction="vertical" style={{ width: "100%" }}>
          <Text type="secondary" style={{ wordBreak: "break-all" }}>{pricingImportPath}</Text>
          {pricingImportPreview && <>
            <Descriptions size="small" column={1} bordered>
              <Descriptions.Item label={t("usage.importValidRows")}>{pricingImportPreview.validRows}</Descriptions.Item>
              <Descriptions.Item label={t("usage.importNewModels")}>{pricingImportPreview.newModels.join(", ") || "—"}</Descriptions.Item>
              <Descriptions.Item label={t("usage.importUpdatedModels")}>{pricingImportPreview.updatedModels.join(", ") || "—"}</Descriptions.Item>
            </Descriptions>
            {pricingImportPreview.errors.length > 0 && <Alert type="error" showIcon message={t("usage.importPricingErrors")} description={pricingImportPreview.errors.join("\n")} />}
          </>}
        </Space>
      </Modal>

      <Modal
        title={t("usage.maintainLogs")}
        open={maintenanceOpen}
        confirmLoading={maintaining}
        okText={t("usage.confirmMaintenance")}
        onOk={() => void maintainLogs()}
        onCancel={() => setMaintenanceOpen(false)}
      >
        <Space direction="vertical" style={{ width: "100%" }}>
          <Alert
            type="warning"
            showIcon
            message={t("usage.maintenancePreview", {
              total: maintenancePreview?.totalRows ?? 0,
              byAge: maintenancePreview?.deleteByAge ?? 0,
              byLimit: maintenancePreview?.deleteByLimit ?? 0,
            })}
          />
          <Form layout="vertical">
            <Form.Item label={t("usage.logRetentionDays")}>
              <InputNumber min={1} max={3650} style={{ width: "100%" }} value={maintenancePolicy?.retentionDays} onChange={(value) => setMaintenancePolicy((current) => current && { ...current, retentionDays: Number(value ?? 90) })} />
            </Form.Item>
            <Form.Item label={t("usage.logMaxRows")}>
              <InputNumber min={100} max={5000000} style={{ width: "100%" }} value={maintenancePolicy?.maxRows} onChange={(value) => setMaintenancePolicy((current) => current && { ...current, maxRows: Number(value ?? 100000) })} />
            </Form.Item>
            <Form.Item label={t("usage.autoMaintain")}>
              <Switch checked={maintenancePolicy?.autoMaintain ?? false} onChange={(checked) => setMaintenancePolicy((current) => current && { ...current, autoMaintain: checked })} />
            </Form.Item>
          </Form>
        </Space>
      </Modal>

      <Drawer
        title={t("usage.logDetail")}
        open={detailDiagnostic !== null}
        onClose={() => setDetailDiagnostic(null)}
        width={560}
      >
        <pre style={{ whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{detailDiagnostic}</pre>
      </Drawer>

      <Modal
        title={t("usage.trendChart")}
        open={trendExpanded}
        onCancel={() => setTrendExpanded(false)}
        footer={null}
        width="96vw"
        styles={{ body: { minHeight: "70vh" } }}
      >
        <UsageTrendChart data={dashboard?.trend ?? []} period={period} granularity={dashboard?.trendGranularity ?? usagePeriodGranularity(period)} t={t} expanded />
      </Modal>
    </>
  );
}

function UsageTrendChart({
  data,
  period,
  granularity,
  t,
  expanded = false,
}: {
  data: UsageDashboard["trend"];
  period: UsagePeriod;
  granularity: "hour" | "day";
  t: (key: string) => string;
  expanded?: boolean;
}) {
  const { token } = theme.useToken();
  const chartData = useMemo(() => {
    const byDate = new Map(data.map((row) => [row.date, row]));
    const keys =
      granularity === "hour" && (period === "24h" || period === "today")
        ? usagePeriodHourKeys(period)
        : data.map((row) => row.date);
    return keys.map((key) => {
      const row = byDate.get(key);
      return {
        date: key,
        label: trendBucketLabel(key, granularity),
        inputTokens: row?.inputTokens ?? 0,
        outputTokens: row?.outputTokens ?? 0,
        cacheCreationInputTokens: row?.cacheCreationInputTokens ?? 0,
        cacheReadInputTokens: row?.cacheReadInputTokens ?? 0,
        estimatedCost: row?.estimatedCost ?? 0,
        currency: row?.currency ?? "USD",
      };
    });
  }, [data, granularity, period]);
  const chartCurrency = useMemo(() => {
    const currencies = [...new Set(chartData.map((row) => row.currency).filter((value) => value && value !== "MIXED"))];
    return currencies.length === 1 ? currencies[0] : "USD";
  }, [chartData]);
  const colors = {
    input: token.colorInfo,
    output: token.colorSuccess,
    cacheWrite: token.colorWarning,
    cacheRead: token.colorPrimary,
    cost: token.colorError,
    grid: token.colorBorderSecondary,
    axis: token.colorTextSecondary,
    tooltipBg: token.colorBgElevated,
    tooltipBorder: token.colorBorderSecondary,
  };
  const height = expanded ? 520 : 350;

  if (!chartData.length) return <Empty description={t("usage.noData")} />;

  return (
    <div style={{ width: "100%", height }} role="img" aria-label={t("usage.trendChart")}>
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={chartData} margin={{ top: 12, right: 16, left: 0, bottom: 4 }}>
          <defs>
            <linearGradient id="usageColorInput" x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor={colors.input} stopOpacity={0.28} />
              <stop offset="95%" stopColor={colors.input} stopOpacity={0} />
            </linearGradient>
            <linearGradient id="usageColorOutput" x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor={colors.output} stopOpacity={0.28} />
              <stop offset="95%" stopColor={colors.output} stopOpacity={0} />
            </linearGradient>
            <linearGradient id="usageColorCacheWrite" x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor={colors.cacheWrite} stopOpacity={0.22} />
              <stop offset="95%" stopColor={colors.cacheWrite} stopOpacity={0} />
            </linearGradient>
            <linearGradient id="usageColorCacheRead" x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor={colors.cacheRead} stopOpacity={0.22} />
              <stop offset="95%" stopColor={colors.cacheRead} stopOpacity={0} />
            </linearGradient>
          </defs>
          <CartesianGrid strokeDasharray="3 3" vertical={false} stroke={colors.grid} />
          <XAxis
            dataKey="label"
            axisLine={false}
            tickLine={false}
            tick={{ fill: colors.axis, fontSize: 12 }}
            dy={8}
            interval={granularity === "hour" ? 1 : "preserveStartEnd"}
            minTickGap={granularity === "hour" ? 8 : 20}
          />
          <YAxis
            yAxisId="tokens"
            axisLine={false}
            tickLine={false}
            tick={{ fill: colors.axis, fontSize: 12 }}
            tickFormatter={(value: number) => formatCompactNumber(value)}
            width={48}
          />
          <YAxis
            yAxisId="cost"
            orientation="right"
            axisLine={false}
            tickLine={false}
            tick={{ fill: colors.axis, fontSize: 12 }}
            tickFormatter={(value: number) => `${currencyPrefix(chartCurrency)}${Number(value).toFixed(2)}`}
            width={56}
          />
          <RechartsTooltip
            contentStyle={{
              background: colors.tooltipBg,
              border: `1px solid ${colors.tooltipBorder}`,
              borderRadius: 8,
            }}
            labelFormatter={(_label, payload) => {
              const row = payload?.[0]?.payload as { date?: string } | undefined;
              return row?.date ?? String(_label);
            }}
            formatter={(value, name, item) => {
              const numeric = typeof value === "number" ? value : Number(value);
              if (name === t("usage.estimatedCost")) {
                const row = item?.payload as { currency?: string } | undefined;
                return [formatCost(numeric, row?.currency ?? chartCurrency), name];
              }
              return [formatCompactNumber(numeric), name];
            }}
          />
          <Legend />
          <Area
            yAxisId="tokens"
            type="monotone"
            dataKey="inputTokens"
            name={t("usage.inputTokens")}
            stroke={colors.input}
            fill="url(#usageColorInput)"
            strokeWidth={2}
          />
          <Area
            yAxisId="tokens"
            type="monotone"
            dataKey="outputTokens"
            name={t("usage.outputTokens")}
            stroke={colors.output}
            fill="url(#usageColorOutput)"
            strokeWidth={2}
          />
          <Area
            yAxisId="tokens"
            type="monotone"
            dataKey="cacheCreationInputTokens"
            name={t("usage.cacheWriteTokens")}
            stroke={colors.cacheWrite}
            fill="url(#usageColorCacheWrite)"
            strokeWidth={2}
          />
          <Area
            yAxisId="tokens"
            type="monotone"
            dataKey="cacheReadInputTokens"
            name={t("usage.cacheReadTokens")}
            stroke={colors.cacheRead}
            fill="url(#usageColorCacheRead)"
            strokeWidth={2}
          />
          <Area
            yAxisId="cost"
            type="monotone"
            dataKey="estimatedCost"
            name={t("usage.estimatedCost")}
            stroke={colors.cost}
            fill="none"
            strokeWidth={2}
            strokeDasharray="4 4"
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  );
}

function Metric({ title, value, suffix, prefix, precision, icon, compact }: { title: string; value: number; suffix?: string; prefix?: string; precision?: number; icon?: ReactNode; compact?: boolean }) {
  return (
    <Col xs={24} sm={12} xl={6}>
      <Card size="small">
        <Statistic
          title={title}
          value={value}
          suffix={suffix}
          prefix={prefix ?? icon}
          precision={precision}
          formatter={compact ? (v) => formatCompactNumber(Number(v)) : undefined}
        />
      </Card>
    </Col>
  );
}

function BreakdownCard({ title, data, t }: { title: string; data: UsageDashboard["byModel"]; t: (key: string) => string }) {
  return <Card size="small" title={title}>
    <Table
        size="small"
        pagination={false}
        rowKey="key"
        locale={{ emptyText: t("usage.noData") }}
        dataSource={data}
        columns={[
          { title: t("usage.name"), dataIndex: "key", ellipsis: true },
          { title: t("usage.requests"), dataIndex: "requestCount" },
          {
            title: t("usage.totalTokens"),
            render: (_: unknown, row: UsageDashboard["byModel"][number]) =>
              formatCompactNumber(
                row.inputTokens +
                  row.cacheReadInputTokens +
                  row.cacheCreationInputTokens +
                  row.outputTokens,
              ),
          },
          { title: t("usage.estimatedCost"), dataIndex: "estimatedCost", render: (v: number, row) =>
            row.currency === "MIXED" ? t("usage.mixedCurrency") : formatCost(v, row.currency) },
        ]}
    />
  </Card>;
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
function formatCost(value: number, currency?: string | null) {
  return `${currencyPrefix(currency)}${value.toFixed(4)}`;
}
function errMsg(e: unknown): string { return e instanceof Error ? e.message : String(e); }
