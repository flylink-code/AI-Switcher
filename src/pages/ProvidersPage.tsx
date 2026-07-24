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
} from "@ant-design/icons";
import { useTranslation } from "react-i18next";
import type { Provider, ProviderInput, ProviderTarget } from "@/types/backend";
import { useProvidersStore } from "@/stores/providersStore";
import { ProviderForm } from "@/components/ProviderForm";

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
    if (!provider.apiKey.trim()) {
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
    { title: t("providers.colName"), dataIndex: "name", render: (_: string, row) => <Space><Text strong>{row.name}</Text>{row.isCurrent && <Tag color="green">{t("providers.current")}</Tag>}</Space> },
    { title: t("providers.colBaseUrl"), dataIndex: "baseUrl", ellipsis: true, render: (value: string) => <Text code copyable style={{ wordBreak: "break-all" }}>{value}</Text> },
    { title: t("providers.colModel"), dataIndex: "model", ellipsis: true },
    { title: t("providers.colProtocol"), dataIndex: "protocolType", width: 110, render: (value: string) => <Tag color={value === "proxy" ? "orange" : "blue"}>{value}</Tag> },
    {
      title: t("providers.colActions"), key: "actions", width: 240,
      render: (_: unknown, row: Provider, index: number) => <Space size="small">
        <Tooltip title={t("providers.moveUp")}><Button size="small" icon={<ArrowUpOutlined />} disabled={index === 0 || busy} onClick={() => void store.move(row.id, -1)} /></Tooltip>
        <Tooltip title={t("providers.moveDown")}><Button size="small" icon={<ArrowDownOutlined />} disabled={index === store.providers.length - 1 || busy} onClick={() => void store.move(row.id, 1)} /></Tooltip>
        <Button size="small" type={row.isCurrent ? "default" : "primary"} disabled={row.isCurrent || busy} icon={<ThunderboltOutlined />} onClick={() => void handleSwitch(row)}>{t("providers.switch")}</Button>
        <Tooltip title={t("providers.edit")}><Button size="small" icon={<EditOutlined />} disabled={busy} onClick={() => openEdit(row)} /></Tooltip>
        <Popconfirm title={t("providers.confirmDelete")} okText={t("providers.delete")} cancelText={t("providers.cancel")} onConfirm={() => void handleDelete(row)} disabled={busy}>
          <Tooltip title={t("providers.delete")}><Button size="small" danger icon={<DeleteOutlined />} disabled={busy} /></Tooltip>
        </Popconfirm>
      </Space>,
    },
  ];

  return <Space direction="vertical" size="middle" style={{ width: "100%" }}>
    {store.error && <Alert type="error" showIcon message={store.error} closable onClose={() => store.clearError()} />}
    <Segmented<ProviderTarget>
      value={target}
      onChange={setTarget}
      options={[
        { value: "claude_code", label: t("providers.claudeCode") },
        { value: "claude_desktop", label: t("providers.claudeDesktop") },
      ]}
    />
    <Card
      size="small"
      styles={{ body: { padding: 12 } }}
      title={<Space><GlobalOutlined />{t("providers.title")}<Text type="secondary" style={{ fontWeight: "normal", fontSize: 12 }}>{t(target === "claude_code" ? "providers.codeSubtitle" : "providers.desktopSubtitle")}</Text></Space>}
      extra={<Space>
        <Button loading={busy} onClick={() => void handleOfficial()}>{t("providers.officialLogin")}</Button>
        <Button icon={<ImportOutlined />} loading={busy} onClick={() => void handleImport()}>{t("providers.importLive")}</Button>
        <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>{t("providers.create")}</Button>
      </Space>}
    >
      <Table<Provider> rowKey="id" size="middle" loading={store.loading} dataSource={store.providers} columns={columns} pagination={false} locale={{ emptyText: t("providers.empty") }} />
    </Card>
    <ProviderForm open={formOpen} editing={editing} onCancel={() => setFormOpen(false)} onSubmit={handleSubmit} />
  </Space>;
}

function errMsg(e: unknown): string { return e instanceof Error ? e.message : String(e); }
