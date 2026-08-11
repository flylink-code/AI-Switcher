import { useEffect, useRef, useState } from "react";
import {
  Alert,
  App,
  Badge,
  Button,
  Dropdown,
  Empty,
  Modal,
  Popconfirm,
  Space,
  Tag,
  Tooltip,
  Typography,
  type MenuProps,
} from "antd";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import ExportOutlined from "@ant-design/icons/es/icons/ExportOutlined";
import FolderOpenOutlined from "@ant-design/icons/es/icons/FolderOpenOutlined";
import LoginOutlined from "@ant-design/icons/es/icons/LoginOutlined";
import EditOutlined from "@ant-design/icons/es/icons/EditOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import NodeIndexOutlined from "@ant-design/icons/es/icons/NodeIndexOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { CodexOauthDeviceStart, Provider } from "@/types/backend";
import { useProvidersStore } from "@/stores/providersStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { ProviderForm } from "@/components/ProviderForm";
import { ImportPreviewDialog } from "@/components/ImportPreviewDialog";
import { OnboardingTip } from "@/components/OnboardingTip";
import { ProviderBrandIcon } from "@/components/ProviderBrandIcon";
import { AgentTargetSwitcher } from "@/components/AgentTargetSwitcher";
import { usageSourceIcon } from "@/components/UsageSourceIcons";
import { managedAppsRuntimeStatusOptions, proxyStatusOptions } from "@/lib/appQueries";
import { useNavigatePage } from "@/lib/navigation";
import { errMsg, useProviderActions } from "@/lib/useProviderActions";
import {
  ensureCodexOauthProvider,
  getAntigravityGatewayStatus,
  getCodexAuthStatus,
  getPaths,
  pollCodexOauthLogin,
  startCodexOauthLogin,
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

  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Provider | null>(null);

  const [codexAuth, setCodexAuth] = useState<{ loggedIn: boolean; loginCommand: string } | null>(null);
  const [oauthDevice, setOauthDevice] = useState<CodexOauthDeviceStart | null>(null);
  const [oauthPolling, setOauthPolling] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

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
  } = useProviderActions({ target, editing, closeForm: () => setFormOpen(false) });

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
    setFormOpen(true);
  };

  const openEdit = (provider: Provider) => {
    setEditing(provider);
    setFormOpen(true);
  };

  const handleOpenOpencodeConfig = async () => {
    try {
      const paths = await getPaths();
      await revealItemInDir(paths.opencodeConfigPath);
    } catch (error) {
      void message.error(errMsg(error));
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
    ...(target !== "codex" && target !== "opencode"
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
      key: "importLive",
      icon: <ImportOutlined />,
      label: target === "opencode" ? t("providers.syncOpenCodeLive") : t("providers.importLive"),
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
          <AgentTargetSwitcher value={target} onChange={setProvidersTarget} />
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
          <Button
            icon={<ThunderboltOutlined />}
            loading={batchTesting}
            onClick={() => void handleSpeedtestAll()}
          >
            {t("providers.speedtestAll")}
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
      {target !== "opencode" && (
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

                {/* Lightweight Metadata Row (No bulky inner-card) */}
                <div style={{ display: "flex", alignItems: "center", gap: "8px", flexWrap: "wrap", margin: "2px 0 4px 0" }}>
                  <Tag style={{ margin: 0, borderRadius: 4, fontSize: 11, background: "var(--color-bg-subtle, rgba(0,0,0,0.04))" }}>
                    Model: {provider.model || "Default"}
                  </Tag>
                  <Tag style={{ margin: 0, borderRadius: 4, fontSize: 11, background: "var(--color-bg-subtle, rgba(0,0,0,0.04))" }}>
                    {provider.protocolType}
                  </Tag>
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
          <div className="cc-provider-card" style={{ textAlign: "center", padding: "var(--space-8)" }}>
            <Empty description={t("providers.empty", { defaultValue: "暂无配置的供应商" })}>
              <Button type="primary" size="small" onClick={openCreate}>
                {t("providers.create")}
              </Button>
            </Empty>
          </div>
        )}
      </div>

      {/* Form / Modals */}
      <ProviderForm
        open={formOpen}
        editing={editing}
        target={target}
        onCancel={() => setFormOpen(false)}
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
    </Space>
  );
}
