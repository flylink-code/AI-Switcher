import { useEffect, useState } from "react";
import {
  App,
  Button,
  Form,
  Checkbox,
  Input,
  Modal,
  Select,
  type InputRef,
} from "antd";
import { useTranslation } from "react-i18next";
import type { Provider, ProviderInput, ProtocolType } from "@/types/backend";
import { discoverProviderModels } from "@/services/api";

interface ProviderFormProps {
  open: boolean;
  /** When editing, the provider being edited; when null, creating. */
  editing: Provider | null;
  onCancel: () => void;
  onSubmit: (input: ProviderInput) => Promise<void>;
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
      await onSubmit(values);
    } catch {
      // validation errors are shown inline by the form
    }
  };

  const discoverModels = async () => {
    if (!editing) return;
    setDiscovering(true);
    try {
      const result = await discoverProviderModels(editing.id);
      setModels(result.models);
      void message.info(result.message);
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally { setDiscovering(false); }
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
          name="baseUrl"
          label={t("providers.fieldBaseUrl")}
          rules={[{ required: true, message: t("providers.requiredBaseUrl") }]}
        >
          <Input placeholder="https://api.example.com/anthropic" />
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

        <Form.Item name="model" label={t("providers.fieldModel")} extra={
          <Button type="link" size="small" loading={discovering} disabled={!editing} onClick={() => void discoverModels()}>
            {t("providers.discoverModels")}
          </Button>
        }>
          <Input placeholder="model-name" list="provider-models" />
        </Form.Item>
        <datalist id="provider-models">
          {models.map((model) => <option key={model} value={model} />)}
        </datalist>

        <Form.Item name="protocolType" label={t("providers.fieldProtocol")}>
          <Select
            options={[
              { value: "anthropic", label: t("providers.protocolAnthropic") },
              { value: "openai_chat", label: t("providers.protocolOpenAiChat") },
              { value: "openai_responses", label: t("providers.protocolOpenAiResponses") },
            ]}
          />
        </Form.Item>

        <Form.Item name="notes" label={t("providers.fieldNotes")}>
          <Input.TextArea rows={2} />
        </Form.Item>
      </Form>
    </Modal>
  );
}
