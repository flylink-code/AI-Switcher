import { useMemo, useState } from "react";
import {
  Button,
  Card,
  Checkbox,
  Drawer,
  Dropdown,
  Form,
  Input,
  Modal,
  AutoComplete,
  Popconfirm,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  Typography,
  message,
} from "antd";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import ExportOutlined from "@ant-design/icons/es/icons/ExportOutlined";
import LoginOutlined from "@ant-design/icons/es/icons/LoginOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import DownOutlined from "@ant-design/icons/es/icons/DownOutlined";
import ScanOutlined from "@ant-design/icons/es/icons/ScanOutlined";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  addAntigravityGatewayUpstream,
  addKiroGatewayUpstream,
  deleteGatewayUpstream,
  discoverGatewayUpstreamModels,
  discoverGatewayUpstreamModelsBatch,
  ensureCodexOauthProvider,
  exportGatewayUpstreams,
  importGatewayUpstreamsFromProviders,
  importGatewayUpstreamsJson,
  importLiveConfigAsUpstreams,
  listGatewayUpstreamHealth,
  listGatewayUpstreamModels,
  listGatewayUpstreams,
  listProviders,
  pollCodexOauthLogin,
  setGatewayUpstreamModelVisible,
  startCodexOauthLogin,
  testUpstreamConnection,
  upsertGatewayUpstream,
} from "@/services/providers";
import { LABEL_KEYS, PROVIDER_TARGET_OPTIONS } from "@/components/AgentTargetSwitcher";
import { filterUiAgents } from "@/lib/agentVisibility";
import { PROVIDER_PRESETS, type ProviderPreset } from "@/lib/providerPresets";
import {
  buildEndpointPreview,
  ensureOpenAiV1Suffix,
  isReservedListenerUrl,
  needsOpenAiV1Suffix,
  normalizeBaseUrl,
} from "@/lib/providerUrl";
import { ProviderQuotaView } from "@/components/ProviderQuotaView";
import type {
  CodexOauthDeviceStart,
  GatewayUpstreamHealth,
  Provider,
  ProviderInput,
  ProviderTarget,
  ProtocolType,
} from "@/types/backend";

const { Text } = Typography;

const PROTOCOL_OPTIONS: { value: ProtocolType; label: string }[] = [
  { value: "anthropic", label: "Anthropic" },
  { value: "openai_chat", label: "OpenAI Chat" },
  { value: "openai_responses", label: "OpenAI Responses" },
];

function healthLabel(
  row: GatewayUpstreamHealth | undefined,
  t: (key: string, opts?: Record<string, unknown>) => string,
) {
  if (!row || row.status === "unknown") {
    return <Tag>{t("proxy.healthUnknown", { defaultValue: "未检测" })}</Tag>;
  }
  if (row.status === "ok") {
    const latency = row.lastLatencyMs != null
      ? t("proxy.healthLatency", { ms: row.lastLatencyMs, defaultValue: "{{ms}}ms" })
      : "";
    return (
      <Space size={4}>
        <Tag color="green">{t("proxy.healthOk", { defaultValue: "正常" })}</Tag>
        {latency ? <Text type="secondary">{latency}</Text> : null}
      </Space>
    );
  }
  if (row.status === "cooling") {
    const secs = Math.max(1, Math.ceil(row.cooldownRemainingMs / 1000));
    return (
      <Space size={4}>
        <Tag color="orange">{t("proxy.healthCooling", { secs, defaultValue: "冷却中 {{secs}}s" })}</Tag>
        <Text type="secondary">×{row.consecutiveFailures}</Text>
      </Space>
    );
  }
  return (
    <Space size={4}>
      <Tag color="red">{t("proxy.healthFailing", { defaultValue: "连续失败" })}</Tag>
      <Text type="secondary">×{row.consecutiveFailures}</Text>
      {row.lastLatencyMs != null ? <Text type="secondary">{row.lastLatencyMs}ms</Text> : null}
    </Space>
  );
}

function canQueryUpstreamQuota(row: Provider): boolean {
  if (!row.apiKeySet) return false;
  switch (row.providerKind) {
    case "antigravity":
    case "kiro":
    case "smart_gateway":
      return false;
    case "standard":
    case "codex_oauth":
      return true;
    default: {
      const _exhaustive: never = row.providerKind;
      return _exhaustive;
    }
  }
}

const UPSTREAM_PRESETS: ProviderPreset[] = PROVIDER_PRESETS.filter(
  (preset) =>
    !isReservedListenerUrl(preset.baseUrl) &&
    !preset.baseUrl.includes(":15830") &&
    !preset.baseUrl.includes(":15831"),
);

export function GatewayUpstreamPanel({
  allowlistTarget = "claude_code",
}: {
  allowlistTarget?: ProviderTarget;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [form] = Form.useForm<ProviderInput>();
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState<Provider | null>(null);
  const [saving, setSaving] = useState(false);
  const [addingAg, setAddingAg] = useState(false);
  const [addingKiro, setAddingKiro] = useState(false);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [refreshing, setRefreshing] = useState(false);
  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(null);

  const [importOpen, setImportOpen] = useState(false);
  const [importTarget, setImportTarget] = useState<ProviderTarget>(allowlistTarget);
  const [importIds, setImportIds] = useState<string[]>([]);
  const [importAllowlist, setImportAllowlist] = useState(true);
  const [importing, setImporting] = useState(false);

  const [modelsUpstream, setModelsUpstream] = useState<Provider | null>(null);
  const [modelsSaving, setModelsSaving] = useState(false);

  // Codex / ChatGPT OAuth state
  const [oauthDevice, setOauthDevice] = useState<CodexOauthDeviceStart | null>(null);
  const [oauthPolling, setOauthPolling] = useState(false);

  // Test connection state
  const [testingId, setTestingId] = useState<string | null>(null);

  // JSON Import state
  const [importJsonOpen, setImportJsonOpen] = useState(false);
  const [importJsonText, setImportJsonText] = useState("");
  const [importJsonLoading, setImportJsonLoading] = useState(false);

  const watchedBaseUrl = Form.useWatch("baseUrl", form);
  const watchedProtocol = Form.useWatch("protocolType", form) ?? "anthropic";
  const endpointPreview = buildEndpointPreview(watchedBaseUrl, watchedProtocol);
  const showAppendV1 =
    (watchedProtocol === "openai_chat" || watchedProtocol === "openai_responses")
    && typeof watchedBaseUrl === "string"
    && needsOpenAiV1Suffix(watchedBaseUrl);

  const upstreamsQuery = useQuery({
    queryKey: ["gateway-upstreams"],
    queryFn: listGatewayUpstreams,
  });
  const healthQuery = useQuery({
    queryKey: ["gateway-upstream-health"],
    queryFn: listGatewayUpstreamHealth,
    refetchInterval: 15_000,
  });
  const importProvidersQuery = useQuery({
    queryKey: ["providers", importTarget],
    queryFn: () => listProviders(importTarget),
    enabled: importOpen,
  });
  const modelsQuery = useQuery({
    queryKey: ["gateway-upstream-models", modelsUpstream?.id],
    queryFn: () => listGatewayUpstreamModels(modelsUpstream!.id),
    enabled: Boolean(modelsUpstream),
  });

  const importableProviders = useMemo(
    () => (importProvidersQuery.data ?? []).filter((item) => item.providerKind !== "smart_gateway"),
    [importProvidersQuery.data],
  );

  const urlOptions = useMemo(() => {
    const seen = new Set<string>();
    const options: Array<{ value: string }> = [];
    for (const url of [
      ...UPSTREAM_PRESETS.map((preset) => preset.baseUrl),
      ...(upstreamsQuery.data ?? []).map((item) => item.baseUrl),
    ]) {
      const value = url.trim();
      if (!value || isReservedListenerUrl(value) || seen.has(value)) continue;
      seen.add(value);
      options.push({ value });
    }
    return options;
  }, [upstreamsQuery.data]);

  const invalidatePool = async () => {
    await queryClient.invalidateQueries({ queryKey: ["gateway-upstreams"] });
    await queryClient.invalidateQueries({ queryKey: ["gateway-upstream-health"] });
    await queryClient.invalidateQueries({ queryKey: ["gateway-catalog-models"] });
    await queryClient.invalidateQueries({ queryKey: ["gateway-catalog-entries"] });
    await queryClient.invalidateQueries({ queryKey: ["provider-quota"] });
  };

  const openCreate = () => {
    setEditing(null);
    setSelectedPresetId(null);
    form.resetFields();
    form.setFieldsValue({
      name: "",
      baseUrl: "",
      apiKey: "",
      model: "",
      protocolType: "anthropic",
      notes: "",
      targetApp: "claude_code",
      modelMapping: { sonnet: "", opus: "", haiku: "", fable: "", subagent: "" },
    });
    setOpen(true);
  };

  const openEdit = (provider: Provider) => {
    setEditing(provider);
    setSelectedPresetId(null);
    form.resetFields();
    form.setFieldsValue({
      id: provider.id,
      name: provider.name,
      baseUrl: provider.baseUrl,
      apiKey: "",
      model: provider.model,
      protocolType: provider.protocolType,
      notes: provider.notes,
      targetApp: "claude_code",
      modelMapping: { sonnet: "", opus: "", haiku: "", fable: "", subagent: "" },
    });
    setOpen(true);
  };

  const applyPreset = (preset: ProviderPreset) => {
    setSelectedPresetId(preset.id);
    form.setFieldsValue({
      name: preset.name,
      baseUrl: preset.baseUrl,
      model: preset.model,
      protocolType: preset.protocolType,
      notes: preset.notes ?? "",
    });
  };

  const clearPreset = () => {
    setSelectedPresetId(null);
    form.setFieldsValue({
      name: "",
      baseUrl: "",
      model: "",
      protocolType: "anthropic",
      notes: "",
    });
  };

  const normalizeBaseUrlField = () => {
    const value = form.getFieldValue("baseUrl");
    if (typeof value !== "string" || !value.trim()) return;
    try {
      let next = normalizeBaseUrl(value);
      const protocol = form.getFieldValue("protocolType") as ProtocolType;
      if (protocol === "openai_chat" || protocol === "openai_responses") {
        next = ensureOpenAiV1Suffix(next);
      }
      form.setFieldValue("baseUrl", next);
    } catch {
      // Keep invalid input so the validator can explain it.
    }
  };

  const appendV1Suffix = () => {
    const value = form.getFieldValue("baseUrl");
    if (typeof value !== "string" || !value.trim()) return;
    try {
      form.setFieldValue("baseUrl", ensureOpenAiV1Suffix(value));
    } catch {
      // Keep invalid input visible.
    }
  };

  const handleSave = async () => {
    const values = await form.validateFields();
    setSaving(true);
    try {
      let baseUrl = normalizeBaseUrl(values.baseUrl);
      if (values.protocolType === "openai_chat" || values.protocolType === "openai_responses") {
        baseUrl = ensureOpenAiV1Suffix(baseUrl);
      }
      if (isReservedListenerUrl(baseUrl)) {
        throw new Error(t("proxy.upstreamReservedUrl", { defaultValue: "上游不能指向本机 15821–15828" }));
      }
      await upsertGatewayUpstream({
        id: editing?.id,
        name: values.name,
        baseUrl,
        apiKey: values.apiKey ?? "",
        model: values.model,
        protocolType: values.protocolType,
        notes: values.notes ?? "",
        targetApp: "claude_code",
        modelMapping: { sonnet: "", opus: "", haiku: "", fable: "", subagent: "" },
        providerKind:
          editing?.providerKind === "antigravity" || editing?.providerKind === "kiro"
            ? editing.providerKind
            : "standard",
      });
      await invalidatePool();
      setOpen(false);
      void message.success(t("proxy.upstreamSaved"));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteGatewayUpstream(id);
      setSelectedIds((current) => current.filter((item) => item !== id));
      await invalidatePool();
      void message.success(t("proxy.upstreamDeleted"));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    }
  };

  const handleAddAg = async () => {
    setAddingAg(true);
    try {
      await addAntigravityGatewayUpstream();
      await invalidatePool();
      void message.success(t("proxy.upstreamAgAdded"));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setAddingAg(false);
    }
  };

  const handleAddKiro = async () => {
    setAddingKiro(true);
    try {
      await addKiroGatewayUpstream();
      await invalidatePool();
      void message.success(t("proxy.upstreamKiroAdded"));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setAddingKiro(false);
    }
  };

  const handleImport = async () => {
    if (importIds.length === 0) {
      void message.warning(t("proxy.importUpstreamSelect"));
      return;
    }
    setImporting(true);
    try {
      const result = await importGatewayUpstreamsFromProviders(
        importTarget,
        importIds,
        importAllowlist ? allowlistTarget : null,
      );
      await invalidatePool();
      setImportOpen(false);
      setImportIds([]);
      void message.success(
        t("proxy.importUpstreamDone", { imported: result.imported, skipped: result.skipped }),
      );
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setImporting(false);
    }
  };

  const handleRefreshSelected = async () => {
    if (selectedIds.length === 0) {
      void message.warning(t("proxy.refreshModelsSelect"));
      return;
    }
    setRefreshing(true);
    try {
      const results = await discoverGatewayUpstreamModelsBatch(selectedIds);
      await invalidatePool();
      if (modelsUpstream && selectedIds.includes(modelsUpstream.id)) {
        await queryClient.invalidateQueries({ queryKey: ["gateway-upstream-models", modelsUpstream.id] });
      }
      const failed = results.filter((item) => item.result.error);
      if (failed.length > 0) {
        void message.warning(
          t("proxy.refreshModelsPartial", {
            ok: results.length - failed.length,
            failed: failed.length,
          }),
        );
      } else {
        void message.success(t("proxy.refreshModelsDone", { count: results.length }));
      }
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setRefreshing(false);
    }
  };

  const handleToggleModel = async (modelId: string, visible: boolean) => {
    if (!modelsUpstream) return;
    setModelsSaving(true);
    try {
      const rows = await setGatewayUpstreamModelVisible(modelsUpstream.id, modelId, visible);
      queryClient.setQueryData(["gateway-upstream-models", modelsUpstream.id], rows);
      await queryClient.invalidateQueries({ queryKey: ["gateway-catalog-entries"] });
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setModelsSaving(false);
    }
  };

  const handleRefreshDrawer = async () => {
    if (!modelsUpstream) return;
    setModelsSaving(true);
    try {
      const result = await discoverGatewayUpstreamModels(modelsUpstream.id);
      await queryClient.invalidateQueries({ queryKey: ["gateway-upstream-models", modelsUpstream.id] });
      await invalidatePool();
      if (result.error) {
        void message.warning(result.error);
      } else {
        void message.success(t("proxy.refreshModelsDone", { count: 1 }));
      }
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setModelsSaving(false);
    }
  };

  const handleCodexOauthLogin = async () => {
    setOauthPolling(true);
    try {
      const device = await startCodexOauthLogin();
      setOauthDevice(device);
      await openUrl(device.verificationUri);
      const deadline = Date.now() + device.expiresIn * 1000;
      while (Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, Math.max(1, device.interval) * 1000));
        const result = await pollCodexOauthLogin(device.deviceCode);
        if (result.status === "pending") continue;
        if (result.status === "complete" && result.account) {
          await ensureCodexOauthProvider("claude_code", result.account.accountId);
          await invalidatePool();
          setOauthDevice(null);
          void message.success(
            t("providers.chatgptLoginSuccess", {
              defaultValue: "ChatGPT / Codex 账号登录成功，已作为上游加入上游池",
            }),
          );
          return;
        }
        throw new Error(result.message || t("providers.chatgptLoginFailed", { defaultValue: "登录失败" }));
      }
      throw new Error(t("providers.chatgptLoginExpired", { defaultValue: "登录已超时，请重试" }));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setOauthPolling(false);
    }
  };

  const handleTestUpstream = async (row: Provider) => {
    setTestingId(row.id);
    try {
      const result = await testUpstreamConnection(row.id);
      if (result.ok) {
        void message.success(
          t("proxy.upstreamTestSuccess", {
            name: row.name,
            ms: result.latencyMs ?? 0,
            defaultValue: `上游 [${row.name}] 连接成功 (${result.latencyMs ?? 0}ms)`,
          }),
        );
      } else {
        void message.error(
          t("proxy.upstreamTestFailed", {
            name: row.name,
            error: result.message || "连接失败",
            defaultValue: `上游 [${row.name}] 连接失败: ${result.message || "未知错误"}`,
          }),
        );
      }
      await queryClient.invalidateQueries({ queryKey: ["gateway-upstream-health"] });
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setTestingId(null);
    }
  };

  const handleExportUpstreams = async () => {
    try {
      const jsonText = await exportGatewayUpstreams();
      await navigator.clipboard.writeText(jsonText);
      void message.success(
        t("proxy.exportUpstreamCopied", { defaultValue: "上游配置 JSON 已复制到剪贴板" }),
      );
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    }
  };

  const handleImportUpstreamsSubmit = async () => {
    if (!importJsonText.trim()) {
      void message.warning(t("proxy.importJsonEmpty", { defaultValue: "请先粘贴上游配置 JSON" }));
      return;
    }
    setImportJsonLoading(true);
    try {
      await importGatewayUpstreamsJson(importJsonText);
      await invalidatePool();
      setImportJsonOpen(false);
      setImportJsonText("");
      void message.success(t("proxy.importUpstreamSuccess", { defaultValue: "成功导入上游配置" }));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setImportJsonLoading(false);
    }
  };

  const handleImportLiveAsUpstreams = async (target: ProviderTarget) => {
    try {
      const result = await importLiveConfigAsUpstreams(target);
      await invalidatePool();
      void message.success(
        t("proxy.importLiveDone", {
          client: t(LABEL_KEYS[target] ?? `workspace.${target}`),
          imported: result.imported ?? 0,
          skipped: result.skipped ?? 0,
          defaultValue: `已从 ${t(LABEL_KEYS[target] ?? `workspace.${target}`)} 本机配置导入 ${result.imported ?? 0} 个上游（跳过 ${result.skipped ?? 0} 个）`,
        }),
      );
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <Card
      size="small"
      className="page-surface"
      title={t("proxy.upstreamPool")}
      extra={
        <Space wrap>
          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={refreshing}
            disabled={selectedIds.length === 0}
            onClick={() => void handleRefreshSelected()}
          >
            {t("proxy.refreshSelectedModels")}
          </Button>
          <Button size="small" loading={addingAg} onClick={() => void handleAddAg()}>
            {t("proxy.addAgUpstream")}
          </Button>
          <Button size="small" loading={addingKiro} onClick={() => void handleAddKiro()}>
            {t("proxy.addKiroUpstream")}
          </Button>
          <Dropdown
            menu={{
              items: [
                {
                  key: "oauth",
                  icon: <LoginOutlined />,
                  label: t("providers.chatgptLogin", { defaultValue: "ChatGPT / Codex OAuth" }),
                  disabled: oauthPolling,
                  onClick: () => void handleCodexOauthLogin(),
                },
                {
                  key: "importLive",
                  icon: <ScanOutlined />,
                  label: t("proxy.importLiveAsUpstreams", { defaultValue: "从本机 Live 配置导入上游" }),
                  children: [
                    {
                      key: "importLiveCode",
                      label: "Claude Code",
                      onClick: () => void handleImportLiveAsUpstreams("claude_code"),
                    },
                    {
                      key: "importLiveCodex",
                      label: "Codex",
                      onClick: () => void handleImportLiveAsUpstreams("codex"),
                    },
                    {
                      key: "importLiveOpenCode",
                      label: "OpenCode",
                      onClick: () => void handleImportLiveAsUpstreams("opencode"),
                    },
                  ],
                },
                {
                  key: "importProviders",
                  icon: <ImportOutlined />,
                  label: t("proxy.importFromProviders"),
                  onClick: () => {
                    setImportTarget(allowlistTarget);
                    setImportIds([]);
                    setImportAllowlist(true);
                    setImportOpen(true);
                  },
                },
                {
                  key: "importJson",
                  icon: <ImportOutlined />,
                  label: t("proxy.importUpstreams", { defaultValue: "导入 JSON" }),
                  onClick: () => {
                    setImportJsonText("");
                    setImportJsonOpen(true);
                  },
                },
                {
                  key: "exportJson",
                  icon: <ExportOutlined />,
                  label: t("proxy.exportUpstreams", { defaultValue: "导出 JSON" }),
                  onClick: () => void handleExportUpstreams(),
                },
              ],
            }}
          >
            <Button size="small">
              <Space size={4}>
                {t("proxy.moreActions", { defaultValue: "更多操作" })}
                <DownOutlined style={{ fontSize: 10 }} />
              </Space>
            </Button>
          </Dropdown>
          <Button size="small" type="primary" icon={<PlusOutlined />} onClick={openCreate}>
            {t("proxy.addUpstream")}
          </Button>
        </Space>
      }
    >
      <Text type="secondary" style={{ display: "block", marginBottom: 12, fontSize: 12 }}>
        {t("proxy.upstreamPoolHint")}
      </Text>
      <Table
        size="small"
        rowKey="id"
        pagination={false}
        loading={upstreamsQuery.isLoading}
        dataSource={upstreamsQuery.data ?? []}
        locale={{ emptyText: t("proxy.upstreamEmpty") }}
        rowSelection={{
          selectedRowKeys: selectedIds,
          onChange: (keys) => setSelectedIds(keys.map(String)),
        }}
        columns={[
          { title: t("proxy.upstreamName"), dataIndex: "name", ellipsis: true },
          { title: t("proxy.upstreamUrl"), dataIndex: "baseUrl", ellipsis: true },
          { title: t("proxy.upstreamModel"), dataIndex: "model", ellipsis: true, width: 160 },
          {
            title: t("proxy.upstreamProtocol"),
            dataIndex: "protocolType",
            width: 140,
          },
          {
            title: t("proxy.upstreamQuota"),
            width: 200,
            render: (_, row: Provider) =>
              canQueryUpstreamQuota(row) ? (
                <ProviderQuotaView providerId={row.id} />
              ) : null,
          },
          {
            title: t("proxy.upstreamHealth", { defaultValue: "健康" }),
            width: 180,
            render: (_: unknown, row: Provider) =>
              healthLabel(
                (healthQuery.data ?? []).find((item) => item.upstreamId === row.id),
                t,
              ),
          },
          {
            title: t("proxy.upstreamActions"),
            width: 240,
            render: (_, row: Provider) => (
              <Space size={4}>
                <Button
                  type="link"
                  size="small"
                  icon={<ThunderboltOutlined />}
                  loading={testingId === row.id}
                  onClick={() => void handleTestUpstream(row)}
                >
                  {t("proxy.testConnection", { defaultValue: "测试" })}
                </Button>
                <Button type="link" size="small" onClick={() => setModelsUpstream(row)}>
                  {t("proxy.upstreamModels")}
                </Button>
                <Button type="link" size="small" onClick={() => openEdit(row)}>
                  {t("common.edit")}
                </Button>
                <Popconfirm
                  title={t("proxy.deleteUpstreamConfirm")}
                  onConfirm={() => void handleDelete(row.id)}
                >
                  <Button type="link" size="small" danger>
                    {t("common.delete")}
                  </Button>
                </Popconfirm>
              </Space>
            ),
          },
        ]}
      />
      <Modal
        open={open}
        title={editing ? t("proxy.editUpstream") : t("proxy.addUpstream")}
        onCancel={() => setOpen(false)}
        onOk={() => void handleSave()}
        confirmLoading={saving}
        destroyOnHidden
      >
        <Form form={form} layout="vertical">
          {!editing ? (
            <Form.Item
              label={t("providers.fromPreset")}
              extra={t("providers.fromPresetHint")}
            >
              <Space wrap size={[8, 8]}>
                <Button
                  size="small"
                  type={selectedPresetId === null ? "primary" : "default"}
                  onClick={clearPreset}
                >
                  {t("providers.blankPreset")}
                </Button>
                {UPSTREAM_PRESETS.map((preset) => (
                  <Button
                    key={preset.id}
                    size="small"
                    type={selectedPresetId === preset.id ? "primary" : "default"}
                    onClick={() => applyPreset(preset)}
                  >
                    {preset.name}
                    {preset.protocolType === "openai_chat"
                      ? " · Chat"
                      : preset.protocolType === "openai_responses"
                        ? " · Responses"
                        : ""}
                  </Button>
                ))}
              </Space>
            </Form.Item>
          ) : null}
          <Form.Item name="name" label={t("proxy.upstreamName")} rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item
            name="baseUrl"
            label={t("proxy.upstreamUrl")}
            extra={
              <Space direction="vertical" size={2}>
                {endpointPreview ? (
                  <>
                    <Text type="secondary">{t("providers.endpointPreview")}</Text>
                    <Text code copyable>{endpointPreview}</Text>
                  </>
                ) : (
                  <Text type="secondary">{t("providers.baseUrlHint")}</Text>
                )}
                {showAppendV1 ? (
                  <Button type="link" size="small" onClick={appendV1Suffix} style={{ paddingInline: 0 }}>
                    {t("providers.appendV1")}
                  </Button>
                ) : null}
              </Space>
            }
            rules={[
              { required: true },
              {
                validator: async (_, value: unknown) => {
                  if (typeof value !== "string" || !value.trim()) return;
                  try {
                    const normalized = normalizeBaseUrl(value);
                    if (isReservedListenerUrl(normalized)) {
                      throw new Error("upstreamReservedUrl");
                    }
                  } catch (error) {
                    const key = error instanceof Error ? error.message : "invalidBaseUrl";
                    if (key === "upstreamReservedUrl") {
                      throw new Error(
                        t("proxy.upstreamReservedUrl", { defaultValue: "上游不能指向本机 15821–15828" }),
                      );
                    }
                    throw new Error(t(`providers.${key}`));
                  }
                },
              },
            ]}
          >
            <AutoComplete
              options={urlOptions}
              placeholder="https://api.deepseek.com/anthropic"
              onBlur={normalizeBaseUrlField}
              filterOption={(input, option) =>
                String(option?.value ?? "").toLowerCase().includes(input.trim().toLowerCase())
              }
            />
          </Form.Item>
          <Form.Item
            name="apiKey"
            label={t("proxy.upstreamKey")}
            extra={editing ? t("proxy.upstreamKeyKeep") : undefined}
          >
            <Input.Password />
          </Form.Item>
          <Form.Item name="model" label={t("proxy.upstreamModel")} rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="protocolType" label={t("proxy.upstreamProtocol")} rules={[{ required: true }]}>
            <Select options={PROTOCOL_OPTIONS} />
          </Form.Item>
          <Form.Item name="notes" label={t("proxy.upstreamNotes")}>
            <Input.TextArea rows={2} />
          </Form.Item>
        </Form>
      </Modal>
      <Modal
        open={importOpen}
        title={t("proxy.importFromProviders")}
        onCancel={() => setImportOpen(false)}
        onOk={() => void handleImport()}
        confirmLoading={importing}
        destroyOnHidden
      >
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <div>
            <Text type="secondary" style={{ display: "block", marginBottom: 8, fontSize: 12 }}>
              {t("proxy.importUpstreamHint")}
            </Text>
            <Select
              style={{ width: "100%" }}
              value={importTarget}
              onChange={(value) => {
                setImportTarget(value);
                setImportIds([]);
              }}
              options={filterUiAgents(PROVIDER_TARGET_OPTIONS).map((item) => ({
                value: item,
                label: t(LABEL_KEYS[item]),
              }))}
            />
          </div>
          <Checkbox.Group
            style={{ display: "flex", flexDirection: "column", gap: 8 }}
            value={importIds}
            onChange={(values) => setImportIds(values.map(String))}
            options={importableProviders.map((item) => ({
              value: item.id,
              label: `${item.name} · ${item.baseUrl}`,
            }))}
          />
          {importableProviders.length === 0 && (
            <Text type="secondary">{t("proxy.importUpstreamEmpty")}</Text>
          )}
          <Checkbox checked={importAllowlist} onChange={(event) => setImportAllowlist(event.target.checked)}>
            {t("proxy.importAddAllowlist")}
          </Checkbox>
        </Space>
      </Modal>
      <Drawer
        title={modelsUpstream ? t("proxy.upstreamModelsTitle", { name: modelsUpstream.name }) : t("proxy.upstreamModels")}
        open={Boolean(modelsUpstream)}
        onClose={() => setModelsUpstream(null)}
        width={420}
        extra={
          <Button size="small" loading={modelsSaving} onClick={() => void handleRefreshDrawer()}>
            {t("proxy.refreshModels")}
          </Button>
        }
      >
        <Text type="secondary" style={{ display: "block", marginBottom: 12, fontSize: 12 }}>
          {t("proxy.upstreamModelsHint")}
        </Text>
        <Space direction="vertical" size="small" style={{ width: "100%" }}>
          {(modelsQuery.data ?? []).map((row) => {
            const isDefault =
              Boolean(modelsUpstream) &&
              row.modelId.trim().toLowerCase() === (modelsUpstream?.model ?? "").trim().toLowerCase();
            return (
              <Space
                key={row.modelId}
                align="center"
                style={{ width: "100%", justifyContent: "space-between" }}
              >
                <Text ellipsis style={{ maxWidth: 260 }}>
                  {row.modelId}
                  {isDefault ? ` (${t("proxy.upstreamDefaultModel")})` : ""}
                </Text>
                <Switch
                  size="small"
                  checked={row.visible}
                  disabled={isDefault || modelsSaving}
                  onChange={(checked) => void handleToggleModel(row.modelId, checked)}
                />
              </Space>
            );
          })}
          {(modelsQuery.data ?? []).length === 0 && (
            <Text type="secondary">{t("proxy.upstreamModelsEmpty")}</Text>
          )}
        </Space>
      </Drawer>
      <Modal
        open={Boolean(oauthDevice)}
        title={t("providers.chatgptLogin", { defaultValue: "ChatGPT / Codex OAuth 登录" })}
        onCancel={() => {
          setOauthDevice(null);
          setOauthPolling(false);
        }}
        footer={null}
        destroyOnHidden
      >
        <Space direction="vertical" size="middle" style={{ width: "100%", padding: "12px 0" }}>
          <Text>
            {t("providers.chatgptLoginPrompt", {
              defaultValue: "请在打开的浏览器页面中确认授权，输入以下设备代码：",
            })}
          </Text>
          <div
            style={{
              textAlign: "center",
              padding: "16px",
              background: "var(--ant-color-fill-quaternary, rgba(0,0,0,0.04))",
              borderRadius: 8,
            }}
          >
            <Text strong copyable style={{ fontSize: 24, letterSpacing: 2 }}>
              {oauthDevice?.userCode}
            </Text>
          </div>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("providers.chatgptLoginPolling", {
              defaultValue: "正在等待授权完成，请勿关闭此窗口...",
            })}
          </Text>
        </Space>
      </Modal>
      <Modal
        open={importJsonOpen}
        title={t("proxy.importJsonTitle", { defaultValue: "导入全局上游 JSON" })}
        onCancel={() => setImportJsonOpen(false)}
        onOk={() => void handleImportUpstreamsSubmit()}
        confirmLoading={importJsonLoading}
        destroyOnHidden
      >
        <Space direction="vertical" size="small" style={{ width: "100%" }}>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("proxy.importJsonHint", { defaultValue: "粘贴此前导出的上游 JSON 内容进行批量导入。" })}
          </Text>
          <Input.TextArea
            rows={8}
            value={importJsonText}
            onChange={(e) => setImportJsonText(e.target.value)}
            placeholder='[{"name": "...", "baseUrl": "...", ...}]'
          />
        </Space>
      </Modal>
    </Card>
  );
}
