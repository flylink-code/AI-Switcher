import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  App,
  Button,
  Card,
  Checkbox,
  Form,
  Input,
  Modal,
  Popconfirm,
  Space,
  Spin,
  Switch,
  Table,
  Tooltip,
  Typography,
  type TableColumnsType,
} from "antd";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import EditOutlined from "@ant-design/icons/es/icons/EditOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import SaveOutlined from "@ant-design/icons/es/icons/SaveOutlined";
import SyncOutlined from "@ant-design/icons/es/icons/SyncOutlined";
import { useTranslation } from "react-i18next";
import type { McpServer, McpServerInput, McpTarget } from "@/types/backend";
import {
  deleteMcpServer,
  importMcpServers,
  listMcpServers,
  saveMcpServer,
  toggleMcpServer,
} from "@/services/api";

const { Text, Paragraph } = Typography;

interface FormValues {
  name: string;
  serverConfig: string;
  enabledClaudeCode: boolean;
  enabledClaudeDesktop: boolean;
}

const EXAMPLE_CONFIG = `{
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/allow"]
}`;

export default function McpPage() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [servers, setServers] = useState<McpServer[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<McpServer | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [form] = Form.useForm<FormValues>();

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setServers(await listMcpServers());
      setError(null);
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const openCreate = () => {
    setEditing(null);
    form.setFieldsValue({
      name: "",
      serverConfig: EXAMPLE_CONFIG,
      enabledClaudeCode: true,
      enabledClaudeDesktop: false,
    });
    setFormOpen(true);
  };

  const openEdit = (server: McpServer) => {
    setEditing(server);
    form.setFieldsValue({
      name: server.name,
      serverConfig: JSON.stringify(server.serverConfig, null, 2),
      enabledClaudeCode: server.enabledClaudeCode,
      enabledClaudeDesktop: server.enabledClaudeDesktop,
    });
    setFormOpen(true);
  };

  const handleSave = async (values: FormValues) => {
    let serverConfig: Record<string, unknown>;
    try {
      const parsed: unknown = JSON.parse(values.serverConfig);
      if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
        throw new Error(t("mcp.invalidConfig"));
      }
      serverConfig = parsed as Record<string, unknown>;
    } catch (e) {
      form.setFields([{ name: "serverConfig", errors: [errMsg(e)] }]);
      return;
    }

    setBusy(true);
    try {
      const input: McpServerInput = {
        id: editing?.id,
        name: values.name.trim(),
        serverConfig,
        enabledClaudeCode: values.enabledClaudeCode,
        enabledClaudeDesktop: values.enabledClaudeDesktop,
      };
      await saveMcpServer(input);
      void message.success(t(editing ? "mcp.updated" : "mcp.created"));
      setFormOpen(false);
      await load();
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleToggle = async (server: McpServer, target: McpTarget, enabled: boolean) => {
    setBusy(true);
    try {
      await toggleMcpServer(server.id, target, enabled);
      setServers((current) =>
        current.map((item) =>
          item.id === server.id
            ? {
                ...item,
                enabledClaudeCode:
                  target === "claude_code" ? enabled : item.enabledClaudeCode,
                enabledClaudeDesktop:
                  target === "claude_desktop" ? enabled : item.enabledClaudeDesktop,
              }
            : item,
        ),
      );
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async (server: McpServer) => {
    setBusy(true);
    try {
      await deleteMcpServer(server.id);
      void message.success(t("mcp.deleted"));
      await load();
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleImport = async () => {
    setBusy(true);
    try {
      const summary = await importMcpServers();
      void message.success(
        t("mcp.imported", { imported: summary.imported, updated: summary.updated }),
      );
      await load();
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const columns: TableColumnsType<McpServer> = [
    {
      title: t("mcp.colName"),
      dataIndex: "name",
      render: (value: string) => <Text strong>{value}</Text>,
    },
    {
      title: t("mcp.colConfig"),
      dataIndex: "serverConfig",
      ellipsis: true,
      render: (config: Record<string, unknown>) => (
        <Text code style={{ wordBreak: "break-all" }}>
          {config.command ? String(config.command) : config.url ? String(config.url) : "JSON"}
        </Text>
      ),
    },
    {
      title: t("mcp.claudeCode"),
      dataIndex: "enabledClaudeCode",
      width: 135,
      render: (enabled: boolean, server) => (
        <Switch
          size="small"
          checked={enabled}
          disabled={busy}
          checkedChildren={t("common.enabled")}
          unCheckedChildren={t("common.disabled")}
          onChange={(value) => void handleToggle(server, "claude_code", value)}
        />
      ),
    },
    {
      title: t("mcp.claudeDesktop"),
      dataIndex: "enabledClaudeDesktop",
      width: 145,
      render: (enabled: boolean, server) => (
        <Switch
          size="small"
          checked={enabled}
          disabled={busy}
          checkedChildren={t("common.enabled")}
          unCheckedChildren={t("common.disabled")}
          onChange={(value) => void handleToggle(server, "claude_desktop", value)}
        />
      ),
    },
    {
      title: t("mcp.colActions"),
      key: "actions",
      width: 120,
      render: (_, server) => (
        <Space size="small">
          <Tooltip title={t("mcp.edit")}>
            <Button size="small" icon={<EditOutlined />} disabled={busy} onClick={() => openEdit(server)} />
          </Tooltip>
          <Popconfirm
            title={t("mcp.confirmDelete")}
            okText={t("mcp.delete")}
            cancelText={t("common.cancel")}
            onConfirm={() => void handleDelete(server)}
            disabled={busy}
          >
            <Tooltip title={t("mcp.delete")}>
              <Button size="small" danger icon={<DeleteOutlined />} disabled={busy} />
            </Tooltip>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <Spin spinning={loading}>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        {error && <Alert type="error" showIcon closable message={error} onClose={() => setError(null)} />}
        <Alert type="info" showIcon message={t("mcp.title")} description={t("mcp.description")} />
        <Card
          size="small"
          title={t("mcp.title")}
          extra={
            <Space>
              <Button icon={<ImportOutlined />} loading={busy} onClick={() => void handleImport()}>
                {t("mcp.import")}
              </Button>
              <Button icon={<SyncOutlined />} disabled={busy} onClick={() => void load()}>
                {t("common.refresh")}
              </Button>
              <Button type="primary" icon={<PlusOutlined />} disabled={busy} onClick={openCreate}>
                {t("mcp.create")}
              </Button>
            </Space>
          }
        >
          <Table<McpServer>
            rowKey="id"
            columns={columns}
            dataSource={servers}
            pagination={false}
            locale={{ emptyText: t("mcp.empty") }}
          />
        </Card>
      </Space>

      <Modal
        title={t(editing ? "mcp.editTitle" : "mcp.createTitle")}
        open={formOpen}
        onCancel={() => setFormOpen(false)}
        confirmLoading={busy}
        okText={t("mcp.save")}
        cancelText={t("common.cancel")}
        onOk={() => void form.submit()}
        width={720}
      >
        <Form form={form} layout="vertical" onFinish={handleSave} initialValues={{ enabledClaudeCode: true }}>
          <Form.Item name="name" label={t("mcp.fieldName")} rules={[{ required: true, message: t("mcp.requiredName") }]}>
            <Input autoFocus disabled={busy} />
          </Form.Item>
          <Form.Item
            name="serverConfig"
            label={t("mcp.fieldConfig")}
            extra={t("mcp.configHelp")}
            rules={[{ required: true, message: t("mcp.requiredConfig") }]}
          >
            <Input.TextArea rows={11} spellCheck={false} disabled={busy} style={{ fontFamily: "monospace" }} />
          </Form.Item>
          <Form.Item>
            <Space>
              <Form.Item name="enabledClaudeCode" valuePropName="checked" noStyle>
                <Checkbox disabled={busy}>{t("mcp.enableCode")}</Checkbox>
              </Form.Item>
              <Form.Item name="enabledClaudeDesktop" valuePropName="checked" noStyle>
                <Checkbox disabled={busy}>{t("mcp.enableDesktop")}</Checkbox>
              </Form.Item>
            </Space>
          </Form.Item>
          <Paragraph type="secondary" style={{ marginBottom: 0 }}>
            <SaveOutlined /> {t("mcp.syncNote")}
          </Paragraph>
        </Form>
      </Modal>
    </Spin>
  );
}

function errMsg(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
