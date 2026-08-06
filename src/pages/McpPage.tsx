import { useState } from "react";
import {
  Alert,
  App,
  Button,
  Card,
  Checkbox,
  Collapse,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Table,
  Tooltip,
  Typography,
  type TableColumnsType,
} from "antd";
import ArrowDownOutlined from "@ant-design/icons/es/icons/ArrowDownOutlined";
import ArrowUpOutlined from "@ant-design/icons/es/icons/ArrowUpOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import EditOutlined from "@ant-design/icons/es/icons/EditOutlined";
import GlobalOutlined from "@ant-design/icons/es/icons/GlobalOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import SaveOutlined from "@ant-design/icons/es/icons/SaveOutlined";
import SyncOutlined from "@ant-design/icons/es/icons/SyncOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { OnboardingTip } from "@/components/OnboardingTip";
import { ImportPreviewDialog } from "@/components/ImportPreviewDialog";
import type { ImportPreview, McpServer, McpServerInput, McpTarget, RegistryMcpServer } from "@/types/backend";
import {
  buildMcpDeeplink,
  clearMcpOauth,
  confirmImportPreview,
  deleteMcpServer,
  importMcpServers,
  installMcpRegistryServer,
  previewImportText,
  reorderMcpServers,
  saveMcpServer,
  searchMcpRegistry,
  toggleMcpServer,
} from "@/services/api";
import {
  mcpDesktopConflictOptions,
  mcpOauthStatusOptions,
  mcpServersOptions,
} from "@/lib/appQueries";

const { Text, Paragraph } = Typography;

interface KeyValueEntry {
  key: string;
  value: string;
}

interface FormValues {
  name: string;
  transport: "stdio" | "http" | "sse";
  command: string;
  url: string;
  args: string[];
  env: KeyValueEntry[];
  headers: KeyValueEntry[];
  serverConfig: string;
  enabledClaudeCode: boolean;
  enabledClaudeDesktop: boolean;
  enabledCodex: boolean;
}

const EXAMPLE_CONFIG: Record<string, unknown> = {
  command: "npx",
  args: ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/allow"],
};

type McpTransport = FormValues["transport"];

function detectTransport(config: Record<string, unknown>): McpTransport {
  const rawType = typeof config.type === "string" ? config.type.toLowerCase() : "";
  if (rawType === "http" || rawType === "streamable-http") return "http";
  if (rawType === "sse") return "sse";
  if (typeof config.url === "string" && config.url.trim()) return "http";
  return "stdio";
}

function usesMcpRemoteBridge(config: Record<string, unknown>): boolean {
  const command = typeof config.command === "string" ? config.command : "";
  const args = Array.isArray(config.args)
    ? config.args.filter((item): item is string => typeof item === "string")
    : [];
  return [command, ...args].some((part) => part.toLowerCase().includes("mcp-remote"));
}

function objectToEntries(value: unknown): KeyValueEntry[] {
  if (!value || typeof value !== "object" || Array.isArray(value)) return [];
  return Object.entries(value as Record<string, unknown>)
    .filter(([key]) => key.trim())
    .map(([key, val]) => ({ key, value: val == null ? "" : String(val) }));
}

function entriesToObject(entries: KeyValueEntry[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const entry of entries) {
    const key = entry.key.trim();
    if (!key) continue;
    out[key] = entry.value;
  }
  return out;
}

function parseConfigObject(
  config: Record<string, unknown>,
): Pick<FormValues, "transport" | "command" | "url" | "args" | "env" | "headers" | "serverConfig"> {
  const command = typeof config.command === "string" ? config.command : "";
  const url = typeof config.url === "string" ? config.url : "";
  const args = Array.isArray(config.args)
    ? config.args.filter((item): item is string => typeof item === "string")
    : [];
  const env = objectToEntries(config.env);
  const headers = objectToEntries(config.headers);
  return {
    transport: detectTransport(config),
    command,
    url,
    args: args.length ? args : [""],
    env,
    headers,
    serverConfig: JSON.stringify(config, null, 2),
  };
}

function mergeStructuredIntoConfig(
  baseJson: string,
  structured: Pick<FormValues, "transport" | "command" | "url" | "args" | "env" | "headers">,
): Record<string, unknown> {
  let base: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(baseJson);
    if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
      base = {};
    } else {
      base = { ...(parsed as Record<string, unknown>) };
    }
  } catch {
    base = {};
  }

  if (structured.transport === "http" || structured.transport === "sse") {
    base.type = structured.transport;
    const url = structured.url.trim();
    if (url) base.url = url;
    else delete base.url;
    // Remote transports don't use stdio command/args unless the user keeps them in advanced JSON.
    delete base.command;
    delete base.args;
  } else {
    delete base.type;
    const command = structured.command.trim();
    if (command) base.command = command;
    else delete base.command;

    const args = structured.args.map((item) => item.trim()).filter(Boolean);
    if (args.length) base.args = args;
    else delete base.args;

    // URL without type is invalid for Claude Code; drop stale remote fields on stdio.
    delete base.url;
  }

  const env = entriesToObject(structured.env);
  if (Object.keys(env).length) base.env = env;
  else delete base.env;

  const headers = entriesToObject(structured.headers);
  if (Object.keys(headers).length) base.headers = headers;
  else delete base.headers;

  return base;
}

function syncStructuredToJson(form: ReturnType<typeof Form.useForm<FormValues>>[0]) {
  const values = form.getFieldsValue();
  const merged = mergeStructuredIntoConfig(values.serverConfig ?? "{}", values);
  form.setFieldValue("serverConfig", JSON.stringify(merged, null, 2));
}

export default function McpPage() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const queryClient = useQueryClient();
  const serversQuery = useQuery(mcpServersOptions);
  const oauthQuery = useQuery(mcpOauthStatusOptions);
  const conflictQuery = useQuery(mcpDesktopConflictOptions);
  const servers = serversQuery.data ?? [];
  const [busy, setBusy] = useState(false);
  const [importPreview, setImportPreview] = useState<ImportPreview | null>(null);
  const [importConfirming, setImportConfirming] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [editing, setEditing] = useState<McpServer | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [registryOpen, setRegistryOpen] = useState(false);
  const [registryQuery, setRegistryQuery] = useState("");
  const [registryResults, setRegistryResults] = useState<RegistryMcpServer[]>([]);
  const [registryLoading, setRegistryLoading] = useState(false);
  const [registryCode, setRegistryCode] = useState(true);
  const [registryDesktop, setRegistryDesktop] = useState(false);
  const [form] = Form.useForm<FormValues>();
  const watchedTransport = Form.useWatch("transport", form) as McpTransport | undefined;
  const watchedCommand = Form.useWatch("command", form) as string | undefined;
  const watchedArgs = Form.useWatch("args", form) as string[] | undefined;
  const showRemoteBridgeHint = usesMcpRemoteBridge({
    command: watchedCommand ?? "",
    args: watchedArgs ?? [],
  });

  const openCreate = () => {
    setEditing(null);
    const structured = parseConfigObject(EXAMPLE_CONFIG);
    form.setFieldsValue({
      name: "",
      ...structured,
      enabledClaudeCode: true,
      enabledClaudeDesktop: false,
      enabledCodex: false,
    });
    setFormOpen(true);
  };

  const openEdit = (server: McpServer) => {
    setEditing(server);
    const structured = parseConfigObject(server.serverConfig);
    form.setFieldsValue({
      name: server.name,
      ...structured,
      enabledClaudeCode: server.enabledClaudeCode,
      enabledClaudeDesktop: server.enabledClaudeDesktop,
      enabledCodex: server.enabledCodex,
    });
    setFormOpen(true);
  };

  const applyAdvancedJson = () => {
    try {
      const parsed: unknown = JSON.parse(form.getFieldValue("serverConfig"));
      if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
        throw new Error(t("mcp.invalidConfig"));
      }
      const structured = parseConfigObject(parsed as Record<string, unknown>);
      form.setFieldsValue(structured);
    } catch (e) {
      form.setFields([{ name: "serverConfig", errors: [errMsg(e)] }]);
    }
  };

  const handleSave = async (values: FormValues) => {
    let serverConfig: Record<string, unknown>;
    try {
      serverConfig = mergeStructuredIntoConfig(values.serverConfig, values);
      if (!Object.keys(serverConfig).length) {
        throw new Error(t("mcp.requiredConfig"));
      }
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
        enabledCodex: values.enabledCodex,
      };
      await saveMcpServer(input);
      void message.success(t(editing ? "mcp.updated" : "mcp.created"));
      setFormOpen(false);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: mcpServersOptions.queryKey }),
        queryClient.invalidateQueries({ queryKey: mcpDesktopConflictOptions.queryKey }),
      ]);
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
      queryClient.setQueryData<McpServer[]>(mcpServersOptions.queryKey, (current = []) =>
        current.map((item) =>
          item.id === server.id
            ? {
                ...item,
                enabledClaudeCode:
                  target === "claude_code" ? enabled : item.enabledClaudeCode,
                enabledClaudeDesktop:
                  target === "claude_desktop" ? enabled : item.enabledClaudeDesktop,
                enabledCodex: target === "codex" ? enabled : item.enabledCodex,
              }
            : item,
        ),
      );
      await queryClient.invalidateQueries({ queryKey: mcpDesktopConflictOptions.queryKey });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleReorder = async (id: string, direction: -1 | 1) => {
    const index = servers.findIndex((server) => server.id === id);
    if (index < 0) return;
    const newIndex = index + direction;
    if (newIndex < 0 || newIndex >= servers.length) return;
    const ordered = [...servers];
    [ordered[index], ordered[newIndex]] = [ordered[newIndex], ordered[index]];
    setBusy(true);
    try {
      await reorderMcpServers(ordered.map((server) => server.id));
      queryClient.setQueryData(mcpServersOptions.queryKey, ordered);
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
      await queryClient.invalidateQueries({ queryKey: mcpServersOptions.queryKey });
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
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: mcpServersOptions.queryKey }),
        queryClient.invalidateQueries({ queryKey: mcpDesktopConflictOptions.queryKey }),
      ]);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleImportClipboard = async () => {
    setBusy(true);
    try {
      const text = await navigator.clipboard.readText();
      const preview = await previewImportText(text);
      if (preview.resource !== "mcp") {
        void message.warning(t("deeplink.expectMcp"));
        return;
      }
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
      await queryClient.invalidateQueries({ queryKey: mcpServersOptions.queryKey });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setImportConfirming(false);
    }
  };

  const handleShareLink = async (server: McpServer) => {
    try {
      const link = await buildMcpDeeplink(server.id);
      await navigator.clipboard.writeText(link);
      void message.success(t("deeplink.linkCopied"));
    } catch (e) {
      void message.error(errMsg(e));
    }
  };

  const handleRefresh = async () => {
    setRefreshing(true);
    try {
      await Promise.all([
        serversQuery.refetch(),
        oauthQuery.refetch(),
        conflictQuery.refetch(),
      ]);
    } finally {
      setRefreshing(false);
    }
  };

  const handleClearOauth = async (serverNames: string[] = []) => {
    setBusy(true);
    try {
      await clearMcpOauth(serverNames);
      void message.success(t("mcp.oauthCleared"));
      await queryClient.invalidateQueries({ queryKey: mcpOauthStatusOptions.queryKey });
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const searchRegistry = async () => {
    setRegistryLoading(true);
    try {
      setRegistryResults(await searchMcpRegistry(registryQuery));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setRegistryLoading(false);
    }
  };

  const openRegistry = () => {
    setRegistryOpen(true);
    if (!registryResults.length) void searchRegistry();
  };

  const installRegistryServer = async (server: RegistryMcpServer) => {
    setBusy(true);
    try {
      await installMcpRegistryServer(server.name, registryCode, registryDesktop);
      void message.success(t("mcp.registryInstalled", { name: server.title }));
      await queryClient.invalidateQueries({ queryKey: mcpServersOptions.queryKey });
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
      render: (config: Record<string, unknown>) => {
        const transport = detectTransport(config);
        const summary = config.command
          ? String(config.command)
          : config.url
            ? String(config.url)
            : "JSON";
        return (
          <Space size={4} wrap>
            <Text type="secondary">{t(`mcp.transport.${transport}`)}</Text>
            <Text code style={{ wordBreak: "break-all" }}>
              {summary}
            </Text>
          </Space>
        );
      },
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
      title: "Codex",
      dataIndex: "enabledCodex",
      width: 110,
      render: (enabled: boolean, server) => (
        <Switch
          size="small"
          checked={enabled}
          disabled={busy}
          checkedChildren={t("common.enabled")}
          unCheckedChildren={t("common.disabled")}
          onChange={(value) => void handleToggle(server, "codex", value)}
        />
      ),
    },
    {
      title: t("mcp.colActions"),
      key: "actions",
      width: 180,
      render: (_, server, index) => (
        <Space size="small">
          <Tooltip title={t("mcp.moveUp")}>
            <Button
              size="small"
              icon={<ArrowUpOutlined />}
              disabled={index === 0 || busy}
              onClick={() => void handleReorder(server.id, -1)}
            />
          </Tooltip>
          <Tooltip title={t("mcp.moveDown")}>
            <Button
              size="small"
              icon={<ArrowDownOutlined />}
              disabled={index === servers.length - 1 || busy}
              onClick={() => void handleReorder(server.id, 1)}
            />
          </Tooltip>
          <Tooltip title={t("mcp.edit")}>
            <Button size="small" icon={<EditOutlined />} disabled={busy} onClick={() => openEdit(server)} />
          </Tooltip>
          <Tooltip title={t("deeplink.shareLink")}>
            <Button size="small" icon={<GlobalOutlined />} disabled={busy} onClick={() => void handleShareLink(server)} />
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
    <>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        {serversQuery.error && <Alert type="error" showIcon message={errMsg(serversQuery.error)} />}
        <OnboardingTip tipKey="mcp" message={t("mcp.title")} description={t("mcp.description")} />
        {conflictQuery.data?.message && (
          <Alert
            type={conflictQuery.data.conflictLikely ? "warning" : "info"}
            showIcon
            message={t("mcp.connectorsTitle")}
            description={
              <Space direction="vertical" size={4}>
                <Text>{conflictQuery.data.message}</Text>
                {conflictQuery.data.extensionArtifacts.length > 0 && (
                  <Text type="secondary">
                    {t("mcp.connectorsArtifacts", {
                      names: conflictQuery.data.extensionArtifacts.slice(0, 8).join(", "),
                    })}
                  </Text>
                )}
              </Space>
            }
          />
        )}
        <Card size="small" className="page-surface" title={t("mcp.oauthTitle")}>
          {oauthQuery.error ? (
            <Alert type="error" showIcon message={errMsg(oauthQuery.error)} />
          ) : (
            <Space direction="vertical" style={{ width: "100%" }}>
              <Text type="secondary">
                {oauthQuery.data?.note ||
                  t("mcp.oauthSummary", {
                    count: oauthQuery.data?.entryCount ?? 0,
                    storage: oauthQuery.data?.storage ?? "none",
                  })}
              </Text>
              {(oauthQuery.data?.serverNames.length ?? 0) > 0 && (
                <Text>
                  {t("mcp.oauthServers", {
                    names: (oauthQuery.data?.serverNames ?? []).join(", "),
                  })}
                </Text>
              )}
              <Space>
                <Button
                  size="small"
                  loading={oauthQuery.isFetching}
                  onClick={() => void oauthQuery.refetch()}
                >
                  {t("common.refresh")}
                </Button>
                <Popconfirm
                  title={t("mcp.oauthClearConfirm")}
                  okText={t("mcp.oauthClear")}
                  cancelText={t("common.cancel")}
                  disabled={!oauthQuery.data?.clearable || (oauthQuery.data.entryCount ?? 0) === 0}
                  onConfirm={() => void handleClearOauth()}
                >
                  <Button
                    size="small"
                    danger
                    disabled={!oauthQuery.data?.clearable || (oauthQuery.data.entryCount ?? 0) === 0 || busy}
                  >
                    {t("mcp.oauthClear")}
                  </Button>
                </Popconfirm>
              </Space>
            </Space>
          )}
        </Card>
        <Card
          size="small"
          className="page-surface"
          title={t("mcp.title")}
          extra={
            <Space>
              <Button icon={<GlobalOutlined />} disabled={busy} onClick={openRegistry}>
                {t("mcp.registryBrowse")}
              </Button>
              <Button icon={<ImportOutlined />} loading={busy} onClick={() => void handleImport()}>
                {t("mcp.import")}
              </Button>
              <Button loading={busy} onClick={() => void handleImportClipboard()}>
                {t("mcp.importClipboard")}
              </Button>
              <Button
                icon={<SyncOutlined />}
                disabled={busy}
                loading={refreshing}
                onClick={() => void handleRefresh()}
              >
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
            loading={serversQuery.isPending}
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
        width={760}
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={handleSave}
          onValuesChange={(changed) => {
            if ("serverConfig" in changed) return;
            syncStructuredToJson(form);
          }}
          initialValues={{ enabledClaudeCode: true, transport: "stdio", args: [""], env: [], headers: [] }}
        >
          <Form.Item name="name" label={t("mcp.fieldName")} rules={[{ required: true, message: t("mcp.requiredName") }]}>
            <Input autoFocus disabled={busy} />
          </Form.Item>
          <Form.Item name="transport" label={t("mcp.fieldTransport")} rules={[{ required: true }]}>
            <Select
              disabled={busy}
              options={[
                { value: "stdio", label: t("mcp.transport.stdio") },
                { value: "http", label: t("mcp.transport.http") },
                { value: "sse", label: t("mcp.transport.sse") },
              ]}
            />
          </Form.Item>
          {(watchedTransport === "http" || watchedTransport === "sse") && (
            <Alert
              type="info"
              showIcon
              style={{ marginBottom: 16 }}
              message={t("mcp.remoteHint")}
            />
          )}
          {showRemoteBridgeHint && (
            <Alert
              type="warning"
              showIcon
              style={{ marginBottom: 16 }}
              message={t("mcp.mcpRemoteHint")}
            />
          )}
          {watchedTransport === "stdio" ? (
            <Form.Item name="command" label={t("mcp.fieldCommand")}>
              <Input disabled={busy} placeholder="npx" spellCheck={false} />
            </Form.Item>
          ) : (
            <Form.Item
              name="url"
              label={t("mcp.fieldUrl")}
              rules={[{ required: true, message: t("mcp.requiredUrl") }]}
            >
              <Input disabled={busy} placeholder="https://..." spellCheck={false} />
            </Form.Item>
          )}
          {watchedTransport === "stdio" && (
          <Form.Item label={t("mcp.fieldArgs")}>
            <Form.List name="args">
              {(fields, { add, remove, move }) => (
                <Space direction="vertical" style={{ width: "100%" }}>
                  {fields.map((field, index) => (
                    <Space key={field.key} align="baseline" style={{ width: "100%" }}>
                      <Form.Item {...field} style={{ flex: 1, marginBottom: 0 }}>
                        <Input disabled={busy} spellCheck={false} />
                      </Form.Item>
                      <Tooltip title={t("mcp.moveUp")}>
                        <Button
                          size="small"
                          icon={<ArrowUpOutlined />}
                          disabled={index === 0 || busy}
                          onClick={() => {
                            move(index, index - 1);
                            syncStructuredToJson(form);
                          }}
                        />
                      </Tooltip>
                      <Tooltip title={t("mcp.moveDown")}>
                        <Button
                          size="small"
                          icon={<ArrowDownOutlined />}
                          disabled={index === fields.length - 1 || busy}
                          onClick={() => {
                            move(index, index + 1);
                            syncStructuredToJson(form);
                          }}
                        />
                      </Tooltip>
                      <Button
                        size="small"
                        danger
                        icon={<DeleteOutlined />}
                        disabled={busy}
                        onClick={() => {
                          remove(field.name);
                          syncStructuredToJson(form);
                        }}
                      />
                    </Space>
                  ))}
                  <Button
                    type="dashed"
                    block
                    icon={<PlusOutlined />}
                    disabled={busy}
                    onClick={() => {
                      add("");
                      syncStructuredToJson(form);
                    }}
                  >
                    {t("mcp.addArg")}
                  </Button>
                </Space>
              )}
            </Form.List>
          </Form.Item>
          )}
          <KeyValueList
            name="env"
            label={t("mcp.fieldEnv")}
            busy={busy}
            addLabel={t("mcp.addEntry")}
            onChanged={() => syncStructuredToJson(form)}
            t={t}
          />
          <KeyValueList
            name="headers"
            label={t("mcp.fieldHeaders")}
            busy={busy}
            addLabel={t("mcp.addEntry")}
            onChanged={() => syncStructuredToJson(form)}
            t={t}
          />
          <Collapse
            items={[
              {
                key: "advanced",
                label: t("mcp.advancedJson"),
                children: (
                  <Form.Item
                    name="serverConfig"
                    extra={t("mcp.configHelp")}
                    rules={[{ required: true, message: t("mcp.requiredConfig") }]}
                  >
                    <Input.TextArea
                      rows={10}
                      spellCheck={false}
                      disabled={busy}
                      style={{ fontFamily: "monospace" }}
                      onBlur={applyAdvancedJson}
                    />
                  </Form.Item>
                ),
              },
            ]}
          />
          <Form.Item>
            <Space>
              <Form.Item name="enabledClaudeCode" valuePropName="checked" noStyle>
                <Checkbox disabled={busy}>{t("mcp.enableCode")}</Checkbox>
              </Form.Item>
              <Form.Item name="enabledClaudeDesktop" valuePropName="checked" noStyle>
                <Checkbox disabled={busy}>{t("mcp.enableDesktop")}</Checkbox>
              </Form.Item>
              <Form.Item name="enabledCodex" valuePropName="checked" noStyle>
                <Checkbox disabled={busy}>{t("mcp.enableCodex")}</Checkbox>
              </Form.Item>
            </Space>
          </Form.Item>
          <Paragraph type="secondary" style={{ marginBottom: 0 }}>
            <SaveOutlined /> {t("mcp.syncNote")}
          </Paragraph>
        </Form>
      </Modal>

      <Modal
        title={t("mcp.registryTitle")}
        open={registryOpen}
        onCancel={() => setRegistryOpen(false)}
        footer={null}
        width={920}
      >
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <Alert type="info" showIcon message={t("mcp.registryNotice")} />
          <Input.Search
            value={registryQuery}
            onChange={(e) => setRegistryQuery(e.target.value)}
            onSearch={() => void searchRegistry()}
            placeholder={t("mcp.registrySearchPlaceholder")}
            loading={registryLoading}
            enterButton={t("mcp.registrySearch")}
          />
          <Space>
            <Checkbox checked={registryCode} disabled={busy} onChange={(e) => setRegistryCode(e.target.checked)}>
              {t("mcp.enableCode")}
            </Checkbox>
            <Checkbox checked={registryDesktop} disabled={busy} onChange={(e) => setRegistryDesktop(e.target.checked)}>
              {t("mcp.enableDesktop")}
            </Checkbox>
          </Space>
          <Table<RegistryMcpServer>
            size="small"
            rowKey="name"
            dataSource={registryResults}
            loading={registryLoading}
            pagination={{ pageSize: 10, hideOnSinglePage: true }}
            locale={{ emptyText: t("mcp.registryEmpty") }}
            columns={[
              {
                title: t("mcp.colName"),
                dataIndex: "title",
                render: (_: string, server) => (
                  <Space direction="vertical" size={0}>
                    <Text strong>{server.title}</Text>
                    <Text type="secondary" code>
                      {server.name}
                    </Text>
                  </Space>
                ),
              },
              {
                title: t("mcp.registryVersion"),
                dataIndex: "version",
                width: 100,
                render: (value: string) => value || "—",
              },
              {
                title: t("mcp.description"),
                dataIndex: "description",
                render: (value: string, server) => value || <Text type="secondary">{server.supportNote || "—"}</Text>,
              },
              {
                title: t("mcp.colActions"),
                width: 120,
                render: (_: unknown, server) => (
                  <Button
                    type="link"
                    disabled={!server.installable || busy}
                    loading={busy}
                    onClick={() => void installRegistryServer(server)}
                  >
                    {server.installable ? t("mcp.registryInstall") : t("mcp.registryManual")}
                  </Button>
                ),
              },
            ]}
          />
        </Space>
      </Modal>
      <ImportPreviewDialog
        open={importPreview !== null}
        preview={importPreview}
        confirming={importConfirming}
        onCancel={() => setImportPreview(null)}
        onConfirm={() => void handleConfirmImport()}
      />
    </>
  );
}

function KeyValueList({
  name,
  label,
  busy,
  addLabel,
  onChanged,
  t,
}: {
  name: "env" | "headers";
  label: string;
  busy: boolean;
  addLabel: string;
  onChanged: () => void;
  t: (key: string) => string;
}) {
  return (
    <Form.Item label={label}>
      <Form.List name={name}>
        {(fields, { add, remove, move }) => (
          <Space direction="vertical" style={{ width: "100%" }}>
            {fields.map((field, index) => (
              <Space key={field.key} align="baseline" style={{ width: "100%" }}>
                <Form.Item
                  name={[field.name, "key"]}
                  style={{ flex: 1, marginBottom: 0 }}
                  rules={[{ required: true, message: t("mcp.fieldKey") }]}
                >
                  <Input disabled={busy} placeholder={t("mcp.fieldKey")} spellCheck={false} />
                </Form.Item>
                <Form.Item name={[field.name, "value"]} style={{ flex: 1, marginBottom: 0 }}>
                  <Input disabled={busy} placeholder={t("mcp.fieldValue")} spellCheck={false} />
                </Form.Item>
                <Tooltip title={t("mcp.moveUp")}>
                  <Button
                    size="small"
                    icon={<ArrowUpOutlined />}
                    disabled={index === 0 || busy}
                    onClick={() => {
                      move(index, index - 1);
                      onChanged();
                    }}
                  />
                </Tooltip>
                <Tooltip title={t("mcp.moveDown")}>
                  <Button
                    size="small"
                    icon={<ArrowDownOutlined />}
                    disabled={index === fields.length - 1 || busy}
                    onClick={() => {
                      move(index, index + 1);
                      onChanged();
                    }}
                  />
                </Tooltip>
                <Button
                  size="small"
                  danger
                  icon={<DeleteOutlined />}
                  disabled={busy}
                  onClick={() => {
                    remove(field.name);
                    onChanged();
                  }}
                />
              </Space>
            ))}
            <Button
              type="dashed"
              block
              icon={<PlusOutlined />}
              disabled={busy}
              onClick={() => {
                add({ key: "", value: "" });
                onChanged();
              }}
            >
              {addLabel}
            </Button>
          </Space>
        )}
      </Form.List>
    </Form.Item>
  );
}

function errMsg(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
