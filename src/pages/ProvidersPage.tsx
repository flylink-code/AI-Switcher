import { useEffect, useRef, useState } from "react";
import {
  Alert,
  App,
  Badge,
  Button,
  Card,
  Dropdown,
  Modal,
  Popconfirm,
  Segmented,
  Space,
  Tag,
  Tooltip,
  Typography,
  type MenuProps,
} from "antd";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import MedicineBoxOutlined from "@ant-design/icons/es/icons/MedicineBoxOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import ExportOutlined from "@ant-design/icons/es/icons/ExportOutlined";
import FolderOpenOutlined from "@ant-design/icons/es/icons/FolderOpenOutlined";
import LoginOutlined from "@ant-design/icons/es/icons/LoginOutlined";
import EditOutlined from "@ant-design/icons/es/icons/EditOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import SwapOutlined from "@ant-design/icons/es/icons/SwapOutlined";
import ScanOutlined from "@ant-design/icons/es/icons/ScanOutlined";
import NodeIndexOutlined from "@ant-design/icons/es/icons/NodeIndexOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { CodexOauthDeviceStart, Provider, ProviderDoctorReport, ProviderTarget } from "@/types/backend";
import { useProvidersStore } from "@/stores/providersStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { ProviderForm } from "@/components/ProviderForm";
import { ImportPreviewDialog } from "@/components/ImportPreviewDialog";
import { ImportFromAgentDialog, canCopyProviderTo } from "@/components/ImportFromAgentDialog";
import { OnboardingTip } from "@/components/OnboardingTip";
import { ProviderBrandIcon } from "@/components/ProviderBrandIcon";
import { AgentTargetSwitcher, LABEL_KEYS, PROVIDER_TARGET_OPTIONS } from "@/components/AgentTargetSwitcher";
import { usageSourceIcon } from "@/components/UsageSourceIcons";
import { ResourceEmptyState } from "@/components/workspace/ResourceEmptyState";
import { managedAppsRuntimeStatusOptions, proxyStatusOptions } from "@/lib/appQueries";
import { useNavigatePage } from "@/lib/navigation";
import { errMsg, useProviderActions } from "@/lib/useProviderActions";
import {
  batchDiagnoseProviders,
  ensureCodexOauthProvider,
  getAntigravityGatewayStatus,
  startDshWeb,
  getCodexAuthStatus,
  getPaths,
  getPiSettings,
  pollCodexOauthLogin,
  quarantineFailedProviders,
  startCodexOauthLogin,
  updatePiSettings,
} from "@/services/api";

const { Text } = Typography;

/**
 * Providers page — classic cc-switch card list layout: a header row with the
 * page-local Agent switcher + runtime status tags + primary actions, then
 * full-width provider cards (official mode card first).
 */
export default function ProvidersPage() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const navigate = useNavigatePage();
  const store = useProvidersStore();
  const target = usePagePreferencesStore((state) => state.providersTarget);
  const setProvidersTarget = usePagePreferencesStore((state) => state.setProvidersTarget);
  const isCatalogTarget = target === "opencode" || target === "pi";

  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Provider | null>(null);
  const [importHint, setImportHint] = useState<string | null>(null);
  const [importFromOpen, setImportFromOpen] = useState(false);

  const [codexAuth, setCodexAuth] = useState<{ loggedIn: boolean; loginCommand: string } | null>(null);
  const [oauthDevice, setOauthDevice] = useState<CodexOauthDeviceStart | null>(null);
  const [oauthPolling, setOauthPolling] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const [doctorModalOpen, setDoctorModalOpen] = useState(false);
  const [doctorLoading, setDoctorLoading] = useState(false);
  const [doctorReports, setDoctorReports] = useState<ProviderDoctorReport[]>([]);
  const [quarantining, setQuarantining] = useState(false);
  const [piThinkingLevel, setPiThinkingLevel] = useState<string>("medium");
  const [startingDsh, setStartingDsh] = useState(false);

  const piSettingsQuery = useQuery({
    queryKey: ["pi-settings"],
    queryFn: async () => {
      const res = await getPiSettings();
      if (typeof res.defaultThinkingLevel === "string") {
        setPiThinkingLevel(res.defaultThinkingLevel);
      }
      return res;
    },
    enabled: target === "pi",
  });

  const handleRunDoctor = async () => {
    setDoctorLoading(true);
    try {
      const reports = await batchDiagnoseProviders(target);
      setDoctorReports(reports);
      setDoctorModalOpen(true);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setDoctorLoading(false);
    }
  };

  const handleQuarantineFailed = async () => {
    const failedIds = doctorReports.filter((r) => !r.ok && (r.category === "authentication" || r.statusCode === 401 || r.statusCode === 403)).map((r) => r.providerId);
    if (!failedIds.length) {
      void message.info(t("providers.noFailedToQuarantine", { defaultValue: "没有发现需要隔离的 401/403 鉴权异常节点" }));
      return;
    }
    setQuarantining(true);
    try {
      const count = await quarantineFailedProviders(failedIds);
      void message.success(t("providers.quarantinedSuccess", { count, defaultValue: `已成功隔离 ${count} 个失效节点` }));
      setDoctorModalOpen(false);
      await store.load(target);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setQuarantining(false);
    }
  };

  const handleUpdatePiThinkingLevel = async (level: string) => {
    setPiThinkingLevel(level);
    try {
      await updatePiSettings(null, null, level);
      void message.success(t("providers.piThinkingLevelUpdated", { defaultValue: `已设置 Pi 思考强度为: ${level}` }));
      void piSettingsQuery.refetch();
    } catch (e) {
      void message.error(errMsg(e));
    }
  };

  const {
    busy,
    setBusy,
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
    handleImportFile,
    handleConfirmImport,
    handleCopyToTarget,
  } = useProviderActions({
    target,
    editing,
    closeForm: () => {
      setFormOpen(false);
      setImportHint(null);
    },
  });

  const officialCurrent = !store.providers.some((provider) => provider.isCurrent);

  useEffect(() => {
    void store.load(target);
  }, [store.load, target]);

  useEffect(() => {
    if (target !== "codex") return;
    void getCodexAuthStatus().then(setCodexAuth).catch(() => setCodexAuth(null));
  }, [target]);

  // Header status tags
  const runtimeQuery = useQuery(managedAppsRuntimeStatusOptions);
  const proxyQuery = useQuery(proxyStatusOptions(target));
  const proxy = proxyQuery.data;
  const antigravityQuery = useQuery({
    queryKey: ["antigravity-gateway"],
    queryFn: getAntigravityGatewayStatus,
    refetchInterval: 5_000,
  });
  const antigravity = antigravityQuery.data;

  const appRunningKey =
    target === "claude_code"
      ? "claudeCode"
      : target === "claude_desktop"
        ? "claudeDesktop"
        : target === "opencode"
          ? "opencode"
          : "codex";
  const isAppRunning = Boolean(runtimeQuery.data?.[appRunningKey]);

  const openCreate = () => {
    setEditing(null);
    setImportHint(null);
    setFormOpen(true);
  };

  const openEdit = (provider: Provider, hint?: string | null) => {
    setEditing(provider);
    setImportHint(hint ?? null);
    setFormOpen(true);
  };

  const afterCopyToTarget = async (source: Provider, dest: ProviderTarget) => {
    const copied = await handleCopyToTarget(source, dest);
    if (!copied) return;
    const hint = t("providers.copiedAdjustHint", { agent: t(LABEL_KEYS[source.targetApp]) });
    if (dest !== target) {
      setProvidersTarget(dest);
    }
    await store.load(dest);
    openEdit(copied, hint);
  };

  const handleOpenOpencodeConfig = async () => {
    try {
      const paths = await getPaths();
      await revealItemInDir(paths.opencodeConfigPath);
    } catch (error) {
      void message.error(errMsg(error));
    }
  };

  const handleStartDshWeb = async () => {
    setStartingDsh(true);
    try {
      const url = await startDshWeb();
      await openUrl(url);
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setStartingDsh(false);
    }
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

  const importExportItems: MenuProps["items"] = [
    ...(target !== "codex" && !isCatalogTarget
      ? [
          {
            key: "chatgptLogin",
            icon: <LoginOutlined />,
            label: t("providers.chatgptLogin"),
            disabled: oauthPolling,
            onClick: () => void handleCodexOauthLogin(),
          },
        ]
      : []),
    {
      key: "importFromAgent",
      icon: <SwapOutlined />,
      label: t("providers.importFromAgent"),
      disabled: busy,
      onClick: () => setImportFromOpen(true),
    },
    {
      key: "importLive",
      icon: <ImportOutlined />,
      label:
        target === "opencode"
          ? t("providers.syncOpenCodeLive")
          : target === "pi"
            ? t("providers.syncPiLive")
            : t("providers.importLive"),
      disabled: busy,
      onClick: () => void handleImportLive(),
    },
    {
      key: "importClipboard",
      label: t("providers.importClipboard"),
      disabled: busy,
      onClick: () => void handleImportClipboard(),
    },
    {
      key: "importFile",
      label: t("providers.importFile"),
      disabled: busy,
      onClick: () => fileInputRef.current?.click(),
    },
    {
      key: "export",
      icon: <ExportOutlined />,
      label: t("providers.exportJson", { defaultValue: t("providers.export") }),
      disabled: busy,
      onClick: () => void handleExport(),
    },
    ...(target === "opencode"
      ? [
          { type: "divider" as const },
          {
            key: "opencodeConfig",
            icon: <FolderOpenOutlined />,
            label: t("providers.opencodeOpenConfig"),
            onClick: () => void handleOpenOpencodeConfig(),
          },
        ]
      : []),
  ];

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%", minWidth: 0 }}>
      {store.error && (
        <Alert type="error" showIcon message={store.error} closable onClose={() => store.clearError()} />
      )}

      {/* Header: page-local Agent switcher + runtime status + actions */}
      <div className="cc-workbench-header">
        <div className="cc-header-left">
          <AgentTargetSwitcher value={target} onChange={setProvidersTarget} targets={PROVIDER_TARGET_OPTIONS} />
          <Badge
            status={isAppRunning ? "success" : "default"}
            text={isAppRunning ? t("workbench.running") : t("workbench.stopped")}
          />
          <Tag
            icon={<NodeIndexOutlined />}
            color={isCatalogTarget ? "blue" : proxy?.running ? "green" : undefined}
            style={{ cursor: "pointer", margin: 0 }}
            onClick={() => navigate("proxy")}
          >
            {isCatalogTarget
              ? t("workbench.proxyDirect")
              : proxy?.running
                ? t("workbench.proxyRunning", { port: proxy.port })
                : t("workbench.proxyStopped")}
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
            <Button icon={<ScanOutlined />} loading={busy} onClick={() => void handleImportLive()}>
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
          <Button
            icon={<MedicineBoxOutlined />}
            loading={doctorLoading}
            onClick={() => void handleRunDoctor()}
          >
            {t("providers.providerDoctor", { defaultValue: "供应商诊断 (Doctor)" })}
          </Button>
          <Dropdown menu={{ items: importExportItems }} trigger={["click"]}>
            <Button icon={<ImportOutlined />} loading={oauthPolling}>
              {t("providers.importExport")}
            </Button>
          </Dropdown>
          <input
            ref={fileInputRef}
            type="file"
            accept="application/json"
            hidden
            onChange={(event) => {
              const file = event.target.files?.[0];
              if (file) void handleImportFile(file);
              event.currentTarget.value = "";
            }}
          />
        </div>
      </div>

      {/* Onboarding Tips */}
      {!isCatalogTarget && (
        <OnboardingTip
          tipKey="providers_hot_switch"
          type="info"
          message={t("providers.hotSwitchTitle")}
          description={t("providers.hotSwitchDescription")}
        />
      )}
      {target === "opencode" && (
        <Alert
          type="info"
          showIcon
          style={{ minHeight: "38px", padding: "6px 14px", borderRadius: "6px" }}
          message={
            <span style={{ fontSize: "12.5px" }}>
              <strong>{t("providers.opencodeNoSwitchTitle")}</strong> — {t("providers.opencodeNoSwitchDescription")}
            </span>
          }
        />
      )}
      {target === "pi" && (
        <>
          <Alert
            type="info"
            showIcon
            style={{ minHeight: "38px", padding: "6px 14px", borderRadius: "6px" }}
            message={
              <span style={{ fontSize: "12.5px" }}>
                <strong>{t("providers.piNoSwitchTitle")}</strong> — {t("providers.piNoSwitchDescription")}
              </span>
            }
          />
          <Card size="small" style={{ margin: "8px 0" }} className="page-surface">
            <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
              <Space>
                <strong>{t("providers.piThinkingLevelTitle", { defaultValue: "Pi 默认思考强度 (Thinking Level)" })}</strong>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {t("providers.piThinkingLevelHint", { defaultValue: "控制 Pi 模型 Reasoning/Thinking 思考过程" })}
                </Text>
              </Space>
              <Segmented
                value={piThinkingLevel}
                onChange={(val) => void handleUpdatePiThinkingLevel(String(val))}
                options={[
                  { label: "关闭 (off)", value: "off" },
                  { label: "极低 (minimal)", value: "minimal" },
                  { label: "低 (low)", value: "low" },
                  { label: "中 (medium)", value: "medium" },
                  { label: "高 (high)", value: "high" },
                  { label: "超高 (xhigh)", value: "xhigh" },
                  { label: "最大 (max)", value: "max" },
                ]}
              />
            </Space>
          </Card>
        </>
      )}
      {target === "dsh" && (
        <Space direction="vertical" size="small" style={{ width: "100%" }}>
          <Alert
            type="info"
            showIcon
            style={{ minHeight: "38px", padding: "6px 14px", borderRadius: "6px" }}
            message={
              <span style={{ fontSize: "12.5px" }}>
                <strong>{t("providers.dshNoSwitchTitle")}</strong> — {t("providers.dshNoSwitchDescription")}
              </span>
            }
          />
          <Button type="primary" icon={<NodeIndexOutlined />} loading={startingDsh} onClick={() => void handleStartDshWeb()}>
            {t("providers.startDshWeb")}
          </Button>
        </Space>
      )}
      {target === "codex" && (
        <OnboardingTip
          tipKey="providers_codex_auth"
          type={codexAuth?.loggedIn ? "success" : "info"}
          message={codexAuth?.loggedIn ? t("providers.codexLoginDetected") : t("providers.codexLoginNeeded")}
          description={
            <Space direction="vertical" size={4}>
              <Space wrap>
                <Text code>{codexAuth?.loginCommand ?? "codex login"}</Text>
                <Button
                  size="small"
                  onClick={() => void navigator.clipboard?.writeText(codexAuth?.loginCommand ?? "codex login")}
                >
                  {t("common.copy", { defaultValue: "复制命令" })}
                </Button>
              </Space>
              <Text type="secondary">{t("providers.codexLoginHint")}</Text>
            </Space>
          }
        />
      )}

      {/* Provider Card List */}
      <div className="cc-provider-list">
        {/* Official Provider Card — same 3-row structure as custom cards */}
        {target !== "opencode" && target !== "pi" && target !== "dsh" && (
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

              <div className="cc-provider-card-meta">
                <Tag style={{ margin: 0, borderRadius: 4, fontSize: 11, background: "var(--color-bg-subtle, rgba(0,0,0,0.04))" }}>
                  {t("providers.officialModeTag", { defaultValue: "官方" })}
                </Tag>
                <Tag style={{ margin: 0, borderRadius: 4, fontSize: 11, background: "var(--color-bg-subtle, rgba(0,0,0,0.04))" }}>
                  native
                </Tag>
              </div>

              <div className="cc-provider-card-footer">
                <Text type="secondary" ellipsis style={{ maxWidth: 220, fontSize: 11 }}>
                  {t("providers.officialModeHint", { defaultValue: "使用官方原生 API Endpoint / 账号凭据" })}
                </Text>
                <div className="cc-provider-actions" style={{ display: "flex", alignItems: "center", gap: 6, minHeight: 28 }}>
                  {!officialCurrent && (
                    <Button
                      type="primary"
                      size="small"
                      style={{ borderRadius: 6, fontSize: 12 }}
                      loading={switchingId === "official"}
                      onClick={() => void handleOfficial()}
                    >
                      {t("providers.switchTo")}
                    </Button>
                  )}
                </div>
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
              className={`cc-provider-card ${!isCatalogTarget && isCurrent ? "cc-provider-card-active" : ""}`}
            >
              <div className="cc-provider-card-body">
                <div className="cc-provider-card-header">
                  <div className="cc-provider-main">
                    <ProviderBrandIcon provider={provider} size={36} />
                    <div className="cc-provider-info">
                      <span className="cc-provider-name">{provider.name}</span>
                    </div>
                  </div>
                  <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    {!isCatalogTarget && isCurrent && (
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

                <div className="cc-provider-card-meta">
                  <Tag style={{ margin: 0, borderRadius: 4, fontSize: 11, background: "var(--color-bg-subtle, rgba(0,0,0,0.04))" }}>
                    Model: {provider.model || "Default"}
                  </Tag>
                  <Tag style={{ margin: 0, borderRadius: 4, fontSize: 11, background: "var(--color-bg-subtle, rgba(0,0,0,0.04))" }}>
                    {provider.protocolType}
                  </Tag>
                </div>

                <div className="cc-provider-card-footer">
                  <Text type="secondary" ellipsis style={{ maxWidth: 220, fontSize: 11 }}>
                    {provider.baseUrl}
                  </Text>
                  <div className="cc-provider-actions" style={{ display: "flex", alignItems: "center", gap: 6, minHeight: 28 }}>
                    {!isCatalogTarget && !isCurrent && (
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
                      <Tooltip title={t("providers.copyToAgent")}>
                        <Dropdown
                          trigger={["click"]}
                          menu={{
                            items: PROVIDER_TARGET_OPTIONS.filter((option) => option !== target).map((option) => ({
                              key: option,
                              label: t(LABEL_KEYS[option]),
                              disabled: !canCopyProviderTo(provider, option),
                              onClick: () => void afterCopyToTarget(provider, option),
                            })),
                          }}
                        >
                          <Button size="small" type="text" icon={<SwapOutlined />} />
                        </Dropdown>
                      </Tooltip>
                      <Tooltip title={t("deeplink.shareLink")}>
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

        {store.providers.length === 0 && (
          <div className="cc-provider-empty">
            <ResourceEmptyState
              title={t("providers.empty", { defaultValue: "暂无供应商" })}
              description={t("providers.emptyHint", { defaultValue: "为当前 Agent 添加第一个 Provider。" })}
              style={{ padding: "20px 16px" }}
              action={
                <Space>
                  <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
                    {t("providers.create")}
                  </Button>
                  {target === "opencode" && (
                    <Button icon={<ScanOutlined />} loading={busy} onClick={() => void handleImportLive()}>
                      {t("providers.syncOpenCodeLive")}
                    </Button>
                  )}
                </Space>
              }
            />
          </div>
        )}
      </div>

      {/* Form / Modals */}
      <ProviderForm
        open={formOpen}
        editing={editing}
        target={target}
        importHint={importHint}
        onCancel={() => {
          setFormOpen(false);
          setImportHint(null);
        }}
        onSubmit={handleSubmit}
      />

      <ImportFromAgentDialog
        open={importFromOpen}
        dest={target}
        confirming={busy}
        onCancel={() => setImportFromOpen(false)}
        onImport={(provider) => {
          setImportFromOpen(false);
          void afterCopyToTarget(provider, target);
        }}
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
      <Modal
        open={doctorModalOpen}
        title={t("providers.doctorReportTitle", { defaultValue: "供应商健康与诊断报告 (Provider Doctor)" })}
        width={720}
        onCancel={() => setDoctorModalOpen(false)}
        footer={[
          <Button key="close" onClick={() => setDoctorModalOpen(false)}>
            {t("common.close", { defaultValue: "关闭" })}
          </Button>,
          <Button
            key="quarantine"
            type="primary"
            danger
            loading={quarantining}
            disabled={!doctorReports.some((r) => !r.ok && (r.category === "authentication" || r.statusCode === 401 || r.statusCode === 403))}
            onClick={() => void handleQuarantineFailed()}
          >
            {t("providers.quarantineFailed", { defaultValue: "一键隔离 401/403 失效节点" })}
          </Button>,
        ]}
      >
        <Space direction="vertical" style={{ width: "100%" }}>
          <Alert
            type="info"
            showIcon
            message={t("providers.doctorTip", { defaultValue: "诊断测速每个供应商节点的连通性与 401/403 鉴权状态。隔离节点后将不会作为故障切换备选。" })}
          />
          {doctorReports.map((report) => (
            <Card key={report.providerId} size="small" style={{ marginBottom: 8 }}>
              <Space style={{ width: "100%", justifyContent: "space-between" }}>
                <Space>
                  <Tag color={report.ok ? "success" : report.statusCode === 401 || report.statusCode === 403 ? "error" : "warning"}>
                    {report.ok ? "健康 OK" : report.statusCode ? `HTTP ${report.statusCode}` : report.category}
                  </Tag>
                  <strong>{report.providerName}</strong>
                  <Tag>{report.targetApp}</Tag>
                  {report.quarantined && <Tag color="default">已隔离</Tag>}
                </Space>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {report.latencyMs ? `${report.latencyMs} ms` : "—"}
                </Text>
              </Space>
              <div style={{ fontSize: 12, color: report.ok ? "#52c41a" : "#ff4d4f", marginTop: 4 }}>
                {report.message}
              </div>
            </Card>
          ))}
        </Space>
      </Modal>
    </Space>
  );
}
