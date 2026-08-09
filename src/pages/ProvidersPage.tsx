import { useEffect, useState } from "react";
import {
  Alert,
  App,
  Button,
  Card,
  Dropdown,
  Modal,
  Space,
  Table,
  Tag,
  Tooltip,
  Typography,
  type MenuProps,
  type TableColumnsType,
} from "antd";
import { openUrl } from "@tauri-apps/plugin-opener";
import ArrowDownOutlined from "@ant-design/icons/es/icons/ArrowDownOutlined";
import ArrowUpOutlined from "@ant-design/icons/es/icons/ArrowUpOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import EditOutlined from "@ant-design/icons/es/icons/EditOutlined";
import EllipsisOutlined from "@ant-design/icons/es/icons/EllipsisOutlined";
import GlobalOutlined from "@ant-design/icons/es/icons/GlobalOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import SafetyCertificateOutlined from "@ant-design/icons/es/icons/SafetyCertificateOutlined";
import FieldTimeOutlined from "@ant-design/icons/es/icons/FieldTimeOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import { useTranslation } from "react-i18next";
import type { CodexOauthDeviceStart, Provider } from "@/types/backend";
import { useProvidersStore } from "@/stores/providersStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { ProviderForm } from "@/components/ProviderForm";
import { ImportPreviewDialog } from "@/components/ImportPreviewDialog";
import { OnboardingTip } from "@/components/OnboardingTip";
import { WorkspaceTargetSegmented } from "@/components/WorkspaceTargetSegmented";
import { errMsg, useProviderActions } from "@/lib/useProviderActions";
import {
  ensureCodexOauthProvider,
  getCodexAuthStatus,
  pollCodexOauthLogin,
  startCodexOauthLogin,
} from "@/services/api";

const { Text } = Typography;

export default function ProvidersPage() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const store = useProvidersStore();
  const target = usePagePreferencesStore((state) => state.providersTarget);
  const setProvidersTarget = usePagePreferencesStore((state) => state.setProvidersTarget);
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Provider | null>(null);
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

  const columns: TableColumnsType<Provider> = [
    { title: t("providers.colName"), dataIndex: "name", render: (_: string, row) => (
      <Space>
        <Text strong>{row.name}</Text>
        {target !== "opencode" && row.isCurrent && <Tag color="green">{t("providers.current")}</Tag>}
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
        if (row.targetApp === "codex" || row.targetApp === "opencode") {
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
      title: t("providers.colActions"), key: "actions", width: 200,
      render: (_: unknown, row: Provider, index: number) => {
        const moreItems: MenuProps["items"] = [
          {
            key: "up",
            icon: <ArrowUpOutlined />,
            label: t("providers.moveUp"),
            disabled: index === 0 || busy,
            onClick: () => void store.move(row.id, -1),
          },
          {
            key: "down",
            icon: <ArrowDownOutlined />,
            label: t("providers.moveDown"),
            disabled: index === store.providers.length - 1 || busy,
            onClick: () => void store.move(row.id, 1),
          },
          {
            key: "test",
            icon: <SafetyCertificateOutlined />,
            label: t("providers.testConnection"),
            disabled: busy || !row.apiKeySet,
            onClick: () => void handleTest(row),
          },
          {
            key: "speed",
            icon: <FieldTimeOutlined />,
            label: t("providers.speedtest"),
            disabled: busy || !row.baseUrl,
            onClick: () => void handleSpeedtest(row),
          },
          {
            key: "share",
            icon: <GlobalOutlined />,
            label: t("deeplink.shareLink"),
            disabled: busy,
            onClick: () => void handleShareLink(row),
          },
          { type: "divider" },
          {
            key: "delete",
            icon: <DeleteOutlined />,
            danger: true,
            label: t("providers.delete"),
            disabled: busy,
            onClick: () => {
              Modal.confirm({
                title: t("providers.confirmDelete"),
                okText: t("providers.delete"),
                cancelText: t("providers.cancel"),
                okButtonProps: { danger: true },
                onOk: () => handleDelete(row),
              });
            },
          },
        ];
        return (
          <Space size="small">
            {target !== "opencode" && (
              <Button
                size="small"
                type={row.isCurrent ? "default" : "primary"}
                disabled={row.isCurrent || busy}
                icon={<ThunderboltOutlined />}
                onClick={() => void handleSwitch(row)}
              >
                {t("providers.switch")}
              </Button>
            )}
            <Tooltip title={t("providers.edit")}>
              <Button size="small" icon={<EditOutlined />} disabled={busy} onClick={() => openEdit(row)} />
            </Tooltip>
            <Dropdown menu={{ items: moreItems }} trigger={["click"]}>
              <Button size="small" icon={<EllipsisOutlined />} disabled={busy} aria-label={t("providers.moreActions")} />
            </Dropdown>
          </Space>
        );
      },
    },
  ];

  return <Space direction="vertical" size="middle" style={{ width: "100%", minWidth: 0 }}>
    {store.error && <Alert type="error" showIcon message={store.error} closable onClose={() => store.clearError()} />}
    <div className="providers-toolbar">
      <WorkspaceTargetSegmented
        value={target}
        onChange={setProvidersTarget}
        t={t}
        ariaLabel={t("workspace.target")}
      />
      <Space wrap size={8} align="center">
        <div className="providers-action-cluster">
          {target !== "codex" && target !== "opencode" && (
            <Button type="text" size="small" loading={oauthPolling} onClick={() => void handleCodexOauthLogin()}>
              {t("providers.chatgptLogin")}
            </Button>
          )}
          <Button type="text" size="small" icon={<ImportOutlined />} loading={busy} onClick={() => void handleImportLive()}>
            {target === "opencode" ? t("providers.syncOpenCodeLive") : t("providers.importLive")}
          </Button>
          <Button type="text" size="small" loading={busy} onClick={() => void handleExport()}>
            {t("providers.export")}
          </Button>
          <Button type="text" size="small" loading={busy} onClick={() => void handleImportClipboard()}>
            {t("providers.importClipboard")}
          </Button>
          <label>
            <Button type="text" size="small" loading={busy}>{t("providers.importFile")}</Button>
            <input
              type="file"
              accept="application/json"
              hidden
              onChange={(event) => {
                const file = event.target.files?.[0];
                if (file) void handleImportFile(file);
                event.currentTarget.value = "";
              }}
            />
          </label>
        </div>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => openCreate()}>
          {t("providers.create")}
        </Button>
      </Space>
    </div>
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
              <Button size="small" onClick={() => void navigator.clipboard?.writeText(codexAuth?.loginCommand ?? "codex login")}>
                复制命令
              </Button>
            </Space>
            <Text type="secondary">{t("providers.codexLoginHint")}</Text>
          </Space>
        }
      />
    )}
    {target !== "opencode" && (
    <Card
      size="small"
      className="page-surface"
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
    )}
    <Card
      size="small"
      className="page-surface"
      styles={{ body: { padding: 12 } }}
      title={
        <Space wrap>
          <GlobalOutlined />
          {t("providers.title")}
          <Text type="secondary" style={{ fontWeight: "normal", fontSize: 12 }}>
            {target === "claude_code"
              ? t("providers.codeSubtitle")
              : target === "claude_desktop"
                ? t("providers.desktopSubtitle")
                : target === "opencode"
                  ? t("providers.opencodeSubtitle")
                  : "管理 ~/.codex/config.toml 中的直连模型提供方"}
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
        rowClassName={(row) => (row.isCurrent ? "provider-row-current" : "")}
        locale={{ emptyText: t("providers.empty") }}
      />
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
