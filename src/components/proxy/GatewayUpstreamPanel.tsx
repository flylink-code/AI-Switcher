import { useEffect, useMemo, useRef, useState } from "react";
import {
  Button,
  Card,
  Dropdown,
  Form,
  Modal,
  Popconfirm,
  Space,
  Table,
  Tag,
  Tooltip,
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
  batchSpeedtestUpstreamEndpoints,
  cancelBatchSpeedtest,
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
  listUpstreamDailyUsageStats,
  listProviders,
  pollCodexOauthLogin,
  setGatewayUpstreamModelVisible,
  startCodexOauthLogin,
  testUpstreamConnection,
  upsertGatewayUpstream,
} from "@/services/providers";
import { LABEL_KEYS } from "@/components/AgentTargetSwitcher";
import type { ProviderPreset } from "@/lib/providerPresets";
import {
  ensureOpenAiV1Suffix,
  isReservedListenerUrl,
  normalizeBaseUrl,
} from "@/lib/providerUrl";
import { ProviderQuotaView } from "@/components/ProviderQuotaView";
import type {
  BatchSpeedtestResult,
  CodexOauthDeviceStart,
  EndpointSpeedtestResult,
  Provider,
  UpstreamDailyUsageStat,
  ProviderInput,
  ProviderTarget,
  ProtocolType,
} from "@/types/backend";
import {
  CodexOauthModal,
  UPSTREAM_PRESETS,
  UpstreamFormModal,
  UpstreamHealthBadge,
  UpstreamImportJsonModal,
  UpstreamImportModal,
  UpstreamModelsDrawer,
  UpstreamPoolSummary,
} from "./upstream";
import { UpstreamLimitsModal } from "./upstream/UpstreamLimitsModal";
import { refreshUsageQuery, useUsageLogRefresh } from "@/lib/useUsageLogRefresh";
import { UpstreamDailyStats } from "./upstream/UpstreamDailyStats";

const { Text } = Typography;

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
  const [limitsUpstream, setLimitsUpstream] = useState<Provider | null>(null);
  const [modelsSaving, setModelsSaving] = useState(false);

  // Codex / ChatGPT OAuth state
  const [oauthDevice, setOauthDevice] = useState<CodexOauthDeviceStart | null>(null);
  const [oauthPolling, setOauthPolling] = useState(false);

  // Test connection state
  const [testingId, setTestingId] = useState<string | null>(null);

  // Batch speedtest state
  const [batchSpeedtesting, setBatchSpeedtesting] = useState(false);
  const [speedtestResults, setSpeedtestResults] = useState<
    Record<string, EndpointSpeedtestResult & { cancelled?: boolean }>
  >({});
  const [resultsModalOpen, setResultsModalOpen] = useState(false);
  const [lastBatchResult, setLastBatchResult] = useState<BatchSpeedtestResult | null>(null);
  const activeBatchRef = useRef<{ batchId: string; abortController: AbortController } | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (activeBatchRef.current) {
        activeBatchRef.current.abortController.abort();
        activeBatchRef.current = null;
      }
    };
  }, []);

  // JSON Import state
  const [importJsonOpen, setImportJsonOpen] = useState(false);
  const [importJsonText, setImportJsonText] = useState("");
  const [importJsonLoading, setImportJsonLoading] = useState(false);

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

  // 提升上游当日用量统计查询至父面板（30秒周期刷新），子行组件仅负责数据渲染
  const dailyStatsQuery = useQuery({
    queryKey: ["upstream-daily-usage"],
    queryFn: () => listUpstreamDailyUsageStats(),
    staleTime: 10_000,
  });
  useUsageLogRefresh({
    pollIntervalMs: 30_000,
    onRefresh: () => refreshUsageQuery(dailyStatsQuery),
  });

  const dailyStatsMap = useMemo(() => {
    const map = new Map<string, UpstreamDailyUsageStat>();
    if (dailyStatsQuery.data) {
      for (const item of dailyStatsQuery.data) {
        map.set(item.upstreamId, item);
      }
    }
    return map;
  }, [dailyStatsQuery.data]);

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

  const handleBatchSpeedtest = async () => {
    const pool = upstreamsQuery.data ?? [];
    const targetIds = selectedIds.length > 0 ? selectedIds : pool.map((item) => item.id);
    if (targetIds.length === 0) {
      void message.warning(t("proxy.batchSpeedtestEmpty", { defaultValue: "没有可测速的上游" }));
      return;
    }

    const batchId =
      typeof crypto !== "undefined" && crypto.randomUUID
        ? crypto.randomUUID()
        : `batch_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
    const abortController = new AbortController();
    activeBatchRef.current = { batchId, abortController };
    setBatchSpeedtesting(true);

    try {
      const res = await batchSpeedtestUpstreamEndpoints(targetIds, 4, batchId, abortController.signal);
      if (!mountedRef.current || activeBatchRef.current?.batchId !== batchId) {
        return;
      }

      setSpeedtestResults((prev) => {
        const next = { ...prev };
        for (const item of res.items) {
          next[item.id] = {
            ...item.result,
            cancelled: item.cancelled,
          };
        }
        return next;
      });
      setLastBatchResult(res);
      setResultsModalOpen(true);

      const okCount = res.items.filter((i) => !i.cancelled && i.result.ok).length;
      const failedCount = res.items.filter((i) => !i.cancelled && !i.result.ok).length;

      if (res.cancelled) {
        void message.info(
          t("proxy.batchSpeedtestCancelled", {
            completed: res.completed,
            total: res.total,
            defaultValue: `批量测速已取消：已完成 ${res.completed} / ${res.total}`,
          }),
        );
      } else if (failedCount > 0) {
        void message.warning(
          t("proxy.batchSpeedtestPartial", {
            ok: okCount,
            failed: failedCount,
            total: res.total,
            defaultValue: `批量测速完成：成功 ${okCount}，失败 ${failedCount} / ${res.total}`,
          }),
        );
      } else {
        void message.success(
          t("proxy.batchSpeedtestDone", {
            ok: okCount,
            total: res.total,
            defaultValue: `批量测速完成：全部 ${okCount} 个成功`,
          }),
        );
      }
    } catch (error) {
      if (mountedRef.current) {
        void message.error(error instanceof Error ? error.message : String(error));
      }
    } finally {
      if (activeBatchRef.current?.batchId === batchId) {
        activeBatchRef.current = null;
      }
      if (mountedRef.current) {
        setBatchSpeedtesting(false);
      }
    }
  };

  const handleCancelBatchSpeedtest = async () => {
    if (activeBatchRef.current) {
      const { batchId, abortController } = activeBatchRef.current;
      abortController.abort();
      try {
        await cancelBatchSpeedtest(batchId);
        if (mountedRef.current) {
          void message.info(
            t("proxy.batchSpeedtestCancelling", { defaultValue: "正在取消测速..." }),
          );
        }
      } catch (error) {
        if (mountedRef.current) {
          void message.error(error instanceof Error ? error.message : String(error));
        }
      }
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

  const upstreams = upstreamsQuery.data ?? [];
  const healthList = healthQuery.data ?? [];

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
            {t("proxy.refreshSelectedModels", { count: selectedIds.length })}
          </Button>
          <Button
            size="small"
            icon={<ThunderboltOutlined />}
            loading={batchSpeedtesting}
            disabled={batchSpeedtesting || upstreams.length === 0}
            onClick={() => void handleBatchSpeedtest()}
          >
            {selectedIds.length > 0
              ? t("proxy.batchSpeedtestSelected", {
                  count: selectedIds.length,
                  defaultValue: `批量测速 (${selectedIds.length})`,
                })
              : t("proxy.batchSpeedtest", { defaultValue: "批量测速" })}
          </Button>
          {batchSpeedtesting && (
            <Button size="small" danger onClick={() => void handleCancelBatchSpeedtest()}>
              {t("proxy.cancelBatchSpeedtest", { defaultValue: "取消测速" })}
            </Button>
          )}
          {lastBatchResult && !batchSpeedtesting && (
            <Button size="small" onClick={() => setResultsModalOpen(true)}>
              {t("proxy.batchSpeedtestResults", { defaultValue: "测速结果" })}
            </Button>
          )}
          <Dropdown
            menu={{
              items: [
                {
                  key: "addAg",
                  icon: <ThunderboltOutlined />,
                  label: t("proxy.addAntigravityUpstream", { defaultValue: "添加 Antigravity (:15830)" }),
                  disabled: addingAg,
                  onClick: () => void handleAddAg(),
                },
                {
                  key: "addKiro",
                  icon: <ThunderboltOutlined />,
                  label: t("proxy.addKiroUpstream", { defaultValue: "添加 Kiro (:15831)" }),
                  disabled: addingKiro,
                  onClick: () => void handleAddKiro(),
                },
                {
                  key: "addCodexOauth",
                  icon: <LoginOutlined />,
                  label: t("providers.chatgptLogin", { defaultValue: "ChatGPT / Codex OAuth 登录" }),
                  disabled: oauthPolling,
                  onClick: () => void handleCodexOauthLogin(),
                },
                { type: "divider" },
                {
                  key: "importFromProviders",
                  icon: <ImportOutlined />,
                  label: t("proxy.importFromProviders"),
                  onClick: () => setImportOpen(true),
                },
                {
                  key: "importLive",
                  icon: <ScanOutlined />,
                  label: t("proxy.importFromLiveGroup", { defaultValue: "从本机配置导入..." }),
                  children: (["claude_code", "codex", "opencode"] as ProviderTarget[]).map((target) => ({
                    key: `importLive_${target}`,
                    label: t(LABEL_KEYS[target] ?? `workspace.${target}`),
                    onClick: () => void handleImportLiveAsUpstreams(target),
                  })),
                },
                { type: "divider" },
                {
                  key: "importJson",
                  icon: <ImportOutlined />,
                  label: t("proxy.importJsonTitle", { defaultValue: "导入 JSON" }),
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
      <Text type="secondary" style={{ display: "block", marginBottom: 8, fontSize: 12 }}>
        {t("proxy.upstreamPoolHint")}
      </Text>

      {/* Upstream Health Overview */}
      <UpstreamPoolSummary upstreams={upstreams} healthList={healthList} />

      <Table
        size="small"
        rowKey="id"
        pagination={false}
        loading={upstreamsQuery.isLoading}
        dataSource={upstreams}
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
            title: t("upstreamStats.title"),
            width: 180,
            render: (_, row: Provider) => (
              <UpstreamDailyStats
                stat={dailyStatsMap.get(row.id)}
                isLoading={dailyStatsQuery.isPending}
                isError={dailyStatsQuery.isError}
              />
            ),
          },
          {
            title: t("proxy.upstreamHealth", { defaultValue: "健康" }),
            width: 180,
            render: (_: unknown, row: Provider) => {
              const speedtest = speedtestResults[row.id];
              return (
                <Space direction="vertical" size={2}>
                  <UpstreamHealthBadge
                    health={healthList.find((item) => item.upstreamId === row.id)}
                    t={t}
                  />
                  {speedtest && (
                    <Tooltip
                      title={`${speedtest.message} · ${new Date(speedtest.checkedAt).toLocaleTimeString()}`}
                    >
                      <Tag
                        color={speedtest.cancelled ? "default" : speedtest.ok ? "success" : "error"}
                        style={{ marginInlineEnd: 0, fontSize: 11 }}
                      >
                        {speedtest.cancelled
                          ? t("proxy.speedtestCancelled", { defaultValue: "已取消" })
                          : speedtest.ok
                          ? `RTT ${speedtest.latencyMs ?? 0}ms`
                          : t("proxy.speedtestFailed", { defaultValue: "测速失败" })}
                      </Tag>
                    </Tooltip>
                  )}
                </Space>
              );
            },
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
                <Button type="link" size="small" onClick={() => setLimitsUpstream(row)}>
                  {t("upstreamLimits.button", { defaultValue: "网关限额" })}
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

      <UpstreamLimitsModal
        open={Boolean(limitsUpstream)}
        upstreamId={limitsUpstream?.id ?? null}
        upstreamName={limitsUpstream?.name ?? ""}
        onClose={() => setLimitsUpstream(null)}
      />

      <UpstreamFormModal
        open={open}
        editing={editing}
        form={form}
        saving={saving}
        selectedPresetId={selectedPresetId}
        urlOptions={urlOptions}
        onClearPreset={clearPreset}
        onApplyPreset={applyPreset}
        onNormalizeBaseUrl={normalizeBaseUrlField}
        onAppendV1Suffix={appendV1Suffix}
        onSubmit={() => void handleSave()}
        onClose={() => setOpen(false)}
      />

      <UpstreamImportModal
        open={importOpen}
        importTarget={importTarget}
        importIds={importIds}
        importAllowlist={importAllowlist}
        importing={importing}
        importableProviders={importableProviders}
        onTargetChange={setImportTarget}
        onIdsChange={setImportIds}
        onAllowlistChange={setImportAllowlist}
        onSubmit={() => void handleImport()}
        onClose={() => setImportOpen(false)}
      />

      <UpstreamModelsDrawer
        upstream={modelsUpstream}
        models={modelsQuery.data ?? []}
        saving={modelsSaving}
        onClose={() => setModelsUpstream(null)}
        onRefresh={() => void handleRefreshDrawer()}
        onToggleModel={(modelId, visible) => void handleToggleModel(modelId, visible)}
      />

      <CodexOauthModal
        device={oauthDevice}
        onClose={() => {
          setOauthDevice(null);
          setOauthPolling(false);
        }}
      />

      <UpstreamImportJsonModal
        open={importJsonOpen}
        text={importJsonText}
        loading={importJsonLoading}
        onTextChange={setImportJsonText}
        onSubmit={() => void handleImportUpstreamsSubmit()}
        onClose={() => setImportJsonOpen(false)}
      />
      <Modal
        title={t("proxy.batchSpeedtestResults", { defaultValue: "测速结果" })}
        open={resultsModalOpen}
        onCancel={() => setResultsModalOpen(false)}
        footer={[
          <Button key="close" type="primary" onClick={() => setResultsModalOpen(false)}>
            {t("common.close", { defaultValue: "关闭" })}
          </Button>,
        ]}
        width={720}
        destroyOnHidden
      >
        {lastBatchResult && (
          <Space direction="vertical" style={{ width: "100%" }} size={12}>
            <Text type="secondary">
              {t("proxy.batchSpeedtestSummary", {
                total: lastBatchResult.total,
                ok: lastBatchResult.items.filter((i) => !i.cancelled && i.result.ok).length,
                failed: lastBatchResult.items.filter((i) => !i.cancelled && !i.result.ok).length,
                cancelled: lastBatchResult.items.filter((i) => i.cancelled).length,
                defaultValue: `总计 ${lastBatchResult.total} 项 · 成功 ${lastBatchResult.items.filter((i) => !i.cancelled && i.result.ok).length} · 失败 ${lastBatchResult.items.filter((i) => !i.cancelled && !i.result.ok).length} · 已取消 ${lastBatchResult.items.filter((i) => i.cancelled).length}`,
              })}
            </Text>
            <Table
              size="small"
              pagination={false}
              rowKey="id"
              dataSource={lastBatchResult.items}
              columns={[
                {
                  title: t("proxy.upstreamName"),
                  dataIndex: "name",
                  width: 140,
                  ellipsis: true,
                },
                {
                  title: t("proxy.speedtestStatus", { defaultValue: "状态" }),
                  width: 90,
                  render: (_, item) => (
                    <Tag
                      color={item.cancelled ? "default" : item.result.ok ? "success" : "error"}
                    >
                      {item.cancelled
                        ? t("proxy.speedtestCancelled", { defaultValue: "已取消" })
                        : item.result.ok
                        ? t("proxy.speedtestSuccess", { defaultValue: "成功" })
                        : t("proxy.speedtestFailed", { defaultValue: "失败" })}
                    </Tag>
                  ),
                },
                {
                  title: t("proxy.speedtestLatency", { defaultValue: "延迟" }),
                  width: 90,
                  render: (_, item) =>
                    item.result.latencyMs != null ? `${item.result.latencyMs} ms` : "—",
                },
                {
                  title: t("proxy.speedtestMessage", { defaultValue: "详情" }),
                  dataIndex: ["result", "message"],
                  ellipsis: true,
                },
              ]}
            />
          </Space>
        )}
      </Modal>
    </Card>
  );
}
