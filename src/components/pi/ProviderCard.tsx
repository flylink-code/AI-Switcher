import React, { useEffect, useState } from "react";
import { Card, Form, Input, Select, Button, Space, Typography, Tag, Table, Modal, message, Divider, Tooltip } from "antd";
import { CheckOutlined, PlusOutlined, EditOutlined, DeleteOutlined, SettingOutlined } from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";

const { Text } = Typography;

export const THINKING_LEVELS = [
  { value: "off", label: "Off (禁用思考)" },
  { value: "minimal", label: "Minimal (极简)" },
  { value: "low", label: "Low (轻量)" },
  { value: "medium", label: "Medium (中等)" },
  { value: "high", label: "High (高深度)" },
  { value: "xhigh", label: "Extra High (极高深度)" },
  { value: "max", label: "Max (最大思考)" },
];

export interface ProviderItem {
  id: string;
  name: string;
  baseUrl: string;
  apiKey?: string;
  models: string[];
}

export const PRESET_PROVIDERS: ProviderItem[] = [
  {
    id: "anthropic",
    name: "Anthropic",
    baseUrl: "https://api.anthropic.com/v1",
    models: ["claude-3-7-sonnet-20250219", "claude-3-5-sonnet-20241022", "claude-3-5-haiku-20241022"],
  },
  {
    id: "openai",
    name: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    models: ["gpt-4o", "gpt-4o-mini", "o1", "o3-mini"],
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    baseUrl: "https://openrouter.ai/api/v1",
    models: ["anthropic/claude-3.7-sonnet", "openai/gpt-4o", "deepseek/deepseek-r1"],
  },
  {
    id: "siliconflow",
    name: "SiliconFlow (硅基流动)",
    baseUrl: "https://api.siliconflow.cn/v1",
    models: ["deepseek-ai/DeepSeek-V3", "deepseek-ai/DeepSeek-R1"],
  },
  {
    id: "kimi",
    name: "Moonshot Kimi",
    baseUrl: "https://api.moonshot.cn/v1",
    models: ["moonshot-v1-8k", "moonshot-v1-32k", "moonshot-v1-128k"],
  },
];

export const ProviderCard: React.FC = () => {
  const [settings, setSettings] = useState<any>({});
  const [auth, setAuth] = useState<any>({});
  const [modelsObj, setModelsObj] = useState<any>({});
  const [loading, setLoading] = useState<boolean>(true);

  const [activeProvider, setActiveProvider] = useState<string>("");
  const [activeModel, setActiveModel] = useState<string>("");
  const [thinkingLevel, setThinkingLevel] = useState<string>("medium");

  const [editingProvider, setEditingProvider] = useState<ProviderItem | null>(null);
  const [modalOpen, setModalOpen] = useState<boolean>(false);
  const [form] = Form.useForm();

  const loadAllConfig = async () => {
    setLoading(true);
    try {
      const [sVal, aVal, mVal] = await Promise.all([
        invoke<any>("get_pi_settings"),
        invoke<any>("get_pi_auth"),
        invoke<any>("get_pi_models"),
      ]);
      setSettings(sVal || {});
      setAuth(aVal || {});
      setModelsObj(mVal || {});

      setActiveProvider(sVal?.defaultProvider || "");
      setActiveModel(sVal?.defaultModel || "");
      setThinkingLevel(sVal?.defaultThinkingLevel || "medium");
    } catch (e: any) {
      message.error(`获取 Pi 配置失败: ${e.message || e}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadAllConfig();
  }, []);

  const handleSaveSettings = async (dp?: string, dm?: string, dtl?: string) => {
    try {
      const updated = await invoke<any>("update_pi_settings", {
        defaultProvider: dp !== undefined ? dp : activeProvider,
        defaultModel: dm !== undefined ? dm : activeModel,
        defaultThinkingLevel: dtl !== undefined ? dtl : thinkingLevel,
      });
      setSettings(updated);
      message.success("Pi 核心设置保存成功");
    } catch (e: any) {
      message.error(`保存失败: ${e.message || e}`);
    }
  };

  const handleActivate = async (pId: string) => {
    setActiveProvider(pId);
    const pModels = modelsObj.providers?.[pId]?.models || PRESET_PROVIDERS.find(p => p.id === pId)?.models || [];
    const firstModel = pModels[0] || "";
    setActiveModel(firstModel);
    await handleSaveSettings(pId, firstModel, thinkingLevel);
  };

  const handleConnectLocalProxy = async (pId: string) => {
    try {
      const localProxyUrl = "http://127.0.0.1:5250/v1";
      const currentProviders = modelsObj.providers || {};
      const targetProvider = currentProviders[pId] || PRESET_PROVIDERS.find((p) => p.id === pId) || {
        name: pId,
        models: ["claude-3-5-sonnet-20241022"],
      };

      const newModelsObj = {
        ...modelsObj,
        providers: {
          ...currentProviders,
          [pId]: {
            ...targetProvider,
            baseUrl: localProxyUrl,
          },
        },
      };

      await invoke("save_pi_models", { modelsVal: newModelsObj });
      setModelsObj(newModelsObj);
      message.success(`已成功将供应商 [${pId}] 的 BaseURL 指向本地代理网关 (${localProxyUrl})`);
    } catch (e: any) {
      message.error(`指向本地网关失败: ${e.message || e}`);
    }
  };

  const handleOpenEditModal = (item?: ProviderItem) => {
    if (item) {
      setEditingProvider(item);
      form.setFieldsValue({
        id: item.id,
        name: item.name,
        baseUrl: item.baseUrl,
        apiKey: auth[item.id]?.apiKey || auth[item.id]?.token || "",
        modelsStr: (item.models || []).join(", "),
      });
    } else {
      setEditingProvider(null);
      form.resetFields();
    }
    setModalOpen(true);
  };

  const handleModalOk = async () => {
    try {
      const values = await form.validateFields();
      const pId = values.id.trim();
      const newAuth = { ...auth, [pId]: { apiKey: values.apiKey } };

      const parsedModels = values.modelsStr
        ? values.modelsStr.split(",").map((s: string) => s.trim()).filter(Boolean)
        : [];

      const currentProviders = modelsObj.providers || {};
      const newModelsObj = {
        ...modelsObj,
        providers: {
          ...currentProviders,
          [pId]: {
            name: values.name,
            baseUrl: values.baseUrl,
            models: parsedModels,
          },
        },
      };

      await Promise.all([
        invoke("save_pi_auth", { authVal: newAuth }),
        invoke("save_pi_models", { modelsVal: newModelsObj }),
      ]);

      setAuth(newAuth);
      setModelsObj(newModelsObj);
      setModalOpen(false);
      message.success("供应商信息更新成功");
    } catch (e: any) {
      message.error(`保存供应商失败: ${e.message || e}`);
    }
  };

  // 整理要展示的供应商列表（预设 + 已配置）
  const configuredProvidersMap = modelsObj.providers || {};
  const providerList: ProviderItem[] = PRESET_PROVIDERS.map((preset) => {
    const custom = configuredProvidersMap[preset.id];
    return {
      id: preset.id,
      name: custom?.name || preset.name,
      baseUrl: custom?.baseUrl || preset.baseUrl,
      apiKey: auth[preset.id]?.apiKey || auth[preset.id]?.token || "",
      models: custom?.models || preset.models,
    };
  });

  // 包含不在预设里的自定义供应商
  Object.keys(configuredProvidersMap).forEach((id) => {
    if (!providerList.some((p) => p.id === id)) {
      const c = configuredProvidersMap[id];
      providerList.push({
        id,
        name: c.name || id,
        baseUrl: c.baseUrl || "",
        apiKey: auth[id]?.apiKey || auth[id]?.token || "",
        models: c.models || [],
      });
    }
  });

  const columns = [
    {
      title: "供应商 ID / 名称",
      dataIndex: "name",
      key: "name",
      render: (text: string, record: ProviderItem) => (
        <Space direction="vertical" size={2}>
          <Space>
            <Text bold>{text}</Text>
            <Text type="secondary" style={{ fontSize: 12 }}>({record.id})</Text>
            {activeProvider === record.id && (
              <Tag color="success" icon={<CheckOutlined />}>当前激活</Tag>
            )}
          </Space>
        </Space>
      ),
    },
    {
      title: "Base URL",
      dataIndex: "baseUrl",
      key: "baseUrl",
      render: (url: string) => <Text code style={{ fontSize: 12 }}>{url || "-"}</Text>,
    },
    {
      title: "API Key",
      dataIndex: "apiKey",
      key: "apiKey",
      render: (key?: string) =>
        key ? (
          <Text type="success">已配置 (••••{key.slice(-4)})</Text>
        ) : (
          <Text type="warning">未设置</Text>
        ),
    },
    {
      title: "支持模型数",
      dataIndex: "models",
      key: "models",
      render: (models: string[]) => <Tag>{models?.length || 0} 个模型</Tag>,
    },
    {
      title: "操作",
      key: "action",
      render: (_: any, record: ProviderItem) => (
        <Space>
          {activeProvider !== record.id ? (
            <Button size="small" type="primary" ghost onClick={() => handleActivate(record.id)}>
              激活
            </Button>
          ) : (
            <Button size="small" disabled>已激活</Button>
          )}
          <Button size="small" icon={<EditOutlined />} onClick={() => handleOpenEditModal(record)}>
            编辑
          </Button>
          <Tooltip title="将该供应商 BaseURL 指向 http://127.0.0.1:5250/v1 以拦截计费">
            <Button size="small" onClick={() => handleConnectLocalProxy(record.id)}>
              接入网关
            </Button>
          </Tooltip>
        </Space>
      ),
    },
  ];

  return (
    <Card
      title="Pi 默认模型与供应商配置"
      loading={loading}
      extra={
        <Button type="dashed" icon={<PlusOutlined />} onClick={() => handleOpenEditModal()}>
          添加自定义供应商
        </Button>
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
        {/* 全局 Core 设置区域 */}
        <Card type="inner" title={<Space><SettingOutlined /><span>全局核心设置 (`settings.json`)</span></Space>}>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 16 }}>
            <div>
              <Text type="secondary" style={{ display: "block", marginBottom: 6 }}>默认激活供应商：</Text>
              <Select
                style={{ width: "100%" }}
                value={activeProvider}
                onChange={(val) => {
                  setActiveProvider(val);
                  handleActivate(val);
                }}
                options={providerList.map((p) => ({ label: `${p.name} (${p.id})`, value: p.id }))}
              />
            </div>

            <div>
              <Text type="secondary" style={{ display: "block", marginBottom: 6 }}>默认模型：</Text>
              <Input
                value={activeModel}
                onChange={(e) => setActiveModel(e.target.value)}
                onBlur={() => handleSaveSettings(activeProvider, activeModel, thinkingLevel)}
                placeholder="如 claude-3-7-sonnet-20250219"
              />
            </div>

            <div>
              <Text type="secondary" style={{ display: "block", marginBottom: 6 }}>7 档思考深度 (Thinking Level)：</Text>
              <Select
                style={{ width: "100%" }}
                value={thinkingLevel}
                onChange={(val) => {
                  setThinkingLevel(val);
                  handleSaveSettings(activeProvider, activeModel, val);
                }}
                options={THINKING_LEVELS}
              />
            </div>
          </div>
        </Card>

        {/* 供应商明细列表 */}
        <div>
          <Title level={5} style={{ marginBottom: 12 }}>供应商节点卡片与凭据</Title>
          <Table
            rowKey="id"
            columns={columns}
            dataSource={providerList}
            pagination={false}
            size="small"
          />
        </div>
      </div>

      {/* 编辑弹窗 */}
      <Modal
        title={editingProvider ? `编辑供应商 (${editingProvider.id})` : "添加自定义供应商"}
        open={modalOpen}
        onOk={handleModalOk}
        onCancel={() => setModalOpen(false)}
        destroyOnClose
      >
        <Form form={form} layout="vertical">
          <Form.Item name="id" label="供应商 ID" rules={[{ required: true, message: "请输入供应商 ID" }]}>
            <Input disabled={!!editingProvider} placeholder="如 openrouter" />
          </Form.Item>
          <Form.Item name="name" label="显示名称" rules={[{ required: true, message: "请输入显示名称" }]}>
            <Input placeholder="如 OpenRouter" />
          </Form.Item>
          <Form.Item name="baseUrl" label="Base URL (上游接口地址)">
            <Input placeholder="如 https://openrouter.ai/api/v1" />
          </Form.Item>
          <Form.Item name="apiKey" label="API Key / Token">
            <Input.Password placeholder="sk-..." />
          </Form.Item>
          <Form.Item name="modelsStr" label="包含模型 (英文逗号分隔)">
            <Input.TextArea rows={3} placeholder="model-a, model-b" />
          </Form.Item>
        </Form>
      </Modal>
    </Card>
  );
};
