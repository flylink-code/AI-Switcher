import { useMemo, useState } from "react";
import {
  Button,
  Card,
  Checkbox,
  Drawer,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Table,
  Typography,
  message,
} from "antd";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  addAntigravityGatewayUpstream,
  deleteGatewayUpstream,
  discoverGatewayUpstreamModels,
  discoverGatewayUpstreamModelsBatch,
  importGatewayUpstreamsFromProviders,
  listGatewayUpstreamModels,
  listGatewayUpstreams,
  listProviders,
  setGatewayUpstreamModelVisible,
  upsertGatewayUpstream,
} from "@/services/providers";
import { LABEL_KEYS, PROVIDER_TARGET_OPTIONS } from "@/components/AgentTargetSwitcher";
import type { Provider, ProviderInput, ProviderTarget, ProtocolType } from "@/types/backend";

const { Text } = Typography;

const PROTOCOL_OPTIONS: { value: ProtocolType; label: string }[] = [
  { value: "anthropic", label: "Anthropic" },
  { value: "openai_chat", label: "OpenAI Chat" },
  { value: "openai_responses", label: "OpenAI Responses" },
];

export function GatewayUpstreamPanel({ allowlistTarget }: { allowlistTarget: ProviderTarget }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [form] = Form.useForm<ProviderInput>();
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState<Provider | null>(null);
  const [saving, setSaving] = useState(false);
  const [addingAg, setAddingAg] = useState(false);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [refreshing, setRefreshing] = useState(false);

  const [importOpen, setImportOpen] = useState(false);
  const [importTarget, setImportTarget] = useState<ProviderTarget>(allowlistTarget);
  const [importIds, setImportIds] = useState<string[]>([]);
  const [importAllowlist, setImportAllowlist] = useState(true);
  const [importing, setImporting] = useState(false);

  const [modelsUpstream, setModelsUpstream] = useState<Provider | null>(null);
  const [modelsSaving, setModelsSaving] = useState(false);

  const upstreamsQuery = useQuery({
    queryKey: ["gateway-upstreams"],
    queryFn: listGatewayUpstreams,
  });
  const importProvidersQuery = useQuery({
    queryKey: ["providers", importTarget],
    queryFn: () => listProviders(importTarget),
    enabled: importOpen,
  });
  const modelsQuery = useQuery({
    queryKey: ["gateway-upstream-models", modelsUpstream?.id],
    queryFn: () => listGatewayUpstreamModels(modelsUpstream!.id),
    enabled: Boolean(modelsUpstream),
  });

  const importableProviders = useMemo(
    () => (importProvidersQuery.data ?? []).filter((item) => item.providerKind !== "smart_gateway"),
    [importProvidersQuery.data],
  );

  const invalidatePool = async () => {
    await queryClient.invalidateQueries({ queryKey: ["gateway-upstreams"] });
    await queryClient.invalidateQueries({ queryKey: ["gateway-catalog-models"] });
    await queryClient.invalidateQueries({ queryKey: ["gateway-catalog-entries"] });
  };

  const openCreate = () => {
    setEditing(null);
    form.resetFields();
    form.setFieldsValue({
      name: "",
      baseUrl: "",
      apiKey: "",
      model: "",
      protocolType: "anthropic",
      notes: "",
      targetApp: "claude_code",
      modelMapping: { sonnet: "", opus: "", haiku: "", fable: "", subagent: "" },
    });
    setOpen(true);
  };

  const openEdit = (provider: Provider) => {
    setEditing(provider);
    form.resetFields();
    form.setFieldsValue({
      id: provider.id,
      name: provider.name,
      baseUrl: provider.baseUrl,
      apiKey: "",
      model: provider.model,
      protocolType: provider.protocolType,
      notes: provider.notes,
      targetApp: "claude_code",
      modelMapping: { sonnet: "", opus: "", haiku: "", fable: "", subagent: "" },
    });
    setOpen(true);
  };

  const handleSave = async () => {
    const values = await form.validateFields();
    setSaving(true);
    try {
      await upsertGatewayUpstream({
        id: editing?.id,
        name: values.name,
        baseUrl: values.baseUrl,
        apiKey: values.apiKey ?? "",
        model: values.model,
        protocolType: values.protocolType,
        notes: values.notes ?? "",
        targetApp: "claude_code",
        modelMapping: { sonnet: "", opus: "", haiku: "", fable: "", subagent: "" },
        providerKind: editing?.providerKind === "antigravity" ? "antigravity" : "standard",
      });
      await invalidatePool();
      setOpen(false);
      void message.success(t("proxy.upstreamSaved"));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteGatewayUpstream(id);
      setSelectedIds((current) => current.filter((item) => item !== id));
      await invalidatePool();
      void message.success(t("proxy.upstreamDeleted"));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    }
  };

  const handleAddAg = async () => {
    setAddingAg(true);
    try {
      await addAntigravityGatewayUpstream();
      await invalidatePool();
      void message.success(t("proxy.upstreamAgAdded"));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setAddingAg(false);
    }
  };

  const handleImport = async () => {
    if (importIds.length === 0) {
      void message.warning(t("proxy.importUpstreamSelect"));
      return;
    }
    setImporting(true);
    try {
      const result = await importGatewayUpstreamsFromProviders(
        importTarget,
        importIds,
        importAllowlist ? allowlistTarget : null,
      );
      await invalidatePool();
      setImportOpen(false);
      setImportIds([]);
      void message.success(
        t("proxy.importUpstreamDone", { imported: result.imported, skipped: result.skipped }),
      );
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setImporting(false);
    }
  };

  const handleRefreshSelected = async () => {
    if (selectedIds.length === 0) {
      void message.warning(t("proxy.refreshModelsSelect"));
      return;
    }
    setRefreshing(true);
    try {
      const results = await discoverGatewayUpstreamModelsBatch(selectedIds);
      await invalidatePool();
      if (modelsUpstream && selectedIds.includes(modelsUpstream.id)) {
        await queryClient.invalidateQueries({ queryKey: ["gateway-upstream-models", modelsUpstream.id] });
      }
      const failed = results.filter((item) => item.result.error);
      if (failed.length > 0) {
        void message.warning(
          t("proxy.refreshModelsPartial", {
            ok: results.length - failed.length,
            failed: failed.length,
          }),
        );
      } else {
        void message.success(t("proxy.refreshModelsDone", { count: results.length }));
      }
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setRefreshing(false);
    }
  };

  const handleToggleModel = async (modelId: string, visible: boolean) => {
    if (!modelsUpstream) return;
    setModelsSaving(true);
    try {
      const rows = await setGatewayUpstreamModelVisible(modelsUpstream.id, modelId, visible);
      queryClient.setQueryData(["gateway-upstream-models", modelsUpstream.id], rows);
      await queryClient.invalidateQueries({ queryKey: ["gateway-catalog-entries"] });
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setModelsSaving(false);
    }
  };

  const handleRefreshDrawer = async () => {
    if (!modelsUpstream) return;
    setModelsSaving(true);
    try {
      const result = await discoverGatewayUpstreamModels(modelsUpstream.id);
      await queryClient.invalidateQueries({ queryKey: ["gateway-upstream-models", modelsUpstream.id] });
      await invalidatePool();
      if (result.error) {
        void message.warning(result.error);
      } else {
        void message.success(t("proxy.refreshModelsDone", { count: 1 }));
      }
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setModelsSaving(false);
    }
  };

  return (
    <Card
      size="small"
      className="page-surface"
      title={t("proxy.upstreamPool")}
      extra={
        <Space wrap>
          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={refreshing}
            disabled={selectedIds.length === 0}
            onClick={() => void handleRefreshSelected()}
          >
            {t("proxy.refreshSelectedModels")}
          </Button>
          <Button
            size="small"
            icon={<ImportOutlined />}
            onClick={() => {
              setImportTarget(allowlistTarget);
              setImportIds([]);
              setImportAllowlist(true);
              setImportOpen(true);
            }}
          >
            {t("proxy.importFromProviders")}
          </Button>
          <Button size="small" loading={addingAg} onClick={() => void handleAddAg()}>
            {t("proxy.addAgUpstream")}
          </Button>
          <Button size="small" type="primary" icon={<PlusOutlined />} onClick={openCreate}>
            {t("proxy.addUpstream")}
          </Button>
        </Space>
      }
    >
      <Text type="secondary" style={{ display: "block", marginBottom: 12, fontSize: 12 }}>
        {t("proxy.upstreamPoolHint")}
      </Text>
      <Table
        size="small"
        rowKey="id"
        pagination={false}
        loading={upstreamsQuery.isLoading}
        dataSource={upstreamsQuery.data ?? []}
        locale={{ emptyText: t("proxy.upstreamEmpty") }}
        rowSelection={{
          selectedRowKeys: selectedIds,
          onChange: (keys) => setSelectedIds(keys.map(String)),
        }}
        columns={[
          { title: t("proxy.upstreamName"), dataIndex: "name", ellipsis: true },
          { title: t("proxy.upstreamUrl"), dataIndex: "baseUrl", ellipsis: true },
          { title: t("proxy.upstreamModel"), dataIndex: "model", ellipsis: true, width: 160 },
          {
            title: t("proxy.upstreamProtocol"),
            dataIndex: "protocolType",
            width: 140,
          },
          {
            title: t("proxy.upstreamActions"),
            width: 200,
            render: (_, row: Provider) => (
              <Space size={4}>
                <Button type="link" size="small" onClick={() => setModelsUpstream(row)}>
                  {t("proxy.upstreamModels")}
                </Button>
                <Button type="link" size="small" onClick={() => openEdit(row)}>
                  {t("common.edit")}
                </Button>
                <Popconfirm
                  title={t("proxy.deleteUpstreamConfirm")}
                  onConfirm={() => void handleDelete(row.id)}
                >
                  <Button type="link" size="small" danger>
                    {t("common.delete")}
                  </Button>
                </Popconfirm>
              </Space>
            ),
          },
        ]}
      />
      <Modal
        open={open}
        title={editing ? t("proxy.editUpstream") : t("proxy.addUpstream")}
        onCancel={() => setOpen(false)}
        onOk={() => void handleSave()}
        confirmLoading={saving}
        destroyOnHidden
      >
        <Form form={form} layout="vertical">
          <Form.Item name="name" label={t("proxy.upstreamName")} rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="baseUrl" label={t("proxy.upstreamUrl")} rules={[{ required: true }]}>
            <Input placeholder="https://api.example.com" />
          </Form.Item>
          <Form.Item
            name="apiKey"
            label={t("proxy.upstreamKey")}
            extra={editing ? t("proxy.upstreamKeyKeep") : undefined}
          >
            <Input.Password />
          </Form.Item>
          <Form.Item name="model" label={t("proxy.upstreamModel")} rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="protocolType" label={t("proxy.upstreamProtocol")} rules={[{ required: true }]}>
            <Select options={PROTOCOL_OPTIONS} />
          </Form.Item>
          <Form.Item name="notes" label={t("proxy.upstreamNotes")}>
            <Input.TextArea rows={2} />
          </Form.Item>
        </Form>
      </Modal>
      <Modal
        open={importOpen}
        title={t("proxy.importFromProviders")}
        onCancel={() => setImportOpen(false)}
        onOk={() => void handleImport()}
        confirmLoading={importing}
        destroyOnHidden
      >
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <div>
            <Text type="secondary" style={{ display: "block", marginBottom: 8, fontSize: 12 }}>
              {t("proxy.importUpstreamHint")}
            </Text>
            <Select
              style={{ width: "100%" }}
              value={importTarget}
              onChange={(value) => {
                setImportTarget(value);
                setImportIds([]);
              }}
              options={PROVIDER_TARGET_OPTIONS.map((item) => ({
                value: item,
                label: t(LABEL_KEYS[item]),
              }))}
            />
          </div>
          <Checkbox.Group
            style={{ display: "flex", flexDirection: "column", gap: 8 }}
            value={importIds}
            onChange={(values) => setImportIds(values.map(String))}
            options={importableProviders.map((item) => ({
              value: item.id,
              label: `${item.name} · ${item.baseUrl}`,
            }))}
          />
          {importableProviders.length === 0 && (
            <Text type="secondary">{t("proxy.importUpstreamEmpty")}</Text>
          )}
          <Checkbox checked={importAllowlist} onChange={(event) => setImportAllowlist(event.target.checked)}>
            {t("proxy.importAddAllowlist")}
          </Checkbox>
        </Space>
      </Modal>
      <Drawer
        title={modelsUpstream ? t("proxy.upstreamModelsTitle", { name: modelsUpstream.name }) : t("proxy.upstreamModels")}
        open={Boolean(modelsUpstream)}
        onClose={() => setModelsUpstream(null)}
        width={420}
        extra={
          <Button size="small" loading={modelsSaving} onClick={() => void handleRefreshDrawer()}>
            {t("proxy.refreshModels")}
          </Button>
        }
      >
        <Text type="secondary" style={{ display: "block", marginBottom: 12, fontSize: 12 }}>
          {t("proxy.upstreamModelsHint")}
        </Text>
        <Space direction="vertical" size="small" style={{ width: "100%" }}>
          {(modelsQuery.data ?? []).map((row) => {
            const isDefault =
              Boolean(modelsUpstream) &&
              row.modelId.trim().toLowerCase() === (modelsUpstream?.model ?? "").trim().toLowerCase();
            return (
              <Space
                key={row.modelId}
                align="center"
                style={{ width: "100%", justifyContent: "space-between" }}
              >
                <Text ellipsis style={{ maxWidth: 260 }}>
                  {row.modelId}
                  {isDefault ? ` (${t("proxy.upstreamDefaultModel")})` : ""}
                </Text>
                <Switch
                  size="small"
                  checked={row.visible}
                  disabled={isDefault || modelsSaving}
                  onChange={(checked) => void handleToggleModel(row.modelId, checked)}
                />
              </Space>
            );
          })}
          {(modelsQuery.data ?? []).length === 0 && (
            <Text type="secondary">{t("proxy.upstreamModelsEmpty")}</Text>
          )}
        </Space>
      </Drawer>
    </Card>
  );
}
