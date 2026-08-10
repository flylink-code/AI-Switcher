import { useEffect, useMemo, useState } from "react";
import {
  Alert,
  App,
  Button,
  Card,
  Empty,
  Modal,
  Space,
  Tag,
  Typography,
} from "antd";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import GlobalOutlined from "@ant-design/icons/es/icons/GlobalOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import { useTranslation } from "react-i18next";
import type { CodexOauthDeviceStart, Provider } from "@/types/backend";
import { useProvidersStore } from "@/stores/providersStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { ProviderForm } from "@/components/ProviderForm";
import { ImportPreviewDialog } from "@/components/ImportPreviewDialog";
import { OnboardingTip } from "@/components/OnboardingTip";
import { ProviderCard, ProviderToolbar } from "@/components/providers";
import { Stack, Inline, Surface } from "@/components/ui";
import { errMsg, useProviderActions } from "@/lib/useProviderActions";
import {
  ensureCodexOauthProvider,
  getCodexAuthStatus,
  getPaths,
  pollCodexOauthLogin,
  startCodexOauthLogin,
} from "@/services/api";

const { Text } = Typography;

export default function ProvidersPage() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const store = useProvidersStore();
  const target = usePagePreferencesStore((state) => state.providersTarget);

  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Provider | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<string>("all");

  const [codexAuth, setCodexAuth] = useState<{ loggedIn: boolean; loginCommand: string } | null>(null);
  const [oauthDevice, setOauthDevice] = useState<CodexOauthDeviceStart | null>(null);
  const [oauthPolling, setOauthPolling] = useState(false);

  const {
    busy,
    setBusy,
    importPreview,
    importConfirming,
    setImportPreview,
    handleSubmit,
    handleSwitch,
    handleOfficial,
    handleTest,
    handleSpeedtest,
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

  // Filter providers according to search query and status filter
  const filteredProviders = useMemo(() => {
    return store.providers.filter((p) => {
      // Status filter
      if (statusFilter === "healthy" && p.healthStatus !== "healthy") return false;
      if (statusFilter === "unhealthy" && p.healthStatus === "healthy") return false;

      // Search query
      if (searchQuery.trim()) {
        const query = searchQuery.toLowerCase().trim();
        const matchName = p.name.toLowerCase().includes(query);
        const matchUrl = p.baseUrl.toLowerCase().includes(query);
        const matchModel = p.model.toLowerCase().includes(query);
        return matchName || matchUrl || matchModel;
      }

      return true;
    });
  }, [store.providers, searchQuery, statusFilter]);

  return (
    <Stack gap="md" style={{ width: "100%", minWidth: 0 }}>
      {store.error && (
        <Alert
          type="error"
          showIcon
          message={store.error}
          closable
          onClose={() => store.clearError()}
        />
      )}

      {/* Toolbar */}
      <ProviderToolbar
        target={target}
        searchQuery={searchQuery}
        onSearchChange={setSearchQuery}
        statusFilter={statusFilter}
        onStatusFilterChange={setStatusFilter}
        busy={busy}
        oauthPolling={oauthPolling}
        onCodexOauthLogin={() => void handleCodexOauthLogin()}
        onImportLive={() => void handleImportLive()}
        onOpenOpencodeConfig={() => void handleOpenOpencodeConfig()}
        onExport={() => void handleExport()}
        onImportClipboard={() => void handleImportClipboard()}
        onImportFile={(file) => void handleImportFile(file)}
        onCreate={openCreate}
      />

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
        <OnboardingTip
          tipKey="providers_opencode_multi"
          type="info"
          message={t("providers.opencodeNoSwitchTitle")}
          description={t("providers.opencodeNoSwitchDescription")}
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

      {/* Official Mode Header Card */}
      {target !== "opencode" && (
        <Surface variant={officialCurrent ? "elevated" : "default"} padding="sm" style={{ borderColor: officialCurrent ? "var(--color-brand)" : undefined }}>
          <Inline justify="space-between" align="center">
            <Inline gap="sm">
              <GlobalOutlined style={{ color: "var(--color-brand)" }} />
              <Text strong>{t("providers.officialMode")}</Text>
              {officialCurrent && <Tag color="green">{t("providers.current")}</Tag>}
              <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
                {t("providers.officialModeDescription")}
              </Text>
            </Inline>

            <Button
              size="small"
              type={officialCurrent ? "default" : "primary"}
              icon={<ThunderboltOutlined />}
              loading={busy}
              disabled={officialCurrent}
              onClick={() => void handleOfficial()}
            >
              {officialCurrent ? t("providers.current") : t("providers.switch")}
            </Button>
          </Inline>
        </Surface>
      )}

      {/* Providers Compact Card Grid */}
      {filteredProviders.length > 0 ? (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fill, minmax(360px, 1fr))",
            gap: "var(--card-gap)",
          }}
        >
          {filteredProviders.map((provider, index) => (
            <ProviderCard
              key={provider.id}
              provider={provider}
              index={index}
              totalCount={filteredProviders.length}
              busy={busy}
              onSwitch={(p) => void handleSwitch(p)}
              onEdit={openEdit}
              onDelete={handleDelete}
              onTest={(p) => void handleTest(p)}
              onSpeedtest={(p) => void handleSpeedtest(p)}
              onShareLink={(p) => void handleShareLink(p)}
              onMove={(id, dir) => void store.move(id, dir)}
            />
          ))}
        </div>
      ) : (
        <Card size="small" className="page-surface" style={{ textAlign: "center", padding: "var(--space-8)" }}>
          <Empty
            description={
              searchQuery || statusFilter !== "all"
                ? t("providers.noMatchingProviders", { defaultValue: "没有符合过滤条件的供应商" })
                : t("providers.empty", { defaultValue: "暂无配置的供应商" })
            }
          >
            <Button type="primary" size="small" onClick={openCreate}>
              {t("providers.create")}
            </Button>
          </Empty>
        </Card>
      )}

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
    </Stack>
  );
}
