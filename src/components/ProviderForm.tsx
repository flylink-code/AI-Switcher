import { useEffect, useState } from "react";
import {
  App,
  AutoComplete,
  Button,
  Form,
  Checkbox,
  Input,
  Modal,
  Select,
  Space,
  Typography,
  type InputRef,
} from "antd";
import { useTranslation } from "react-i18next";
import type {
  ClaudeModelMapping,
  Provider,
  ProviderInput,
  ProviderTarget,
  ProtocolType,
} from "@/types/backend";
import { discoverProviderModelsInput, testProviderInput } from "@/services/api";

interface ProviderFormProps {
  open: boolean;
  /** When editing, the provider being edited; when null, creating. */
  editing: Provider | null;
  target: ProviderTarget;
  onCancel: () => void;
  onSubmit: (input: ProviderInput) => Promise<void>;
}

const protocolEndpoints: Record<ProtocolType, string> = {
  anthropic: "/v1/messages",
  proxy: "/v1/chat/completions",
  openai_chat: "/v1/chat/completions",
  openai_responses: "/v1/responses",
};

/** Validate and convert a pasted request endpoint into a reusable HTTPS Base URL. */
function normalizeBaseUrl(value: string): string {
  const trimmed = value.trim();
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    throw new Error("invalidBaseUrl");
  }
  if (url.protocol !== "https:") throw new Error("baseUrlMustUseHttps");
  if (url.username || url.password) throw new Error("baseUrlNoCredentials");
  if (url.search || url.hash) throw new Error("baseUrlNoQueryOrFragment");

  let path = url.pathname.replace(/\/+$/, "");
  path = path.replace(/\/(?:chat\/completions|messages|responses|models)$/i, "");
  url.pathname = path.replace(/\/+$/, "") || "/";
  return url.toString().replace(/\/+$/, "");
}

function buildEndpointPreview(baseUrl: string | undefined, protocol: ProtocolType): string {
  if (!baseUrl?.trim()) return "";
  try {
    const base = normalizeBaseUrl(baseUrl);
    const endpoint = protocolEndpoints[protocol];
    return `${base}${base.endsWith("/v1") && endpoint.startsWith("/v1/") ? endpoint.slice(3) : endpoint}`;
  } catch {
    return "";
  }
}

export function ProviderForm({
  open,
  editing,
  target,
  onCancel,
  onSubmit,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [form] = Form.useForm<ProviderInput>();
  const [models, setModels] = useState<string[]>([]);
  const [discovering, setDiscovering] = useState(false);
  const [testing, setTesting] = useState(false);
  const watchedBaseUrl = Form.useWatch("baseUrl", form);
  const watchedProtocol = Form.useWatch("protocolType", form) ?? "anthropic";
  const watchedDefaultModel = Form.useWatch("model", form) ?? "";
  const watchedMapping = Form.useWatch("modelMapping", form);
  const endpointPreview = buildEndpointPreview(watchedBaseUrl, watchedProtocol);
  let nameRef: InputRef | null = null;

  const isEdit = editing !== null;

  useEffect(() => {
    if (!open) return;
    if (editing) {
      setModels([]);
      form.setFieldsValue({
        id: editing.id,
        name: editing.name,
        baseUrl: editing.baseUrl,
        apiKey: "",
        clearApiKey: false,
        model: editing.model,
        modelMapping: editing.modelMapping,
        protocolType: editing.protocolType,
        notes: editing.notes,
        targetApp: editing.targetApp,
      });
    } else {
      setModels([]);
      form.resetFields();
      form.setFieldsValue({
        protocolType: "anthropic" as ProtocolType,
        targetApp: target,
        modelMapping: {
          sonnet: "",
          opus: "",
          haiku: "",
          fable: "",
          subagent: "",
        },
      });
    }
    // Focus the name field after the modal paints.
    setTimeout(() => nameRef?.focus(), 50);
  }, [open, editing, form, target]);

  const handleOk = async () => {
    try {
      const values = await form.validateFields();
      const normalized = { ...values, baseUrl: normalizeBaseUrl(values.baseUrl) };
      form.setFieldValue("baseUrl", normalized.baseUrl);
      await onSubmit(normalized);
    } catch {
      // validation errors are shown inline by the form
    }
  };

  const discoverModels = async () => {
    setDiscovering(true);
    try {
      await form.validateFields(["name", "baseUrl", "apiKey", "protocolType", "targetApp"]);
      const values = form.getFieldsValue(true);
      const normalized = { ...values, baseUrl: normalizeBaseUrl(values.baseUrl) };
      form.setFieldValue("baseUrl", normalized.baseUrl);
      const result = await discoverProviderModelsInput(normalized);
      setModels(result.models);
      void message.info(result.message);
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally { setDiscovering(false); }
  };

  const testConnection = async () => {
    setTesting(true);
    try {
      const values = await form.validateFields();
      const normalized = { ...values, baseUrl: normalizeBaseUrl(values.baseUrl) };
      form.setFieldValue("baseUrl", normalized.baseUrl);
      const result = await testProviderInput(normalized);
      void (result.ok ? message.success : message.error)(result.message);
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally { setTesting(false); }
  };

  const normalizeBaseUrlField = () => {
    const value = form.getFieldValue("baseUrl");
    if (typeof value === "string") {
      try {
        form.setFieldValue("baseUrl", normalizeBaseUrl(value));
      } catch {
        // Keep invalid input visible so the form validator can explain it.
      }
    }
  };

  const roleFields: Array<{
    key: keyof ClaudeModelMapping;
    label: string;
    codeOnly?: boolean;
  }> = [
    { key: "sonnet", label: t("providers.modelRoleSonnet") },
    { key: "opus", label: t("providers.modelRoleOpus") },
    { key: "haiku", label: t("providers.modelRoleHaiku") },
    { key: "fable", label: t("providers.modelRoleFable") },
    { key: "subagent", label: t("providers.modelRoleSubagent"), codeOnly: true },
  ];
  const visibleRoleFields = roleFields.filter(
    (role) => !role.codeOnly || (editing?.targetApp ?? target) === "claude_code",
  );
  const modelOptions = models.map((model) => ({ value: model }));

  const fillAllRoles = () => {
    const model = form.getFieldValue("model")?.trim();
    if (!model) {
      void message.warning(t("providers.requiredDefaultModel"));
      return;
    }
    const mapping = {
      sonnet: model,
      opus: model,
      haiku: model,
      fable: model,
      subagent: (editing?.targetApp ?? target) === "claude_code" ? model : "",
    };
    form.setFieldValue("modelMapping", mapping);
  };

  return (
    <Modal
      open={open}
      title={isEdit ? t("providers.editTitle") : t("providers.createTitle")}
      okText={t("providers.save")}
      cancelText={t("providers.cancel")}
      onCancel={onCancel}
      onOk={handleOk}
      destroyOnHidden
      width={640}
      styles={{ body: { maxHeight: "calc(100vh - 200px)", overflowY: "auto", paddingInlineEnd: 4 } }}
    >
      <Form form={form} layout="vertical" autoComplete="off">
        <Form.Item name="id" hidden>
          <Input />
        </Form.Item>

        <Form.Item
          name="name"
          label={t("providers.fieldName")}
          rules={[{ required: true, message: t("providers.requiredName") }]}
        >
          <Input
            ref={(r) => {
              nameRef = r;
            }}
            placeholder="Kimi / DeepSeek / ..."
          />
        </Form.Item>

        <Form.Item
          name="protocolType"
          label={t("providers.fieldProtocol")}
          rules={[{ required: true }]}
        >
          <Select
            options={[
              { value: "anthropic", label: t("providers.protocolAnthropic") },
              { value: "openai_chat", label: t("providers.protocolOpenAiChat") },
              { value: "openai_responses", label: t("providers.protocolOpenAiResponses") },
            ]}
          />
        </Form.Item>

        <Form.Item
          name="baseUrl"
          label={t("providers.fieldBaseUrl")}
          extra={
            endpointPreview ? (
              <Space direction="vertical" size={2}>
                <Typography.Text type="secondary">{t("providers.endpointPreview")}</Typography.Text>
                <Typography.Text code copyable>{endpointPreview}</Typography.Text>
              </Space>
            ) : (
              t("providers.baseUrlHint")
            )
          }
          rules={[
            { required: true, message: t("providers.requiredBaseUrl") },
            {
              validator: async (_, value: unknown) => {
                if (typeof value !== "string" || !value.trim()) return;
                try {
                  normalizeBaseUrl(value);
                } catch (error) {
                  const key = error instanceof Error ? error.message : "invalidBaseUrl";
                  throw new Error(t(`providers.${key}`));
                }
              },
            },
          ]}
        >
          <Input
            placeholder="https://api.example.com or https://gateway.example.com/openai/v1"
            onBlur={normalizeBaseUrlField}
          />
        </Form.Item>

        <Form.Item
          name="apiKey"
          label={t("providers.fieldApiKey")}
          extra={editing?.apiKeySet ? t("providers.keyStored") : undefined}
        >
          <Input.Password placeholder="sk-..." autoComplete="new-password" />
        </Form.Item>

        {editing?.apiKeySet && (
          <Form.Item name="clearApiKey" valuePropName="checked">
            <Checkbox>{t("providers.clearKey")}</Checkbox>
          </Form.Item>
        )}

        <Form.Item
          name="model"
          label={t("providers.defaultModel")}
          rules={[{ required: true, whitespace: true, message: t("providers.requiredDefaultModel") }]}
          extra={<Space size="small" wrap>
            <Button type="link" size="small" loading={testing} onClick={() => void testConnection()}>{t("providers.testConnection")}</Button>
            <Button type="link" size="small" loading={discovering} onClick={() => void discoverModels()}>{t("providers.discoverModels")}</Button>
            <Button type="link" size="small" onClick={fillAllRoles}>{t("providers.fillAllModels")}</Button>
          </Space>}
        >
          <AutoComplete
            options={modelOptions}
            placeholder="model-name"
            filterOption={(input, option) =>
              String(option?.value ?? "").toLowerCase().includes(input.toLowerCase())
            }
          />
        </Form.Item>

        <Typography.Title level={5} style={{ marginBlock: "4px 8px" }}>
          {t("providers.modelMapping")}
        </Typography.Title>
        <Typography.Paragraph type="secondary" style={{ marginBottom: 12 }}>
          {t("providers.modelMappingHint")}
        </Typography.Paragraph>
        {visibleRoleFields.map((role) => (
          <Form.Item
            key={role.key}
            name={["modelMapping", role.key]}
            label={role.label}
            extra={t("providers.modelFallback", { model: watchedDefaultModel || "—" })}
          >
            <AutoComplete
              allowClear
              options={modelOptions}
              placeholder={watchedDefaultModel || "model-name"}
              filterOption={(input, option) =>
                String(option?.value ?? "").toLowerCase().includes(input.toLowerCase())
              }
            />
          </Form.Item>
        ))}
        <Typography.Paragraph type="secondary">
          {visibleRoleFields.map((role) => {
            const mapped = watchedMapping?.[role.key]?.trim() || watchedDefaultModel || "—";
            return (
              <Typography.Text key={role.key} code style={{ marginInlineEnd: 8 }}>
                {role.label} → {mapped}
              </Typography.Text>
            );
          })}
        </Typography.Paragraph>

        <Form.Item name="notes" label={t("providers.fieldNotes")}>
          <Input.TextArea rows={2} />
        </Form.Item>
      </Form>
    </Modal>
  );
}
