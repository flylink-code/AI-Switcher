import { useEffect, useState } from "react";
import {
  Alert,
  App,
  Button,
  Card,
  Modal,
  Popconfirm,
  Segmented,
  Select,
  Space,
  Table,
  Tag,
  Tooltip,
  Typography,
  type TableColumnsType,
} from "antd";
import { useQuery } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import ArrowDownOutlined from "@ant-design/icons/es/icons/ArrowDownOutlined";
import ArrowUpOutlined from "@ant-design/icons/es/icons/ArrowUpOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import EditOutlined from "@ant-design/icons/es/icons/EditOutlined";
import GlobalOutlined from "@ant-design/icons/es/icons/GlobalOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import SafetyCertificateOutlined from "@ant-design/icons/es/icons/SafetyCertificateOutlined";
import FieldTimeOutlined from "@ant-design/icons/es/icons/FieldTimeOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import { useTranslation } from "react-i18next";
import type {
  CodexOauthDeviceStart,
  ImportPreview,
  Provider,
  ProviderInput,
  ProviderTarget,
} from "@/types/backend";
import { useProvidersStore } from "@/stores/providersStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { ProviderForm } from "@/components/ProviderForm";
import { ImportPreviewDialog } from "@/components/ImportPreviewDialog";
import { OnboardingTip } from "@/components/OnboardingTip";
import { UsageCalendar } from "@/components/UsageCalendar";
import { UsageSourceFilterSegmented } from "@/components/UsageSourceFilterSegmented";
import {
  buildProviderDeeplink,
  confirmImportPreview,
  ensureCodexOauthProvider,
  exportProviders,
  getCodexAuthStatus,
  previewImportText,
  pollCodexOauthLogin,
  startCodexOauthLogin,
  speedtestProviderEndpoint,
  testProviderConnection,
} from "@/services/api";
import { usageTrendOptions } from "@/lib/appQueries";
import {
  USAGE_PERIOD_VALUES,
  usagePeriodLabelKey,
} from "@/utils/usagePeriod";

const { Text } = Typography;

export default function ProvidersPage() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const store = useProvidersStore();
  const target = usePagePreferencesStore((state) => state.providersTarget);
  const setTarget = usePagePreferencesStore((state) => state.setProvidersTarget);
  const heatmapPeriod = usePagePreferencesStore((state) => state.heatmapPeriod);
  const setHeatmapPeriod = usePagePreferencesStore((state) => state.setHeatmapPeriod);
  const heatmapSource = usePagePreferencesStore((state) => state.heatmapSource);
  const setHeatmapSource = usePagePreferencesStore((state) => state.setHeatmapSource);
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Provider | null>(null);
  const [busy, setBusy] = useState(false);
  const [importPreview, setImportPreview] = useState<ImportPreview | null>(null);
  const [importConfirming, setImportConfirming] = useState(false);
  const [codexAuth, setCodexAuth] = useState<{ loggedIn: boolean; loginCommand: string } | null>(null);
  const [oauthDevice, setOauthDevice] = useState<CodexOauthDeviceStart | null>(null);
  const [oauthPolling, setOauthPolling] = useState(false);
  const officialCurrent = !store.providers.some((provider) => provider.isCurrent);
  const usageQuery = useQuery(usageTrendOptions(heatmapPeriod, heatmapSource));

  useEffect(() => { void store.load(target); }, [store.load, target]);
  useEffect(() => {
    if (target !== "codex") return;
    void getCodexAuthStatus().then(setCodexAuth).catch(() => setCodexAuth(null));
  }, [target]);

  const openCreate = () => {
    setEditing(null);
    setFormOpen(true);
  };
  const openEdit = (provider: Provider) => {
    setEditing(provider);
    setFormOpen(true);
  };

  const handleCodexOauthLogin = async () => {
    setBusy(true);
    try {
      const device = await startCodexOauthLogin();
      setOauthDevice(device);
      setOauthPolling(true);
      await openUrl(device.verificationUri);
      const deadline = Date.now() + device.expiresIn * 1000;
      while (Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, Math.max(1, device.interval) * 1000));
        const result = await pollCodexOauthLogin(device.deviceCode);
        if (result.status === "pending") continue;
        if (result.status === "complete" && result.account) {
          await ensureCodexOauthProvider(target, result.account.accountId);
          await store.load(target);
          setOauthDevice(null);
          void message.success(t("providers.chatgptLoginSuccess"));
          return;
        }
        throw new Error(result.message || t("providers.chatgptLoginFailed"));
      }
      throw new Error(t("providers.chatgptLoginExpired"));
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setOauthPolling(false);
      setBusy(false);
    }
  };

  const handleSubmit = async (input: ProviderInput) => {
    setBusy(true);
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
    } finally { setBusy(false); }
  };

  const handleSwitch = async (provider: Provider) => {
    if (!provider.apiKeySet) {
      void message.warning(t("providers.missingKey"));
      return;
    }
    setBusy(true);
    try {
      const result = await store.switchTo(provider.id);
      void message.success(t("providers.switched", { name: provider.name }));
      void message.info(t("providers.hotSwitchHint"));
      const sync = result.sessionSync;
      if (sync) {
        if (sync.status === "warning") {
          void message.warning(sync.message);
        } else if (sync.changedSessionFiles > 0 || sync.sqliteRowsUpdated > 0) {
          void message.success(
            t("providers.sessionSyncSummary", {
              files: sync.changedSessionFiles,
              rows: sync.sqliteRowsUpdated,
            }),
          );
        }
      }
    } catch (e) {
      void message.error(errMsg(e));
    } finally { setBusy(false); }
  };

  const handleTest = async (provider: Provider) => {
    setBusy(true);
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
    } finally { setBusy(false); }
  };

  const handleSpeedtest = async (provider: Provider) => {
    setBusy(true);
    try {
      const result = await speedtestProviderEndpoint(provider.id);
      const notify = result.ok ? message.success : message.warning;
      void notify(result.message);
      useProvidersStore.setState((state) => ({
        providers: state.providers.map((item) =>
          item.id === provider.id
            ? {
                ...item,
                healthLatencyMs: result.latencyMs ?? item.healthLatencyMs ?? null,
              }
            : item,
        ),
      }));
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
    } catch (e) { void message.error(errMsg(e)); }
  };

  const handleImportFile = async (file: File) => {
    setBusy(true);
    try {
      const preview = await previewImportText(await file.text());
      setImportPreview(preview);
    } catch (e) { void message.error(errMsg(e)); }
    finally { setBusy(false); }
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

  const handleShareLink = async (provider: Provider) => {
    try {
      const link = await buildProviderDeeplink(provider.id);
      await navigator.clipboard.writeText(link);
      void message.success(t("deeplink.linkCopied"));
    } catch (e) {
      void message.error(errMsg(e));
    }
  };

  const handleOfficial = async () => {
    setBusy(true);
    try {
      await store.useOfficial();
      void message.success(t("providers.switchedOfficial"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally { setBusy(false); }
  };

  const handleDelete = async (provider: Provider) => {
    setBusy(true);
    try {
      await store.remove(provider.id);
      void message.success(t("providers.deleted"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally { setBusy(false); }
  };

  const handleImport = async () => {
    setBusy(true);
    try {
      await store.importLive();
      void message.success(t("providers.imported"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally { setBusy(false); }
  };

  const columns: TableColumnsType<Provider> = [
    { title: t("providers.colName"), dataIndex: "name", render: (_: string, row) => (
      <Space>
        <Text strong>{row.name}</Text>
        {row.isCurrent && <Tag color="green">{t("providers.current")}</Tag>}
        {row.healthStatus && (
          <Tag color={row.healthStatus === "healthy" ? "green" : "red"}>
            {row.healthStatus === "healthy" ? t("providers.healthy") : t("providers.unhealthy")}
            {row.healthLatencyMs != null ? ` · ${row.healthLatencyMs}ms` : ""}
          </Tag>
        )}
      </Space>
    ) },
    { title: t("providers.colBaseUrl"), dataIndex: "baseUrl", width: 280, ellipsis: true, render: (value: string) => <Text code copyable ellipsis={{ tooltip: value }}>{value}</Text> },
    {
      title: t("providers.colModel"),
      dataIndex: "model",
      width: 210,
      ellipsis: true,
      render: (_: string, row) => {
        if (row.targetApp === "codex") {
          return <Text ellipsis={{ tooltip: row.model }}>{row.model}</Text>;
        }
        const count = Object.entries(row.modelMapping)
          .filter(([key]) => key !== "subagent" || row.targetApp === "claude_code")
          .filter(([, value]) => value.trim()).length;
        return (
          <Space size={4}>
            <Text ellipsis={{ tooltip: row.model }}>{row.model}</Text>
            <Tag>{t("providers.mappingCount", { count })}</Tag>
          </Space>
        );
      },
    },
    { title: t("providers.colProtocol"), dataIndex: "protocolType", width: 150, render: (value: string) => <Tag color={value === "anthropic" ? "blue" : "orange"}>{value}</Tag> },
    {
      title: t("providers.colFailoverGroup"),
      dataIndex: "failoverGroup",
      width: 90,
      render: (value: number) => <Tag>{value ?? 0}</Tag>,
    },
    {
      title: t("providers.colActions"), key: "actions", width: 310,
      render: (_: unknown, row: Provider, index: number) => <Space size="small">
        <Tooltip title={t("providers.moveUp")}><Button size="small" icon={<ArrowUpOutlined />} disabled={index === 0 || busy} onClick={() => void store.move(row.id, -1)} /></Tooltip>
        <Tooltip title={t("providers.moveDown")}><Button size="small" icon={<ArrowDownOutlined />} disabled={index === store.providers.length - 1 || busy} onClick={() => void store.move(row.id, 1)} /></Tooltip>
        <Button size="small" type={row.isCurrent ? "default" : "primary"} disabled={row.isCurrent || busy} icon={<ThunderboltOutlined />} onClick={() => void handleSwitch(row)}>{t("providers.switch")}</Button>
        <Tooltip title={t("providers.testConnection")}><Button size="small" icon={<SafetyCertificateOutlined />} disabled={busy || !row.apiKeySet} onClick={() => void handleTest(row)} /></Tooltip>
        <Tooltip title={t("providers.speedtest")}><Button size="small" icon={<FieldTimeOutlined />} disabled={busy || !row.baseUrl} onClick={() => void handleSpeedtest(row)} /></Tooltip>
        <Tooltip title={t("deeplink.shareLink")}><Button size="small" icon={<GlobalOutlined />} disabled={busy} onClick={() => void handleShareLink(row)} /></Tooltip>
        <Tooltip title={t("providers.edit")}><Button size="small" icon={<EditOutlined />} disabled={busy} onClick={() => openEdit(row)} /></Tooltip>
        <Popconfirm title={t("providers.confirmDelete")} okText={t("providers.delete")} cancelText={t("providers.cancel")} onConfirm={() => void handleDelete(row)} disabled={busy}>
          <Tooltip title={t("providers.delete")}><Button size="small" danger icon={<DeleteOutlined />} disabled={busy} /></Tooltip>
        </Popconfirm>
      </Space>,
    },
  ];

  return <Space direction="vertical" size="middle" style={{ width: "100%", minWidth: 0 }}>
    {store.error && <Alert type="error" showIcon message={store.error} closable onClose={() => store.clearError()} />}
    <Space wrap size={[8, 8]} style={{ width: "100%", justifyContent: "space-between" }}>
      <Segmented<ProviderTarget>
        value={target}
        onChange={setTarget}
        options={[
          { value: "claude_code", label: t("providers.claudeCode") },
          { value: "claude_desktop", label: t("providers.claudeDesktop") },
          { value: "codex", label: "Codex" },
        ]}
      />
      <Space wrap size={[8, 8]}>
        {target !== "codex" && (
          <Button loading={oauthPolling} onClick={() => void handleCodexOauthLogin()}>
            {t("providers.chatgptLogin")}
          </Button>
        )}
        <Button icon={<ImportOutlined />} loading={busy} onClick={() => void handleImport()}>{t("providers.importLive")}</Button>
        <Button loading={busy} onClick={() => void handleExport()}>{t("providers.export")}</Button>
        <Button loading={busy} onClick={() => void handleImportClipboard()}>{t("providers.importClipboard")}</Button>
        <label><Button loading={busy}>{t("providers.importFile")}</Button><input type="file" accept="application/json" hidden onChange={(event) => { const file = event.target.files?.[0]; if (file) void handleImportFile(file); event.currentTarget.value = ""; }} /></label>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => openCreate()}>{t("providers.create")}</Button>
      </Space>
    </Space>
    <OnboardingTip
      tipKey="providers_hot_switch"
      type="info"
      message={t("providers.hotSwitchTitle")}
      description={t("providers.hotSwitchDescription")}
    />
    {target === "codex" && (
      <OnboardingTip
        tipKey="providers_codex_auth"
        type={codexAuth?.loggedIn ? "success" : "info"}
        message={codexAuth?.loggedIn ? t("providers.codexLoginDetected") : t("providers.codexLoginNeeded")}
        description={
          <Space direction="vertical" size={4}>
            <Space wrap>
              <Text code>{codexAuth?.loginCommand ?? "codex login"}</Text>
              <Button size="small" onClick={() => void navigator.clipboard?.writeText(codexAuth?.loginCommand ?? "codex login")}>
                复制命令
              </Button>
            </Space>
            <Text type="secondary">{t("providers.codexLoginHint")}</Text>
          </Space>
        }
      />
    )}
    <Card
      size="small"
      title={
        <Space>
          <GlobalOutlined />
          <Text strong>{t("providers.officialMode")}</Text>
          {officialCurrent && <Tag color="green">{t("providers.current")}</Tag>}
        </Space>
      }
      extra={
        <Button
          size="small"
          type={officialCurrent ? "default" : "primary"}
          icon={<ThunderboltOutlined />}
          loading={busy}
          disabled={officialCurrent}
          onClick={() => void handleOfficial()}
        >
          {t("providers.switch")}
        </Button>
      }
    >
      <Text type="secondary">{t("providers.officialModeDescription")}</Text>
    </Card>
    <Card
      size="small"
      styles={{ body: { padding: 12 } }}
      title={
        <Space wrap>
          <GlobalOutlined />
          {t("providers.title")}
          <Text type="secondary" style={{ fontWeight: "normal", fontSize: 12 }}>
            {target === "claude_code" ? t("providers.codeSubtitle") : target === "claude_desktop" ? t("providers.desktopSubtitle") : "管理 ~/.codex/config.toml 中的直连模型提供方"}
          </Text>
        </Space>
      }
    >
      <Table<Provider>
        rowKey="id"
        size="middle"
        loading={store.loading}
        dataSource={store.providers}
        columns={columns}
        pagination={false}
        tableLayout="fixed"
        scroll={{ x: 1100 }}
        locale={{ emptyText: t("providers.empty") }}
      />
    </Card>
    <Card
      size="small"
      title={t("usage.dailyStatistics")}
      extra={
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
      }
    >
      {usageQuery.error ? (
        <Alert type="error" showIcon message={errMsg(usageQuery.error)} />
      ) : (
        <UsageCalendar data={usageQuery.data?.trend ?? []} period={heatmapPeriod} />
      )}
    </Card>
    <ProviderForm
      open={formOpen}
      editing={editing}
      target={target}
      onCancel={() => {
        setFormOpen(false);
      }}
      onSubmit={handleSubmit}
    />
    <ImportPreviewDialog
      open={importPreview !== null}
      preview={importPreview}
      confirming={importConfirming}
      onCancel={() => setImportPreview(null)}
      onConfirm={() => void handleConfirmImport()}
    />
    <Modal
      open={oauthDevice !== null}
      title={t("providers.chatgptLoginTitle")}
      footer={null}
      closable={!oauthPolling}
      maskClosable={!oauthPolling}
      onCancel={() => setOauthDevice(null)}
    >
      <Space direction="vertical" style={{ width: "100%" }}>
        <Text>{t("providers.chatgptLoginInstructions")}</Text>
        <Typography.Title level={2} copyable style={{ margin: 0 }}>
          {oauthDevice?.userCode}
        </Typography.Title>
        <Button
          type="primary"
          onClick={() => oauthDevice && void openUrl(oauthDevice.verificationUri)}
        >
          {t("providers.openChatgptLogin")}
        </Button>
        {oauthPolling && <Text type="secondary">{t("providers.waitingAuthorization")}</Text>}
      </Space>
    </Modal>
  </Space>;
}

function errMsg(e: unknown): string { return e instanceof Error ? e.message : String(e); }
