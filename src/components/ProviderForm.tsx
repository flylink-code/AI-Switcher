import { useEffect, useRef, useState } from "react";
import {
  App,
  AutoComplete,
  Button,
  Form,
  Checkbox,
  Input,
  InputNumber,
  Modal,
  Select,
  Space,
  Typography,
  type InputRef,
} from "antd";
import { useTranslation } from "react-i18next";
import type {
  ClaudeModelMapping,
  ModelDiscoveryResult,
  Provider,
  ProviderInput,
  ProviderTarget,
  ProtocolType,
} from "@/types/backend";
import {
  discoverProviderModels,
  discoverProviderModelsInput,
  getCachedProviderModels,
  testProviderInput,
} from "@/services/api";

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

const codexModelSuggestions = [
  "gpt-5.6-sol",
  "gpt-5.6-terra",
  "gpt-5.6-luna",
  "gpt-5.5",
  "gpt-5.4",
  "gpt-5.4-mini",
  "gpt-5.3-codex",
];

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

function needsOpenAiV1Suffix(value: string): boolean {
  try {
    const normalized = normalizeBaseUrl(value);
    const url = new URL(normalized);
    const path = url.pathname.replace(/\/+$/, "") || "/";
    return path === "/";
  } catch {
    return false;
  }
}

function ensureOpenAiV1Suffix(value: string): string {
  const normalized = normalizeBaseUrl(value);
  return needsOpenAiV1Suffix(normalized) ? `${normalized}/v1` : normalized;
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
  const [modelResult, setModelResult] = useState<ModelDiscoveryResult | null>(null);
  const [discovering, setDiscovering] = useState(false);
  const [testing, setTesting] = useState(false);
  const watchedBaseUrl = Form.useWatch("baseUrl", form);
  const watchedProtocol = Form.useWatch("protocolType", form) ?? "anthropic";
  const watchedDefaultModel = Form.useWatch("model", form) ?? "";
  const watchedMapping = Form.useWatch("modelMapping", form);
  const endpointPreview = buildEndpointPreview(watchedBaseUrl, watchedProtocol);
  let nameRef: InputRef | null = null;

  const isEdit = editing !== null;
  const isCodex = (editing?.targetApp ?? target) === "codex";
  const prevModelRef = useRef<string | null>(null);
  const skipModelSyncRef = useRef(true);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    skipModelSyncRef.current = true;
    prevModelRef.current = null;
    setModelResult(null);
    if (editing) {
      setModels([]);
      form.setFieldsValue({
        id: editing.id,
        name: editing.name,
        baseUrl: editing.baseUrl,
        apiKey: "",
        clearApiKey: false,
        model: editing.model,
        modelContextWindow: editing.modelContextWindow ?? undefined,
        autoReviewModelOverride: editing.autoReviewModelOverride ?? undefined,
        modelMapping: editing.modelMapping,
        protocolType: editing.protocolType,
        notes: editing.notes,
        targetApp: editing.targetApp,
      });
      void getCachedProviderModels(editing.id)
        .then((result) => {
          if (cancelled || result.source !== "cache") return;
          setModels(result.models);
          setModelResult(result);
        })
        .catch(() => {
          // A missing or unreadable cache must not prevent editing the provider.
        });
    } else {
      setModels([]);
      form.resetFields();
      form.setFieldsValue({
        protocolType: (target === "codex" ? "openai_responses" : "anthropic") as ProtocolType,
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
    return () => {
      cancelled = true;
    };
  }, [open, editing, form, target]);

  useEffect(() => {
    if (!open) return;
    if (isCodex) return;
    const model = watchedDefaultModel?.trim() ?? "";
    if (skipModelSyncRef.current) {
      skipModelSyncRef.current = false;
      prevModelRef.current = model;
      return;
    }
    if (!model || model === prevModelRef.current) return;
    prevModelRef.current = model;
    const isCode = (editing?.targetApp ?? target) === "claude_code";
    form.setFieldValue("modelMapping", {
      sonnet: model,
      opus: model,
      haiku: model,
      fable: model,
      subagent: isCode ? model : "",
    });
  }, [open, watchedDefaultModel, editing, target, form, isCodex]);

  const handleOk = async () => {
    try {
      const values = await form.validateFields();
      let baseUrl = normalizeBaseUrl(values.baseUrl);
      if (isCodex && (values.protocolType === "openai_chat" || values.protocolType === "openai_responses")) {
        baseUrl = ensureOpenAiV1Suffix(baseUrl);
      }
      const normalized = { ...values, baseUrl };
      form.setFieldValue("baseUrl", normalized.baseUrl);
      await onSubmit(normalized);
    } catch {
      // validation errors are shown inline by the form
    }
  };

  const appendV1Suffix = () => {
    const value = form.getFieldValue("baseUrl");
    if (typeof value !== "string" || !value.trim()) return;
    try {
      form.setFieldValue("baseUrl", ensureOpenAiV1Suffix(value));
    } catch {
      // Keep invalid input visible so the form validator can explain it.
    }
  };

  const discoverModels = async () => {
    setDiscovering(true);
    try {
      await form.validateFields(["name", "baseUrl", "apiKey", "protocolType", "targetApp"]);
      const values = form.getFieldsValue(true);
      const normalized = { ...values, baseUrl: normalizeBaseUrl(values.baseUrl) };
      form.setFieldValue("baseUrl", normalized.baseUrl);
      const canPersist =
        editing !== null &&
        normalized.id === editing.id &&
        normalized.baseUrl === editing.baseUrl &&
        normalized.protocolType === editing.protocolType &&
        !normalized.apiKey?.trim() &&
        !normalized.clearApiKey;
      const result = canPersist
        ? await discoverProviderModels(editing.id)
        : await discoverProviderModelsInput(normalized);
      setModels(result.models);
      setModelResult(result);
      if (result.error) {
        void message.warning(`${result.message}: ${result.error}`);
      } else if (!canPersist) {
        void message.info(t("providers.draftModelsNotCached"));
      } else {
        void message.success(result.message);
      }
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
  const modelOptions = [...new Set(isCodex ? [...codexModelSuggestions, ...models] : models)]
    .map((model) => ({ value: model }));
  const modelCacheText = modelResult
    ? modelResult.source === "cache"
      ? t(modelResult.stale ? "providers.modelCacheStale" : "providers.modelCacheFresh", {
          time: new Date(modelResult.checkedAt).toLocaleString(),
        })
      : modelResult.source === "network"
        ? t("providers.modelCacheUpdated", {
            time: new Date(modelResult.checkedAt).toLocaleString(),
          })
        : modelResult.message
    : null;

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
            options={isCodex
              ? [
                  { value: "openai_responses", label: t("providers.protocolOpenAiResponses") },
                  { value: "openai_chat", label: t("providers.protocolOpenAiChat") },
                ]
              : [
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
            <Space direction="vertical" size={2}>
              {endpointPreview ? (
                <>
                  <Typography.Text type="secondary">{t("providers.endpointPreview")}</Typography.Text>
                  <Typography.Text code copyable>{endpointPreview}</Typography.Text>
                </>
              ) : (
                <Typography.Text type="secondary">{t("providers.baseUrlHint")}</Typography.Text>
              )}
              {isCodex
                && (watchedProtocol === "openai_chat" || watchedProtocol === "openai_responses")
                && typeof watchedBaseUrl === "string"
                && needsOpenAiV1Suffix(watchedBaseUrl) && (
                <Button type="link" size="small" onClick={appendV1Suffix} style={{ paddingInline: 0 }}>
                  {t("providers.appendV1")}
                </Button>
              )}
            </Space>
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
          extra={
            <Space direction="vertical" size={0}>
              <Space size="small" wrap>
                <Button type="link" size="small" loading={testing} onClick={() => void testConnection()}>{t("providers.testConnection")}</Button>
                <Button type="link" size="small" loading={discovering} onClick={() => void discoverModels()}>{t("providers.discoverModels")}</Button>
                {!isCodex && <Button type="link" size="small" onClick={fillAllRoles}>{t("providers.fillAllModels")}</Button>}
              </Space>
              {modelCacheText && (
                <Typography.Text type={modelResult?.stale || modelResult?.error ? "warning" : "secondary"}>
                  {modelCacheText}
                </Typography.Text>
              )}
            </Space>
          }
        >
          <AutoComplete
            options={modelOptions}
            placeholder="model-name"
            filterOption={(input, option) =>
              String(option?.value ?? "").toLowerCase().includes(input.toLowerCase())
            }
          />
        </Form.Item>

        {isCodex && (
          <Form.Item
            name="modelContextWindow"
            label={t("providers.modelContextWindow")}
            extra={t("providers.modelContextWindowHint")}
            rules={[
              {
                type: "number",
                min: 1,
                message: t("providers.invalidContextWindow"),
                transform: (value) => (value === null || value === undefined || value === "" ? undefined : Number(value)),
              },
            ]}
          >
            <InputNumber style={{ width: "100%" }} min={1} step={1000} placeholder="272000" />
          </Form.Item>
        )}

        {isCodex && (
          <Form.Item
            name="autoReviewModelOverride"
            label={t("providers.autoReviewModelOverride")}
            extra={t("providers.autoReviewModelOverrideHint")}
          >
            <AutoComplete
              allowClear
              options={modelOptions}
              placeholder="gpt-5.4-mini"
              filterOption={(input, option) =>
                String(option?.value ?? "").toLowerCase().includes(input.toLowerCase())
              }
            />
          </Form.Item>
        )}

        {!isCodex && <><Typography.Title level={5} style={{ marginBlock: "4px 8px" }}>
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
        ))}</>}
        {!isCodex && <Typography.Paragraph type="secondary">
          {visibleRoleFields.map((role) => {
            const mapped = watchedMapping?.[role.key]?.trim() || watchedDefaultModel || "—";
            return (
              <Typography.Text key={role.key} code style={{ marginInlineEnd: 8 }}>
                {role.label} → {mapped}
              </Typography.Text>
            );
          })}
        </Typography.Paragraph>}

        <Form.Item name="notes" label={t("providers.fieldNotes")}>
          <Input.TextArea rows={2} />
        </Form.Item>
      </Form>
    </Modal>
  );
}
