import {
  AutoComplete,
  Button,
  Form,
  Input,
  Modal,
  Select,
  Space,
  Typography,
} from "antd";
import type { FormInstance } from "antd";
import { useTranslation } from "react-i18next";
import {
  buildEndpointPreview,
  isReservedListenerUrl,
  needsOpenAiV1Suffix,
  normalizeBaseUrl,
} from "@/lib/providerUrl";
import { PROVIDER_PRESETS, type ProviderPreset } from "@/lib/providerPresets";
import type { ProtocolType, Provider, ProviderInput } from "@/types/backend";

const { Text } = Typography;

export const UPSTREAM_PRESETS: ProviderPreset[] = PROVIDER_PRESETS.filter(
  (preset) =>
    !isReservedListenerUrl(preset.baseUrl) &&
    !preset.baseUrl.includes(":15830") &&
    !preset.baseUrl.includes(":15831"),
);

export const PROTOCOL_OPTIONS: { value: ProtocolType; label: string }[] = [
  { value: "anthropic", label: "Anthropic" },
  { value: "openai_chat", label: "OpenAI Chat" },
  { value: "openai_responses", label: "OpenAI Responses" },
];

export interface UpstreamFormModalProps {
  open: boolean;
  editing: Provider | null;
  form: FormInstance<ProviderInput>;
  saving: boolean;
  selectedPresetId: string | null;
  urlOptions: Array<{ value: string }>;
  onClearPreset: () => void;
  onApplyPreset: (preset: ProviderPreset) => void;
  onNormalizeBaseUrl: () => void;
  onAppendV1Suffix: () => void;
  onSubmit: () => void;
  onClose: () => void;
}

export function UpstreamFormModal({
  open,
  editing,
  form,
  saving,
  selectedPresetId,
  urlOptions,
  onClearPreset,
  onApplyPreset,
  onNormalizeBaseUrl,
  onAppendV1Suffix,
  onSubmit,
  onClose,
}: UpstreamFormModalProps) {
  const { t } = useTranslation();

  const watchedBaseUrl = Form.useWatch("baseUrl", form);
  const watchedProtocol = Form.useWatch("protocolType", form) ?? "anthropic";
  const endpointPreview = buildEndpointPreview(watchedBaseUrl, watchedProtocol);
  const showAppendV1 =
    (watchedProtocol === "openai_chat" || watchedProtocol === "openai_responses") &&
    typeof watchedBaseUrl === "string" &&
    needsOpenAiV1Suffix(watchedBaseUrl);

  return (
    <Modal
      open={open}
      title={editing ? t("proxy.editUpstream") : t("proxy.addUpstream")}
      onCancel={onClose}
      onOk={onSubmit}
      confirmLoading={saving}
      destroyOnHidden
    >
      <Form form={form} layout="vertical">
        {!editing ? (
          <Form.Item
            label={t("providers.fromPreset")}
            extra={t("providers.fromPresetHint")}
          >
            <Space wrap size={[8, 8]}>
              <Button
                size="small"
                type={selectedPresetId === null ? "primary" : "default"}
                onClick={onClearPreset}
              >
                {t("providers.blankPreset")}
              </Button>
              {UPSTREAM_PRESETS.map((preset) => (
                <Button
                  key={preset.id}
                  size="small"
                  type={selectedPresetId === preset.id ? "primary" : "default"}
                  onClick={() => onApplyPreset(preset)}
                >
                  {preset.name}
                  {preset.protocolType === "openai_chat"
                    ? " · Chat"
                    : preset.protocolType === "openai_responses"
                      ? " · Responses"
                      : ""}
                </Button>
              ))}
            </Space>
          </Form.Item>
        ) : null}
        <Form.Item name="name" label={t("proxy.upstreamName")} rules={[{ required: true }]}>
          <Input />
        </Form.Item>
        <Form.Item
          name="baseUrl"
          label={t("proxy.upstreamUrl")}
          extra={
            <Space direction="vertical" size={2}>
              {endpointPreview ? (
                <>
                  <Text type="secondary">{t("providers.endpointPreview")}</Text>
                  <Text code copyable>{endpointPreview}</Text>
                </>
              ) : (
                <Text type="secondary">{t("providers.baseUrlHint")}</Text>
              )}
              {showAppendV1 ? (
                <Button type="link" size="small" onClick={onAppendV1Suffix} style={{ paddingInline: 0 }}>
                  {t("providers.appendV1")}
                </Button>
              ) : null}
            </Space>
          }
          rules={[
            { required: true },
            {
              validator: async (_, value: unknown) => {
                if (typeof value !== "string" || !value.trim()) return;
                try {
                  const normalized = normalizeBaseUrl(value);
                  if (isReservedListenerUrl(normalized)) {
                    throw new Error("upstreamReservedUrl");
                  }
                } catch (error) {
                  const key = error instanceof Error ? error.message : "invalidBaseUrl";
                  if (key === "upstreamReservedUrl") {
                    throw new Error(
                      t("proxy.upstreamReservedUrl", { defaultValue: "上游不能指向本机 15821–15828" }),
                    );
                  }
                  throw new Error(t(`providers.${key}`));
                }
              },
            },
          ]}
        >
          <AutoComplete
            options={urlOptions}
            placeholder="https://api.deepseek.com/anthropic"
            onBlur={onNormalizeBaseUrl}
            filterOption={(input, option) =>
              String(option?.value ?? "").toLowerCase().includes(input.trim().toLowerCase())
            }
          />
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
  );
}
