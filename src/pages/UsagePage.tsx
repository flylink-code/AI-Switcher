import { useCallback, useEffect, useState, type ReactNode } from "react";
import {
  Alert,
  Button,
  Card,
  Col,
  Drawer,
  Empty,
  Form,
  Input,
  InputNumber,
  Modal,
  Row,
  Select,
  Space,
  Spin,
  Statistic,
  Switch,
  Table,
  Tag,
  Typography,
  message,
} from "antd";
import DollarOutlined from "@ant-design/icons/es/icons/DollarOutlined";
import LineChartOutlined from "@ant-design/icons/es/icons/LineChartOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import UnorderedListOutlined from "@ant-design/icons/es/icons/UnorderedListOutlined";
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
  getUsageDashboard,
  getLogMaintenancePolicy,
  listModelPricing,
  listProxyRequestLogs,
  maintainProxyLogs,
  previewProxyLogMaintenance,
  saveModelPricing,
  saveLogMaintenancePolicy,
} from "@/services/api";

const { Text } = Typography;

export default function UsagePage() {
  const { t } = useTranslation();
  const [days, setDays] = useState(30);
  const [dashboard, setDashboard] = useState<UsageDashboard | null>(null);
  const [pricing, setPricing] = useState<ModelPricing[]>([]);
  const [loading, setLoading] = useState(true);
  const [pricingOpen, setPricingOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [maintaining, setMaintaining] = useState(false);
  const [maintenanceOpen, setMaintenanceOpen] = useState(false);
  const [maintenancePolicy, setMaintenancePolicy] = useState<LogMaintenancePolicy | null>(null);
  const [maintenancePreview, setMaintenancePreview] = useState<LogMaintenancePreview | null>(null);
  const [form] = Form.useForm<ModelPricingInput>();
  const [requestLogs, setRequestLogs] = useState<PaginatedProxyLogs | null>(null);
  const [logPage, setLogPage] = useState(0);
  const [logTargetApp, setLogTargetApp] = useState<ProviderTarget | "all">("all");
  const [detailDiagnostic, setDetailDiagnostic] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [nextDashboard, nextPricing, nextPolicy, nextLogs] = await Promise.all([
        getUsageDashboard(days),
        listModelPricing(),
        getLogMaintenancePolicy(),
        listProxyRequestLogs({
          days,
          page: logPage,
          pageSize: 20,
          targetApp: logTargetApp === "all" ? undefined : logTargetApp,
        }),
      ]);
      setDashboard(nextDashboard);
      setPricing(nextPricing);
      setMaintenancePolicy(nextPolicy);
      setRequestLogs(nextLogs);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setLoading(false);
    }
  }, [days, logPage, logTargetApp]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    setLogPage(0);
  }, [days, logTargetApp]);

  useEffect(() => {
    if (!maintenanceOpen || !maintenancePolicy) return;
    void previewProxyLogMaintenance(maintenancePolicy)
      .then(setMaintenancePreview)
      .catch((e) => void message.error(errMsg(e)));
  }, [maintenanceOpen, maintenancePolicy]);

  const savePricing = async () => {
    try {
      const values = await form.validateFields();
      setSaving(true);
      await saveModelPricing(values);
      setPricingOpen(false);
      form.resetFields();
      void message.success(t("usage.pricingSaved"));
      await refresh();
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
      await refresh();
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
      await refresh();
    } catch (e) { void message.error(errMsg(e)); }
    finally { setMaintaining(false); }
  };

  const summary = dashboard?.summary;
  const totalTokens =
    (summary?.inputTokens ?? 0) +
    (summary?.cacheReadInputTokens ?? 0) +
    (summary?.cacheCreationInputTokens ?? 0) +
    (summary?.outputTokens ?? 0);

  return (
    <Spin spinning={loading}>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <Alert type="info" showIcon message={t("usage.title")} description={t("usage.description")} />

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
              onChange={setDays}
            />
          </Space>
          <Button icon={<ReloadOutlined />} onClick={() => void refresh()}>{t("common.refresh")}</Button>
          <Button loading={maintaining} onClick={() => void openMaintenance()}>{t("usage.maintainLogs")}</Button>
        </Space>

        <Row gutter={[16, 16]}>
          <Metric title={t("usage.requests")} value={summary?.requestCount ?? 0} icon={<ThunderboltOutlined />} />
          <Metric title={t("usage.successRate")} value={successRate(summary?.requestCount, summary?.successfulRequestCount)} suffix="%" />
          <Metric title={t("usage.totalTokens")} value={totalTokens} />
          <Metric title={t("usage.estimatedCost")} value={summary?.estimatedCost ?? 0} precision={4} prefix="$" icon={<DollarOutlined />} />
        </Row>

        <Card size="small" title={<Space><LineChartOutlined />{t("usage.trend")}</Space>}>
          {dashboard?.trend.length ? (
            <Table
              size="small"
              pagination={false}
              rowKey="date"
              dataSource={dashboard.trend}
              columns={[
                { title: t("usage.date"), dataIndex: "date" },
                { title: t("usage.requests"), dataIndex: "requestCount" },
                {
                  title: t("usage.inputTokens"),
                  render: (_: unknown, row: UsageDashboard["trend"][number]) =>
                    formatNumber(
                      row.inputTokens +
                        row.cacheReadInputTokens +
                        row.cacheCreationInputTokens,
                    ),
                },
                { title: t("usage.cacheTokens"), dataIndex: "cacheReadInputTokens", render: formatNumber },
                { title: t("usage.outputTokens"), dataIndex: "outputTokens", render: formatNumber },
                { title: t("usage.estimatedCost"), dataIndex: "estimatedCost", render: (v: number) => formatCost(v) },
              ]}
            />
          ) : <Empty description={t("usage.noData")} />}
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
          extra={
            <Select
              size="small"
              value={logTargetApp}
              style={{ width: 160 }}
              onChange={(value: ProviderTarget | "all") => setLogTargetApp(value)}
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
                  `${formatNumber(row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens)} / ${formatNumber(row.outputTokens)}${row.cacheReadInputTokens ? ` (${t("usage.cached")}: ${formatNumber(row.cacheReadInputTokens)})` : ""}`,
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
            locale={{ emptyText: t("usage.noPricing") }}
            dataSource={pricing}
            columns={[
              { title: t("usage.model"), dataIndex: "model" },
              { title: t("usage.inputPrice"), dataIndex: "inputPricePerMillion", render: (v: number) => formatCost(v) },
              { title: t("usage.outputPrice"), dataIndex: "outputPricePerMillion", render: (v: number) => formatCost(v) },
              { title: t("usage.currency"), dataIndex: "currency", render: (v: string) => <Tag>{v}</Tag> },
              { title: t("usage.actions"), render: (_, row: ModelPricing) => <Button danger type="link" onClick={() => void removePricing(row.model)}>{t("usage.delete")}</Button> },
            ]}
          />
        </Card>
      </Space>

      <Modal title={t("usage.addPricing")} open={pricingOpen} confirmLoading={saving} onOk={() => void savePricing()} onCancel={() => { setPricingOpen(false); form.resetFields(); }}>
        <Form form={form} layout="vertical" initialValues={{ currency: "USD", inputPricePerMillion: 0, outputPricePerMillion: 0 }}>
          <Form.Item name="model" label={t("usage.model")} rules={[{ required: true, message: t("usage.requiredModel") }]}>
            <Input placeholder="claude-sonnet-4" />
          </Form.Item>
          <Form.Item name="inputPricePerMillion" label={t("usage.inputPrice")} rules={[{ required: true }]}>
            <InputNumber min={0} precision={6} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item name="outputPricePerMillion" label={t("usage.outputPrice")} rules={[{ required: true }]}>
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
    </Spin>
  );
}

function Metric({ title, value, suffix, prefix, precision, icon }: { title: string; value: number; suffix?: string; prefix?: string; precision?: number; icon?: ReactNode }) {
  return <Col xs={24} sm={12} xl={6}><Card size="small"><Statistic title={title} value={value} suffix={suffix} prefix={prefix ?? icon} precision={precision} /></Card></Col>;
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
  </Card>;
}

function successRate(total?: number, successful?: number) {
  return total ? Number((((successful ?? 0) / total) * 100).toFixed(1)) : 0;
}
function formatNumber(value: number) { return new Intl.NumberFormat().format(value); }
function formatCost(value: number) { return `$${value.toFixed(4)}`; }
function errMsg(e: unknown): string { return e instanceof Error ? e.message : String(e); }
