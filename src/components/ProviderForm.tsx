import { useEffect } from "react";
import {
  Form,
  Input,
  Modal,
  Select,
  type InputRef,
} from "antd";
import { useTranslation } from "react-i18next";
import type { Provider, ProviderInput, ProtocolType } from "@/types/backend";

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
  const [form] = Form.useForm<ProviderInput>();
  let nameRef: InputRef | null = null;

  const isEdit = editing !== null;

  useEffect(() => {
    if (!open) return;
    if (editing) {
      form.setFieldsValue({
        id: editing.id,
        name: editing.name,
        baseUrl: editing.baseUrl,
        apiKey: editing.apiKey,
        model: editing.model,
        protocolType: editing.protocolType,
        notes: editing.notes,
        targetApp: editing.targetApp,
      });
    } else {
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

        <Form.Item name="apiKey" label={t("providers.fieldApiKey")}>
          <Input.Password placeholder="sk-..." autoComplete="new-password" />
        </Form.Item>

        <Form.Item name="model" label={t("providers.fieldModel")}>
          <Input placeholder="model-name" />
        </Form.Item>

        <Form.Item name="protocolType" label={t("providers.fieldProtocol")}>
          <Select
            options={[
              { value: "anthropic", label: t("providers.protocolAnthropic") },
              { value: "proxy", label: t("providers.protocolProxy") },
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
