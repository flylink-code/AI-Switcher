import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  Alert,
  Button,
  Card,
  Col,
  Divider,
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
  Tooltip,
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
import type {
  LogMaintenancePolicy,
  LogMaintenancePreview,
  ModelPricing,
  ModelPricingInput,
  PaginatedProxyLogs,
  ProviderTarget,
  UsageDashboard,
} from "@/types/backend";
import {
  deleteModelPricing,
  maintainProxyLogs,
  previewProxyLogMaintenance,
  saveModelPricing,
  saveLogMaintenancePolicy,
} from "@/services/api";
import { usageOverviewOptions } from "@/lib/appQueries";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";

const { Text } = Typography;

export default function UsagePage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const days = usePagePreferencesStore((state) => state.usageDays);
  const setDays = usePagePreferencesStore((state) => state.setUsageDays);
  const logPage = usePagePreferencesStore((state) => state.usageLogPage);
  const setLogPage = usePagePreferencesStore((state) => state.setUsageLogPage);
  const logTargetApp = usePagePreferencesStore((state) => state.usageLogTarget);
  const setLogTargetApp = usePagePreferencesStore((state) => state.setUsageLogTarget);
  const [pricingOpen, setPricingOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [maintaining, setMaintaining] = useState(false);
  const [maintenanceOpen, setMaintenanceOpen] = useState(false);
  const [maintenancePolicy, setMaintenancePolicy] = useState<LogMaintenancePolicy | null>(null);
  const [maintenancePreview, setMaintenancePreview] = useState<LogMaintenancePreview | null>(null);
  const [form] = Form.useForm<ModelPricingInput>();
  const [detailDiagnostic, setDetailDiagnostic] = useState<string | null>(null);
  const [trendExpanded, setTrendExpanded] = useState(false);
  const overviewQuery = useQuery({
    ...usageOverviewOptions(days, logPage, logTargetApp),
    placeholderData: keepPreviousData,
  });
  const dashboard = overviewQuery.data?.dashboard ?? null;
  const pricing = overviewQuery.data?.pricing ?? [];
  const requestLogs = overviewQuery.data?.requestLogs ?? null;

  useEffect(() => {
    if (!maintenanceOpen && overviewQuery.data?.maintenancePolicy) {
      setMaintenancePolicy(overviewQuery.data.maintenancePolicy);
    }
  }, [maintenanceOpen, overviewQuery.data?.maintenancePolicy]);

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
        void overviewQuery.refetch();
      }
    }, 30_000);
    return () => window.clearInterval(interval);
  }, [maintaining, overviewQuery.refetch, saving]);

  const savePricing = async () => {
    try {
      const values = await form.validateFields();
      setSaving(true);
      await saveModelPricing(values);
      setPricingOpen(false);
      form.resetFields();
      void message.success(t("usage.pricingSaved"));
      await queryClient.invalidateQueries({ queryKey: ["usage-overview"] });
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
      await queryClient.invalidateQueries({ queryKey: ["usage-overview"] });
    } catch (e) {
      void message.error(errMsg(e));
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
      await queryClient.invalidateQueries({ queryKey: ["usage-overview"] });
    } catch (e) { void message.error(errMsg(e)); }
    finally { setMaintaining(false); }
  };

  const refreshOverview = async () => {
    setRefreshing(true);
    try {
      await overviewQuery.refetch();
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

  return (
    <>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        {overviewQuery.error && <Alert type="error" showIcon message={errMsg(overviewQuery.error)} />}
        <Alert type="info" showIcon message={t("usage.title")} description={t("usage.description")} />
        <Alert type="warning" showIcon message={t("usage.currencyLimit")} />
        <Alert type="info" showIcon message={t("usage.cachePricingIncluded")} />

        <Space wrap style={{ justifyContent: "space-between", width: "100%" }}>
          <Space>
            <Text>{t("usage.period")}</Text>
            <Select
              value={days}
              style={{ width: 130 }}
              options={[7, 30, 90, 365].map((value) => ({
                value,
                label: t("usage.lastDays", { days: value }),
              }))}
              onChange={(value) => {
                setDays(value);
                setLogPage(0);
              }}
            />
          </Space>
          <Button
            icon={<ReloadOutlined />}
            loading={refreshing}
            onClick={() => void refreshOverview()}
          >
            {t("common.refresh")}
          </Button>
          <Button loading={maintaining} onClick={() => void openMaintenance()}>{t("usage.maintainLogs")}</Button>
        </Space>

        <Row gutter={[16, 16]}>
          <Metric title={t("usage.requests")} value={summary?.requestCount ?? 0} icon={<ThunderboltOutlined />} />
          <Metric title={t("usage.successRate")} value={successRate(summary?.requestCount, summary?.successfulRequestCount)} suffix="%" />
          <Metric title={t("usage.totalTokens")} value={totalTokens} />
          <Metric title={t("usage.estimatedCost")} value={summary?.estimatedCost ?? 0} precision={4} prefix="$" icon={<DollarOutlined />} />
        </Row>

        <Card
          size="small"
          title={<Space><LineChartOutlined />{t("usage.trendChart")}</Space>}
          extra={<Button size="small" icon={<ExpandOutlined />} onClick={() => setTrendExpanded(true)}>{t("usage.expandChart")}</Button>}
        >
          <UsageTrendChart data={dashboard?.trend ?? []} t={t} />
        </Card>

        <Row gutter={[16, 16]}>
          <Col xs={24} lg={12}>
            <BreakdownCard title={t("usage.byProvider")} data={dashboard?.byProvider ?? []} t={t}>
              <Divider style={{ margin: "4px 0 0" }}>{t("usage.dailyStatistics")}</Divider>
              <UsageCalendar data={dashboard?.trend ?? []} days={days} t={t} />
            </BreakdownCard>
          </Col>
          <Col xs={24} lg={12}>
            <BreakdownCard title={t("usage.byModel")} data={dashboard?.byModel ?? []} t={t} />
          </Col>
        </Row>

        <Card
          size="small"
          title={<Space><UnorderedListOutlined />{t("usage.requestLogs")}</Space>}
          extra={
            <Select
              size="small"
              value={logTargetApp}
              style={{ width: 160 }}
              onChange={(value: ProviderTarget | "all") => {
                setLogTargetApp(value);
                setLogPage(0);
              }}
              options={[
                { value: "all", label: t("usage.allApps") },
                { value: "claude_code", label: t("providers.claudeCode") },
                { value: "claude_desktop", label: t("providers.claudeDesktop") },
              ]}
            />
          }
        >
          <Table
            size="small"
            rowKey="id"
            locale={{ emptyText: t("usage.noData") }}
            dataSource={requestLogs?.data ?? []}
            loading={overviewQuery.isPending}
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
                render: (v: string | null) => v ?? "—",
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
                    ? `${formatNumber(row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens)} / ${formatNumber(row.outputTokens)}${row.cacheReadInputTokens ? ` (${t("usage.cached")}: ${formatNumber(row.cacheReadInputTokens)})` : ""}`
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

        <Card
          size="small"
          title={t("usage.pricing")}
          extra={<Button type="primary" icon={<PlusOutlined />} onClick={() => setPricingOpen(true)}>{t("usage.addPricing")}</Button>}
        >
          <Table
            size="small"
            rowKey="model"
            pagination={false}
            scroll={{ x: 1300 }}
            locale={{ emptyText: t("usage.noPricing") }}
            dataSource={pricing}
            loading={overviewQuery.isPending}
            columns={[
              { title: t("usage.model"), dataIndex: "model" },
              { title: t("usage.pricingProvider"), dataIndex: "provider", render: (v: string) => v || "-" },
              { title: t("usage.inputPrice"), dataIndex: "inputPricePerMillion", render: (v: number) => formatCost(v) },
              { title: t("usage.cacheReadPrice"), dataIndex: "cacheReadPricePerMillion", render: (v: number) => formatCost(v) },
              { title: t("usage.cacheWritePrice"), dataIndex: "cacheWritePricePerMillion", render: (v: number) => formatCost(v) },
              { title: t("usage.outputPrice"), dataIndex: "outputPricePerMillion", render: (v: number) => formatCost(v) },
              { title: t("usage.batchInputPrice"), dataIndex: "batchInputPricePerMillion", render: (v: number) => formatCost(v) },
              { title: t("usage.batchOutputPrice"), dataIndex: "batchOutputPricePerMillion", render: (v: number) => formatCost(v) },
              { title: t("usage.currency"), dataIndex: "currency", render: (v: string) => <Tag>{v}</Tag> },
              { title: t("usage.priceSource"), dataIndex: "effectiveDate", render: (v: string, row: ModelPricing) => row.sourceUrl ? <a href={row.sourceUrl} target="_blank" rel="noreferrer">{v || t("usage.priceSource")}</a> : "-" },
              { title: t("usage.actions"), render: (_, row: ModelPricing) => <Button danger type="link" onClick={() => void removePricing(row.model)}>{t("usage.delete")}</Button> },
            ]}
          />
        </Card>
      </Space>

      <Modal title={t("usage.addPricing")} open={pricingOpen} confirmLoading={saving} onOk={() => void savePricing()} onCancel={() => { setPricingOpen(false); form.resetFields(); }}>
        <Form form={form} layout="vertical" initialValues={{ currency: "USD", inputPricePerMillion: 0, cacheReadPricePerMillion: 0, cacheWritePricePerMillion: 0, outputPricePerMillion: 0, batchInputPricePerMillion: 0, batchOutputPricePerMillion: 0 }}>
          <Form.Item name="model" label={t("usage.model")} rules={[{ required: true, message: t("usage.requiredModel") }]}>
            <Input placeholder="claude-sonnet-4" />
          </Form.Item>
          <Form.Item name="provider" label={t("usage.pricingProvider")}>
            <Input placeholder="Anthropic" />
          </Form.Item>
          <Form.Item name="inputPricePerMillion" label={t("usage.inputPrice")} rules={[{ required: true }]}>
            <InputNumber min={0} precision={6} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item name="cacheReadPricePerMillion" label={t("usage.cacheReadPrice")}>
            <InputNumber min={0} precision={6} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item name="cacheWritePricePerMillion" label={t("usage.cacheWritePrice")}>
            <InputNumber min={0} precision={6} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item name="outputPricePerMillion" label={t("usage.outputPrice")} rules={[{ required: true }]}>
            <InputNumber min={0} precision={6} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item name="batchInputPricePerMillion" label={t("usage.batchInputPrice")}>
            <InputNumber min={0} precision={6} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item name="batchOutputPricePerMillion" label={t("usage.batchOutputPrice")}>
            <InputNumber min={0} precision={6} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item name="currency" label={t("usage.currency")} rules={[{ required: true }]}>
            <Input maxLength={12} />
          </Form.Item>
        </Form>
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
        <UsageTrendChart data={dashboard?.trend ?? []} t={t} expanded />
      </Modal>
    </>
  );
}

function UsageTrendChart({ data, t, expanded = false }: { data: UsageDashboard["trend"]; t: (key: string) => string; expanded?: boolean }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [containerWidth, setContainerWidth] = useState(720);
  useEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    const updateSize = () => setContainerWidth(Math.max(element.clientWidth, 520));
    updateSize();
    const observer = new ResizeObserver(updateSize);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const values = useMemo(() => data.map((row) => ({
    date: row.date,
    inputTokens: row.inputTokens,
    outputTokens: row.outputTokens,
    cacheCreationInputTokens: row.cacheCreationInputTokens,
    cacheReadInputTokens: row.cacheReadInputTokens,
    estimatedCost: row.estimatedCost,
  })), [data]);
  const series: Array<{
    key: "inputTokens" | "outputTokens" | "cacheCreationInputTokens" | "cacheReadInputTokens" | "estimatedCost";
    label: string;
    color: string;
    cost?: boolean;
  }> = [
    { key: "inputTokens", label: t("usage.inputTokens"), color: "#34cfa0" },
    { key: "outputTokens", label: t("usage.outputTokens"), color: "#5b9cf6" },
    { key: "cacheCreationInputTokens", label: t("usage.cacheWriteTokens"), color: "#f5bd23" },
    { key: "cacheReadInputTokens", label: t("usage.cacheReadTokens"), color: "#38bdf8" },
    { key: "estimatedCost", label: t("usage.estimatedCost"), color: "#f97316", cost: true },
  ];
  const width = containerWidth;
  const height = expanded
    ? Math.min(720, Math.max(480, containerWidth * 0.62))
    : Math.min(500, Math.max(330, containerWidth * 0.42));
  const padding = { top: 26, right: 68, bottom: 52, left: 64 };
  const plotWidth = width - padding.left - padding.right;
  const plotHeight = height - padding.top - padding.bottom;
  const tokenScaleMax = Math.max(
    ...values.flatMap((item) => series.filter((item) => !item.cost).map((metric) => item[metric.key])),
    0,
  ) * 1.15 || 1;
  const costScaleMax = Math.max(...values.map((item) => item.estimatedCost), 0) * 1.15 || 1;
  const pointAt = (value: number, index: number, scaleMax: number) => ({
    x: values.length === 1 ? padding.left + plotWidth / 2 : padding.left + (plotWidth * index) / (values.length - 1),
    y: padding.top + plotHeight * (1 - value / scaleMax),
  });
  const labelIndexes = values.length <= 5
    ? values.map((_, index) => index)
    : [...new Set([0, Math.round((values.length - 1) / 4), Math.round((values.length - 1) / 2), Math.round((values.length - 1) * 3 / 4), values.length - 1])];

  if (!values.length) return <Empty description={t("usage.noData")} />;

  return (
    <Space direction="vertical" size="small" style={{ width: "100%" }}>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }} aria-label={t("usage.trendChart")}>
        {series.map((metric) => (
          <span
            key={metric.key}
            style={{ display: "inline-flex", alignItems: "center", gap: 7, padding: "5px 8px", border: "1px solid var(--ant-color-border-secondary)", borderRadius: 6 }}
          >
            <span style={{ width: 16, height: 4, borderRadius: 3, background: metric.color }} />
            <Text style={{ fontSize: 12 }}>{metric.label}</Text>
            <Text type="secondary" style={{ fontSize: 11 }}>{metric.cost ? "USD" : "Token"}</Text>
          </span>
        ))}
      </div>
      <div ref={containerRef} style={{ width: "100%", minWidth: 0 }}>
        <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label={t("usage.trendChart")} style={{ display: "block", width: "100%", height }} preserveAspectRatio="xMidYMid meet">
          {[0, 0.25, 0.5, 0.75, 1].map((ratio) => {
            const y = padding.top + plotHeight * ratio;
            const tokenValue = tokenScaleMax * (1 - ratio);
            const costValue = costScaleMax * (1 - ratio);
            return <g key={ratio}>
              <line x1={padding.left} x2={width - padding.right} y1={y} y2={y} stroke="currentColor" strokeOpacity="0.12" />
              <text x={padding.left - 12} y={y + 4} textAnchor="end" fontSize="12" fill="currentColor" opacity="0.62">{formatNumber(Math.round(tokenValue))}</text>
              <text x={width - padding.right + 12} y={y + 4} fontSize="12" fill="currentColor" opacity="0.62">{formatCost(costValue)}</text>
            </g>;
          })}
          {series.map((metric) => {
            const scaleMax = metric.cost ? costScaleMax : tokenScaleMax;
            const points = values.map((item, index) => pointAt(item[metric.key], index, scaleMax));
            const linePath = points.map((point, index) => `${index ? "L" : "M"}${point.x.toFixed(2)},${point.y.toFixed(2)}`).join(" ");
            return <g key={metric.key}>
              <path d={linePath} fill="none" stroke={metric.color} strokeWidth={metric.cost ? 3 : 2.25} strokeLinejoin="round" strokeLinecap="round" />
              {values.map((item, index) => {
                const point = points[index];
                const value = item[metric.key];
                return <circle key={`${metric.key}-${item.date}`} cx={point.x} cy={point.y} r={metric.cost ? 3.5 : 2.75} fill="var(--ant-color-bg-container)" stroke={metric.color} strokeWidth="2"><title>{`${item.date}: ${metric.label} ${metric.cost ? formatCost(value) : formatNumber(value)}`}</title></circle>;
              })}
            </g>;
          })}
          {labelIndexes.map((index) => {
            const x = pointAt(0, index, 1).x;
            return <text key={values[index].date} x={x} y={height - 20} textAnchor="middle" fontSize="12" fill="currentColor" opacity="0.62">{values[index].date.slice(5)}</text>;
          })}
        </svg>
      </div>
    </Space>
  );
}

function UsageCalendar({ data, days, t }: { data: UsageDashboard["trend"]; days: number; t: (key: string) => string }) {
  const { token } = theme.useToken();
  const byDate = new Map(data.map((row) => [row.date, row]));
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  start.setDate(start.getDate() - days + 1);
  const daily = Array.from({ length: days }, (_, index) => {
    const date = new Date(start);
    date.setDate(start.getDate() + index);
    const key = localDateKey(date);
    const row = byDate.get(key);
    const tokens = row ? row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens + row.outputTokens : 0;
    return { date, key, row, tokens };
  });
  const max = Math.max(...daily.map((item) => item.tokens), 0);
  const activeDays = daily.filter((item) => item.tokens > 0).length;
  const total = daily.reduce((sum, item) => sum + item.tokens, 0);
  const leading = Array.from({ length: daily[0]?.date.getDay() ?? 0 });

  if (!data.length) return <Empty description={t("usage.noData")} />;

  return (
    <Space direction="vertical" size={14} style={{ width: "100%" }}>
      <Space wrap size={24}>
        <Statistic title={t("usage.activeDays")} value={activeDays} suffix={`/ ${days}`} />
        <Statistic title={t("usage.dailyPeak")} value={max} formatter={(value) => formatNumber(Number(value))} />
        <Statistic title={t("usage.calendarTotal")} value={total} formatter={(value) => formatNumber(Number(value))} />
      </Space>
      <div style={{ overflowX: "auto", paddingBottom: 2 }}>
        <div
          style={{
            display: "grid",
            gridTemplateRows: "repeat(7, 22px)",
            gridAutoFlow: "column",
            gridAutoColumns: "22px",
            gap: 6,
            width: "max-content",
          }}
        >
          {leading.map((_, index) => <span key={`leading-${index}`} />)}
          {daily.map((item) => {
            const level = item.tokens === 0 ? 0 : Math.min(4, Math.ceil((item.tokens / Math.max(max, 1)) * 4));
            const colors = [token.colorFillQuaternary, "#9be9a8", "#40c463", "#30a14e", "#216e39"];
            const tooltip = item.row
              ? `${item.key}: ${formatNumber(item.tokens)} Token · ${item.row.requestCount} ${t("usage.requests")}`
              : `${item.key}: 0 Token`;
            return (
              <Tooltip key={item.key} title={tooltip}>
                <span
                  aria-label={tooltip}
                  style={{ width: 22, height: 22, borderRadius: 4, background: colors[level], outline: `1px solid ${token.colorBorderSecondary}` }}
                />
              </Tooltip>
            );
          })}
        </div>
      </div>
      <Space size={6} align="center" style={{ alignSelf: "flex-end" }}>
        <Text type="secondary" style={{ fontSize: 12 }}>{t("usage.calendarLess")}</Text>
        {[token.colorFillQuaternary, "#9be9a8", "#40c463", "#30a14e", "#216e39"].map((color, index) => (
          <span key={index} style={{ width: 12, height: 12, borderRadius: 2, background: color, outline: `1px solid ${token.colorBorderSecondary}` }} />
        ))}
        <Text type="secondary" style={{ fontSize: 12 }}>{t("usage.calendarMore")}</Text>
      </Space>
    </Space>
  );
}

function localDateKey(value: Date) {
  const year = value.getFullYear();
  const month = String(value.getMonth() + 1).padStart(2, "0");
  const day = String(value.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function Metric({ title, value, suffix, prefix, precision, icon }: { title: string; value: number; suffix?: string; prefix?: string; precision?: number; icon?: ReactNode }) {
  return <Col xs={24} sm={12} xl={6}><Card size="small"><Statistic title={title} value={value} suffix={suffix} prefix={prefix ?? icon} precision={precision} /></Card></Col>;
}

function BreakdownCard({ title, data, t, children }: { title: string; data: UsageDashboard["byModel"]; t: (key: string) => string; children?: ReactNode }) {
  return <Card size="small" title={title}>
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
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
              formatNumber(
                row.inputTokens +
                  row.cacheReadInputTokens +
                  row.cacheCreationInputTokens +
                  row.outputTokens,
              ),
          },
          { title: t("usage.estimatedCost"), dataIndex: "estimatedCost", render: (v: number) => formatCost(v) },
        ]}
      />
      {children}
    </Space>
  </Card>;
}

function successRate(total?: number, successful?: number) {
  return total ? Number((((successful ?? 0) / total) * 100).toFixed(1)) : 0;
}
function formatNumber(value: number) { return new Intl.NumberFormat().format(value); }
function formatCost(value: number) { return `$${value.toFixed(4)}`; }
function errMsg(e: unknown): string { return e instanceof Error ? e.message : String(e); }
