import { useEffect, useState } from "react";
import {
  Alert,
  App,
  Button,
  Card,
  Popconfirm,
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
import type { Provider, ProviderInput } from "@/types/backend";
import { useProvidersStore } from "@/stores/providersStore";
import { ProviderForm } from "@/components/ProviderForm";

const { Text } = Typography;

export default function ProvidersPage() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const store = useProvidersStore();
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Provider | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void store.load();
    void store.loadPresets();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const openCreate = () => {
    setEditing(null);
    setFormOpen(true);
  };

  const openEdit = (provider: Provider) => {
    setEditing(provider);
    setFormOpen(true);
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
    } finally {
      setBusy(false);
    }
  };

  const handleSwitch = async (provider: Provider) => {
    // Official-login preset: empty base_url — route to clear mode.
    if (provider.baseUrl.trim() === "") {
      setBusy(true);
      try {
        await store.useOfficial();
        void message.success(t("providers.switchedOfficial"));
      } catch (e) {
        void message.error(errMsg(e));
      } finally {
        setBusy(false);
      }
      return;
    }
    // Validate that a non-official provider has a token before switching.
    if (provider.apiKey.trim() === "") {
      void message.warning(t("providers.missingKey"));
      return;
    }
    setBusy(true);
    try {
      await store.switchTo(provider.id);
      void message.success(t("providers.switched", { name: provider.name }));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async (provider: Provider) => {
    setBusy(true);
    try {
      await store.remove(provider.id);
      void message.success(t("providers.deleted"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleImport = async () => {
    setBusy(true);
    try {
      await store.importLive();
      void message.success(t("providers.imported"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const columns: TableColumnsType<Provider> = [
    {
      title: t("providers.colName"),
      dataIndex: "name",
      render: (_: string, r) => (
        <Space>
          <Text strong>{r.name}</Text>
          {r.isCurrent && <Tag color="green">{t("providers.current")}</Tag>}
        </Space>
      ),
    },
    {
      title: t("providers.colBaseUrl"),
      dataIndex: "baseUrl",
      ellipsis: true,
      render: (v: string) =>
        v ? (
          <Text code copyable style={{ wordBreak: "break-all" }}>{v}</Text>
        ) : (
          <Tag>{t("providers.officialLogin")}</Tag>
        ),
    },
    { title: t("providers.colModel"), dataIndex: "model", ellipsis: true },
    {
      title: t("providers.colProtocol"),
      dataIndex: "protocolType",
      width: 110,
      render: (v: string) =>
        v === "proxy" ? (
          <Tag color="orange">proxy</Tag>
        ) : (
          <Tag color="blue">anthropic</Tag>
        ),
    },
    {
      title: t("providers.colActions"),
      key: "actions",
      width: 240,
      render: (_: unknown, r: Provider, index: number) => (
        <Space size="small">
          <Tooltip title={t("providers.moveUp")}>
            <Button
              size="small"
              icon={<ArrowUpOutlined />}
              disabled={index === 0 || busy}
              onClick={() => store.move(r.id, -1)}
            />
          </Tooltip>
          <Tooltip title={t("providers.moveDown")}>
            <Button
              size="small"
              icon={<ArrowDownOutlined />}
              disabled={index === store.providers.length - 1 || busy}
              onClick={() => store.move(r.id, 1)}
            />
          </Tooltip>
          <Button
            size="small"
            type={r.isCurrent ? "default" : "primary"}
            disabled={r.isCurrent || busy}
            icon={<ThunderboltOutlined />}
            onClick={() => handleSwitch(r)}
          >
            {t("providers.switch")}
          </Button>
          <Tooltip title={t("providers.edit")}>
            <Button size="small" icon={<EditOutlined />} disabled={busy} onClick={() => openEdit(r)} />
          </Tooltip>
          <Popconfirm
            title={t("providers.confirmDelete")}
            okText={t("providers.delete")}
            cancelText={t("providers.cancel")}
            onConfirm={() => handleDelete(r)}
            disabled={busy}
          >
            <Tooltip title={t("providers.delete")}>
              <Button size="small" danger icon={<DeleteOutlined />} disabled={busy} />
            </Tooltip>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      {store.error && (
        <Alert
          type="error"
          showIcon
          message={store.error}
          closable
          onClose={() => store.clearError()}
        />
      )}

      <Card
        size="small"
        styles={{ body: { padding: 12 } }}
        extra={
          <Space>
            <Button icon={<ImportOutlined />} loading={busy} onClick={handleImport}>
              {t("providers.importLive")}
            </Button>
            <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
              {t("providers.create")}
            </Button>
          </Space>
        }
        title={
          <Space>
            <GlobalOutlined />
            {t("providers.title")}
            <Text type="secondary" style={{ fontWeight: "normal", fontSize: 12 }}>
              {t("providers.subtitle")}
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
        />
      </Card>

      <ProviderForm
        open={formOpen}
        editing={editing}
        presets={store.presets}
        onCancel={() => setFormOpen(false)}
        onSubmit={handleSubmit}
      />
    </Space>
  );
}

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
