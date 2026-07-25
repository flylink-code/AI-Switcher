import { useEffect, useState } from "react";
import {
  Alert,
  App,
  Button,
  Card,
  Popconfirm,
  Segmented,
  Space,
  Table,
  Tag,
  Tooltip,
  Typography,
  type TableColumnsType,
} from "antd";
import {
  PlusOutlined,
  ImportOutlined,
  ArrowUpOutlined,
  ArrowDownOutlined,
  EditOutlined,
  DeleteOutlined,
  ThunderboltOutlined,
  GlobalOutlined,
  SafetyCertificateOutlined,
} from "@ant-design/icons";
import { useTranslation } from "react-i18next";
import type { Provider, ProviderInput, ProviderTarget } from "@/types/backend";
import { useProvidersStore } from "@/stores/providersStore";
import { ProviderForm } from "@/components/ProviderForm";
import { exportProviders, importProvidersJson, testProviderConnection } from "@/services/api";

const { Text } = Typography;

export default function ProvidersPage() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const store = useProvidersStore();
  const [target, setTarget] = useState<ProviderTarget>("claude_code");
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Provider | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => { void store.load(target); }, [store.load, target]);

  const openCreate = () => { setEditing(null); setFormOpen(true); };
  const openEdit = (provider: Provider) => { setEditing(provider); setFormOpen(true); };

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
      await store.switchTo(provider.id);
      void message.success(t("providers.switched", { name: provider.name }));
    } catch (e) {
      void message.error(errMsg(e));
    } finally { setBusy(false); }
  };

  const handleTest = async (provider: Provider) => {
    setBusy(true);
    try {
      const result = await testProviderConnection(provider.id);
      const notify = result.ok ? message.success : message.error;
      void notify(result.message);
      await store.load(target);
    } catch (e) {
      void message.error(errMsg(e));
    } finally { setBusy(false); }
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
      const result = await importProvidersJson(await file.text());
      void message.success(t("providers.importSummary", { imported: result.imported, skipped: result.skipped }));
      await store.load(target);
    } catch (e) { void message.error(errMsg(e)); }
    finally { setBusy(false); }
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
    { title: t("providers.colName"), dataIndex: "name", render: (_: string, row) => <Space><Text strong>{row.name}</Text>{row.isCurrent && <Tag color="green">{t("providers.current")}</Tag>}{row.healthStatus && <Tag color={row.healthStatus === "healthy" ? "green" : "red"}>{row.healthStatus === "healthy" ? t("providers.healthy") : t("providers.unhealthy")}</Tag>}</Space> },
    { title: t("providers.colBaseUrl"), dataIndex: "baseUrl", width: 280, ellipsis: true, render: (value: string) => <Text code copyable ellipsis={{ tooltip: value }}>{value}</Text> },
    { title: t("providers.colModel"), dataIndex: "model", width: 160, ellipsis: true },
    { title: t("providers.colProtocol"), dataIndex: "protocolType", width: 150, render: (value: string) => <Tag color={value === "anthropic" ? "blue" : "orange"}>{value}</Tag> },
    {
      title: t("providers.colActions"), key: "actions", width: 240,
      render: (_: unknown, row: Provider, index: number) => <Space size="small">
        <Tooltip title={t("providers.moveUp")}><Button size="small" icon={<ArrowUpOutlined />} disabled={index === 0 || busy} onClick={() => void store.move(row.id, -1)} /></Tooltip>
        <Tooltip title={t("providers.moveDown")}><Button size="small" icon={<ArrowDownOutlined />} disabled={index === store.providers.length - 1 || busy} onClick={() => void store.move(row.id, 1)} /></Tooltip>
        <Button size="small" type={row.isCurrent ? "default" : "primary"} disabled={row.isCurrent || busy} icon={<ThunderboltOutlined />} onClick={() => void handleSwitch(row)}>{t("providers.switch")}</Button>
        <Tooltip title={t("providers.testConnection")}><Button size="small" icon={<SafetyCertificateOutlined />} disabled={busy || !row.apiKeySet} onClick={() => void handleTest(row)} /></Tooltip>
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
        ]}
      />
      <Space wrap size={[8, 8]}>
        <Button loading={busy} onClick={() => void handleOfficial()}>{t("providers.officialLogin")}</Button>
        <Button icon={<ImportOutlined />} loading={busy} onClick={() => void handleImport()}>{t("providers.importLive")}</Button>
        <Button loading={busy} onClick={() => void handleExport()}>{t("providers.export")}</Button>
        <label><Button loading={busy}>{t("providers.importFile")}</Button><input type="file" accept="application/json" hidden onChange={(event) => { const file = event.target.files?.[0]; if (file) void handleImportFile(file); event.currentTarget.value = ""; }} /></label>
        <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>{t("providers.create")}</Button>
      </Space>
    </Space>
    <Card
      size="small"
      styles={{ body: { padding: 12 } }}
      title={
        <Space wrap>
          <GlobalOutlined />
          {t("providers.title")}
          <Text type="secondary" style={{ fontWeight: "normal", fontSize: 12 }}>
            {t(target === "claude_code" ? "providers.codeSubtitle" : "providers.desktopSubtitle")}
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
        scroll={{ x: 1050 }}
        locale={{ emptyText: t("providers.empty") }}
      />
    </Card>
    <ProviderForm open={formOpen} editing={editing} onCancel={() => setFormOpen(false)} onSubmit={handleSubmit} />
  </Space>;
}

function errMsg(e: unknown): string { return e instanceof Error ? e.message : String(e); }
