import { useEffect, useState } from "react";
import {
  App,
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
import type { Provider, ProviderInput, ProtocolType } from "@/types/backend";
import { discoverProviderModelsInput, testProviderInput } from "@/services/api";

interface ProviderFormProps {
  open: boolean;
  /** When editing, the provider being edited; when null, creating. */
  editing: Provider | null;
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
        protocolType: editing.protocolType,
        notes: editing.notes,
        targetApp: editing.targetApp,
      });
    } else {
      setModels([]);
      form.resetFields();
      form.setFieldsValue({
        protocolType: "anthropic" as ProtocolType,
        targetApp: "claude_code",
      });
    }
    // Focus the name field after the modal paints.
    setTimeout(() => nameRef?.focus(), 50);
  }, [open, editing, form]);

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
      const values = await form.validateFields(["name", "baseUrl", "apiKey", "model", "protocolType", "targetApp", "notes", "id"]);
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

  return (
    <Modal
      open={open}
      title={isEdit ? t("providers.editTitle") : t("providers.createTitle")}
      okText={t("providers.save")}
      cancelText={t("providers.cancel")}
      onCancel={onCancel}
      onOk={handleOk}
      destroyOnHidden
      width={520}
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

        <Form.Item name="model" label={t("providers.fieldModel")} extra={<Space size="small">
          <Button type="link" size="small" loading={testing} onClick={() => void testConnection()}>{t("providers.testConnection")}</Button>
          <Button type="link" size="small" loading={discovering} onClick={() => void discoverModels()}>{t("providers.discoverModels")}</Button>
        </Space>}>
          <Input placeholder="model-name" list="provider-models" />
        </Form.Item>
        <datalist id="provider-models">
          {models.map((model) => <option key={model} value={model} />)}
        </datalist>

        <Form.Item name="notes" label={t("providers.fieldNotes")}>
          <Input.TextArea rows={2} />
        </Form.Item>
      </Form>
    </Modal>
  );
}
