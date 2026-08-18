import { useEffect, useRef, useState } from "react";
import {
  App,
  Alert,
  AutoComplete,
  Button,
  Col,
  Form,
  Checkbox,
  Input,
  InputNumber,
  Modal,
  Row,
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
  getAntigravityDefaults,
  getAntigravityGatewayStatus,
  getCachedProviderModels,
  testProviderInput,
} from "@/services/api";
import {
  mappingFromAntigravityPreset,
  mappingFromModel,
  presetsForTarget,
  syncMappingOnDefaultChange,
  type ProviderPreset,
} from "@/lib/providerPresets";

interface ProviderFormProps {
  open: boolean;
  /** When editing, the provider being edited; when null, creating. */
  editing: Provider | null;
  target: ProviderTarget;
  /** Unified catalog: hide Claude role mapping, keep existing DB mapping on save. */
  gatewayCatalog?: boolean;
  /** Shown after copying a provider from another Agent. */
  importHint?: string | null;
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

function isLocalHttpHost(hostname: string): boolean {
  const host = hostname.toLowerCase();
  return host === "localhost" || host === "127.0.0.1" || host === "[::1]" || host === "::1";
}

/** Validate and convert a pasted request endpoint into a reusable Base URL. */
function normalizeBaseUrl(value: string): string {
  const trimmed = value.trim();
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    throw new Error("invalidBaseUrl");
  }
  const allowLocalHttp = url.protocol === "http:" && isLocalHttpHost(url.hostname);
  if (url.protocol !== "https:" && !allowLocalHttp) throw new Error("baseUrlMustUseHttps");
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

const EMPTY_MODEL_MAPPING: ClaudeModelMapping = {
  sonnet: "",
  opus: "",
  haiku: "",
  fable: "",
  subagent: "",
};

function uniqueModelIds(ids: Array<string | undefined | null>): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const raw of ids) {
    const id = String(raw ?? "").trim();
    if (!id) continue;
    const key = id.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(id);
  }
  return result;
}

export function ProviderForm({
  open,
  editing,
  target,
  gatewayCatalog = false,
  importHint,
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
  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(null);
  const watchedBaseUrl = Form.useWatch("baseUrl", form);
  const watchedProtocol = Form.useWatch("protocolType", form) ?? "anthropic";
  const watchedDefaultModel = Form.useWatch("model", form) ?? "";
  const watchedFailoverModels = Form.useWatch("failoverModels", form) ?? [];
  const watchedHiddenModels: string[] = Form.useWatch("hiddenModels", form) ?? [];
  const watchedMapping = Form.useWatch("modelMapping", form) ?? EMPTY_MODEL_MAPPING;
  const watchedProviderKind = Form.useWatch("providerKind", form) ?? "standard";
  const watchedThinkingMode = Form.useWatch(["thinkingConfig", "mode"], form) ?? "auto";
  const endpointPreview = buildEndpointPreview(watchedBaseUrl, watchedProtocol);
  let nameRef: InputRef | null = null;

  const isEdit = editing !== null;
  const isCodex = (editing?.targetApp ?? target) === "codex";
  const isOpenCode = (editing?.targetApp ?? target) === "opencode";
  const isPi = (editing?.targetApp ?? target) === "pi";
  const isDsh = (editing?.targetApp ?? target) === "dsh";
  // Codex / OpenCode / Pi / Dsh 都不使用 Claude 的 Sonnet/Opus/Haiku 角色映射。
  const isDirect = isCodex || isOpenCode || isPi || isDsh;
  const hideRoleMapping = isDirect || gatewayCatalog;
  const mappingTarget = (editing?.targetApp ?? target) === "claude_code" ? "claude_code" : "claude_desktop";
  // Seed with the loaded default so the sync effect never treats open/edit as a "change".
  const prevModelRef = useRef<string>("");

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setModelResult(null);
    if (editing) {
      setModels([]);
      setSelectedPresetId(null);
      prevModelRef.current = editing.model?.trim() ?? "";
      form.setFieldsValue({
        id: editing.id,
        name: editing.name,
        baseUrl: editing.baseUrl,
        apiKey: "",
        clearApiKey: false,
        model: editing.model,
        modelContextWindow: editing.modelContextWindow ?? undefined,
        autoReviewModelOverride: editing.autoReviewModelOverride ?? undefined,
        webSearchEnabled: editing.webSearchEnabled ?? true,
        modelMapping: editing.modelMapping,
        protocolType: editing.protocolType,
        providerKind: editing.providerKind,
        authBinding: editing.authBinding,
        notes: editing.notes,
        targetApp: editing.targetApp,
        failoverGroup: editing.failoverGroup ?? 0,
        failoverModels: editing.failoverModels ?? [],
        hiddenModels: editing.hiddenModels ?? [],
        thinkingConfig: editing.thinkingConfig ?? {
          mode: "auto",
          budgetTokens: undefined,
          reasoningEffort: undefined,
          prefixThought: true,
        },
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
      setSelectedPresetId(null);
      prevModelRef.current = "";
      form.resetFields();
      form.setFieldsValue({
        protocolType: (target === "codex" ? "openai_responses" : "anthropic") as ProtocolType,
        providerKind: "standard",
        authBinding: "",
        webSearchEnabled: true,
        targetApp: target,
        failoverGroup: 0,
        failoverModels: [],
        hiddenModels: [],
        thinkingConfig: {
          mode: "auto",
          budgetTokens: undefined,
          reasoningEffort: undefined,
          prefixThought: true,
        },
        modelMapping: { ...EMPTY_MODEL_MAPPING },
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
    if (hideRoleMapping) return;
    const model = watchedDefaultModel?.trim() ?? "";
    if (!model || model === prevModelRef.current) return;
    const previous = prevModelRef.current;
    prevModelRef.current = model;
    const nextMapping = syncMappingOnDefaultChange(
      form.getFieldValue("modelMapping"),
      previous,
      model,
      mappingTarget,
    );
    form.setFieldValue("modelMapping", nextMapping);
  }, [open, watchedDefaultModel, mappingTarget, form, hideRoleMapping]);

  const handleOk = async () => {
    try {
      const values = await form.validateFields();
      let baseUrl = normalizeBaseUrl(values.baseUrl);
      if (isDirect && (values.protocolType === "openai_chat" || values.protocolType === "openai_responses")) {
        baseUrl = ensureOpenAiV1Suffix(baseUrl);
      }
      const defaultModel = String(values.model ?? "").trim();
      const mapping = isDirect
        ? { ...EMPTY_MODEL_MAPPING }
        : (values.modelMapping ?? { ...EMPTY_MODEL_MAPPING });
      const hiddenModels = uniqueModelIds(values.hiddenModels ?? []).filter(
        (id) => id.toLowerCase() !== defaultModel.toLowerCase(),
      );
      const normalized = {
        ...values,
        baseUrl,
        modelMapping: mapping,
        hiddenModels,
      };
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
  const modelOptions = [
    ...new Set(
      [
        ...(isCodex ? codexModelSuggestions : []),
        ...models,
        ...(watchedFailoverModels ?? []),
      ].filter((model) => model?.trim()),
    ),
  ].map((model) => ({ value: model }));
  const candidateModels = uniqueModelIds([
    watchedDefaultModel,
    ...models,
    ...(watchedFailoverModels ?? []),
    watchedMapping.sonnet,
    watchedMapping.opus,
    watchedMapping.haiku,
    watchedMapping.fable,
    watchedMapping.subagent,
  ]);
  const defaultModelKey = String(watchedDefaultModel ?? "").trim().toLowerCase();
  const visibleModelValue = candidateModels.filter((id) => {
    if (id.toLowerCase() === defaultModelKey) return true;
    return !watchedHiddenModels.some((hidden) => hidden.trim().toLowerCase() === id.toLowerCase());
  });
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
    form.setFieldValue("modelMapping", mappingFromModel(model, mappingTarget));
  };

  const clearRoleMapping = () => {
    form.setFieldValue("modelMapping", { ...EMPTY_MODEL_MAPPING });
  };

  const applyPreset = (presetId: string) => {
    const preset = presetsForTarget(target).find((item) => item.id === presetId);
    if (!preset) return;
    setSelectedPresetId(presetId);
    applyPresetValues(preset);
  };

  const clearPreset = () => {
    setSelectedPresetId(null);
    setModels([]);
    setModelResult(null);
    prevModelRef.current = "";
    form.resetFields();
    form.setFieldsValue({
      protocolType: (target === "codex" ? "openai_responses" : "anthropic") as ProtocolType,
      providerKind: "standard",
      authBinding: "",
      webSearchEnabled: true,
      targetApp: target,
      modelMapping: { ...EMPTY_MODEL_MAPPING },
    });
  };

  const applyPresetValues = (preset: ProviderPreset) => {
    prevModelRef.current = preset.model.trim();
    const isAgPreset =
      preset.id === "antigravity-builtin" ||
      preset.id === "antigravity-builtin-codex" ||
      preset.id === "antigravity-builtin-pi" ||
      preset.id === "antigravity-builtin-dsh" ||
      preset.id === "antigravity-gateway-external" ||
      preset.id === "antigravity-gateway-external-codex" ||
      preset.id === "antigravity-gateway-external-pi" ||
      preset.id === "antigravity-gateway-external-dsh";
    const isBuiltinAg =
      preset.id === "antigravity-builtin" ||
      preset.id === "antigravity-builtin-codex" ||
      preset.id === "antigravity-builtin-pi" ||
      preset.id === "antigravity-builtin-dsh";
    form.setFieldsValue({
      name: preset.name,
      protocolType: preset.protocolType,
      baseUrl: preset.baseUrl,
      model: preset.model,
      modelContextWindow: preset.modelContextWindow,
      failoverModels: preset.failoverModels ?? [],
      webSearchEnabled: true,
      providerKind: isBuiltinAg ? "antigravity" : "standard",
      authBinding: "",
      notes: preset.notes ?? "",
      targetApp: target,
      modelMapping: isDirect
        ? { ...EMPTY_MODEL_MAPPING }
        : isAgPreset
          ? mappingFromAntigravityPreset(preset.model, target)
          : mappingFromModel(preset.model, target),
    });
    // Seed the suggestion list with the preset's own models so the user can
    // switch between e.g. deepseek-v4-flash / deepseek-v4-pro immediately.
    setModels([...new Set([preset.model, ...(preset.failoverModels ?? [])])]);
    setModelResult(null);

    if (isBuiltinAg || isAgPreset) {
      void getAntigravityDefaults()
        .then((defaults) => {
          const liveIds = (defaults.models ?? []).map((model) => model.id).filter(Boolean);
          const defaultModel = defaults.defaultModel?.trim() || preset.model;
          const flash = defaults.geminiFlash ?? "gemini-3.7-flash";
          const flashLow = defaults.geminiFlashLow ?? "gemini-3.6-flash-low";
          const pro = defaults.geminiPro ?? flash;
          const failover = liveIds.filter((id) => id !== defaultModel);
          const liveRoot = String(defaults.baseUrl || "http://127.0.0.1:15830").replace(/\/$/, "");
          const needsV1 =
            preset.id === "antigravity-builtin-codex" ||
            target === "opencode";
          const baseUrl = isBuiltinAg
            ? needsV1
              ? `${liveRoot}/v1`
              : liveRoot
            : preset.baseUrl;
          form.setFieldsValue({
            baseUrl: isBuiltinAg ? baseUrl : preset.baseUrl,
            apiKey: isBuiltinAg ? defaults.apiKey : form.getFieldValue("apiKey"),
            providerKind: isBuiltinAg ? "antigravity" : form.getFieldValue("providerKind"),
            model: defaultModel,
            failoverModels: failover.length > 0 ? failover : preset.failoverModels ?? [],
            modelMapping: isDirect
              ? { ...EMPTY_MODEL_MAPPING }
              : mappingFromAntigravityPreset(defaultModel, target, {
                  geminiFlash: flashLow,
                  geminiPro: pro,
                }),
          });
          prevModelRef.current = defaultModel;
          setModels([...new Set([defaultModel, ...failover, ...(preset.failoverModels ?? [])])]);
        })
        .catch(() => {
          if (isBuiltinAg) {
            void getAntigravityGatewayStatus()
              .then((status) => {
                const liveRoot = status.baseUrl.replace(/\/$/, "");
                const needsV1 =
                  preset.id === "antigravity-builtin-codex" ||
                  target === "opencode";
                const baseUrl = needsV1 ? `${liveRoot}/v1` : status.baseUrl;
                form.setFieldsValue({
                  baseUrl,
                  apiKey: status.apiKey,
                  providerKind: "antigravity",
                });
              })
              .catch(() => {
                // Keep preset defaults when gateway status is unavailable.
              });
          }
        });
    }
  };

  const createPresets = presetsForTarget(target);

  return (
    <Modal
      open={open}
      title={isEdit ? t("providers.editTitle") : t("providers.createTitle")}
      okText={t("providers.save")}
      cancelText={t("providers.cancel")}
      onCancel={onCancel}
      onOk={handleOk}
      destroyOnHidden
      width={680}
      styles={{ body: { maxHeight: "calc(100vh - 200px)", overflowY: "auto", paddingInlineEnd: 4 } }}
    >
      <Form form={form} layout="vertical" autoComplete="off">
        {importHint ? (
          <Alert
            type="info"
            showIcon
            style={{ marginBottom: 12 }}
            message={importHint}
          />
        ) : null}
        <Form.Item name="id" hidden>
          <Input />
        </Form.Item>
        <Form.Item name="providerKind" hidden>
          <Input />
        </Form.Item>
        <Form.Item name="authBinding" hidden>
          <Input />
        </Form.Item>

        {!isEdit && createPresets.length > 0 ? (
          <Form.Item
            label={t("providers.fromPreset")}
            extra={t("providers.fromPresetHint")}
          >
            <Space wrap size={[8, 8]}>
              <Button
                size="small"
                type={selectedPresetId === null ? "primary" : "default"}
                onClick={clearPreset}
              >
                {t("providers.blankPreset")}
              </Button>
              {createPresets.map((preset) => (
                <Button
                  key={preset.id}
                  size="small"
                  type={selectedPresetId === preset.id ? "primary" : "default"}
                  onClick={() => applyPreset(preset.id)}
                >
                  {preset.name}
                </Button>
              ))}
            </Space>
          </Form.Item>
        ) : null}

        {isPi ? (
          <Typography.Paragraph type="secondary" style={{ marginTop: -8, marginBottom: 12, fontSize: 12 }}>
            {t("providers.piDirectHint")}
          </Typography.Paragraph>
        ) : null}
        {isDsh ? (
          <Typography.Paragraph type="secondary" style={{ marginTop: -8, marginBottom: 12, fontSize: 12 }}>
            {t("providers.dshDirectHint")}
          </Typography.Paragraph>
        ) : null}

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
          extra={isCodex && watchedProtocol === "anthropic" ? t("providers.protocolAnthropicCodexHint") : undefined}
          rules={[{ required: true }]}
        >
          <Select
            options={isCodex
              ? [
                  { value: "openai_responses", label: t("providers.protocolOpenAiResponses") },
                  { value: "openai_chat", label: t("providers.protocolOpenAiChat") },
                  { value: "anthropic", label: t("providers.protocolAnthropicCodex") },
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
              {isDirect
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

        {watchedProviderKind !== "codex_oauth" && <Form.Item
          name="apiKey"
          label={t("providers.fieldApiKey")}
          extra={editing?.apiKeySet ? t("providers.keyStored") : undefined}
        >
          <Input.Password placeholder="sk-..." autoComplete="new-password" />
        </Form.Item>}

        {watchedProviderKind !== "codex_oauth" && editing?.apiKeySet && (
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

        {(isCodex || isOpenCode || isPi || isDsh) && (
          <Form.Item
            name="modelContextWindow"
            label={t("providers.modelContextWindow")}
            extra={isCodex ? t("providers.modelContextWindowHint") : t("providers.modelContextWindowHintCatalog")}
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
            name="webSearchEnabled"
            valuePropName="checked"
            extra={t("providers.webSearchEnabledHint")}
          >
            <Checkbox>{t("providers.webSearchEnabled")}</Checkbox>
          </Form.Item>
        )}

        {isCodex && !gatewayCatalog && (
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

        <Form.Item name="hiddenModels" hidden>
          <Select mode="multiple" />
        </Form.Item>
        <Form.Item
          label={t("providers.visibleModels")}
          extra={t("providers.visibleModelsHint")}
        >
          <Select
            mode="multiple"
            value={visibleModelValue}
            options={candidateModels.map((id) => ({
              value: id,
              label: id,
              disabled: id.toLowerCase() === defaultModelKey,
            }))}
            placeholder={t("providers.visibleModels")}
            style={{ width: "100%" }}
            onChange={(visible: string[]) => {
              const nextHidden = uniqueModelIds([
                ...watchedHiddenModels.filter(
                  (id) => !candidateModels.some((candidate) => candidate.toLowerCase() === id.trim().toLowerCase()),
                ),
                ...candidateModels.filter((id) => !visible.includes(id)),
              ]).filter((id) => id.toLowerCase() !== defaultModelKey);
              form.setFieldValue("hiddenModels", nextHidden);
            }}
          />
        </Form.Item>

        {!hideRoleMapping && (
          <>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 8,
                marginBlock: "4px 4px",
                flexWrap: "wrap",
              }}
            >
              <Typography.Title level={5} style={{ margin: 0 }}>
                {t("providers.modelMapping")}
              </Typography.Title>
              <Space size="small" wrap>
                <Button size="small" onClick={fillAllRoles}>{t("providers.fillAllModels")}</Button>
                <Button size="small" onClick={clearRoleMapping}>{t("providers.clearModelMapping")}</Button>
              </Space>
            </div>
            <Typography.Paragraph type="secondary" style={{ marginBottom: 10, fontSize: 12 }}>
              {t("providers.modelMappingHint")}
            </Typography.Paragraph>
            <Row gutter={[12, 0]}>
              {visibleRoleFields.map((role) => (
                <Col key={role.key} xs={24} sm={12}>
                  <Form.Item
                    name={["modelMapping", role.key]}
                    label={role.label}
                    style={{ marginBottom: 12 }}
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
                </Col>
              ))}
            </Row>
          </>
        )}

        <Typography.Title level={5} style={{ marginBlock: "8px 4px" }}>
          {t("providers.thinkingSection")}
        </Typography.Title>
        <Typography.Paragraph type="secondary" style={{ marginBottom: 10, fontSize: 12 }}>
          {t("providers.thinkingSectionHint")}
        </Typography.Paragraph>
        <Row gutter={[12, 0]}>
          <Col xs={24} sm={8}>
            <Form.Item
              name={["thinkingConfig", "mode"]}
              label={t("providers.fieldThinkingMode")}
            >
              <Select
                options={[
                  { value: "auto", label: t("providers.thinkingModeAuto") },
                  { value: "budget", label: t("providers.thinkingModeBudget") },
                  { value: "effort", label: t("providers.thinkingModeEffort") },
                  { value: "disabled", label: t("providers.thinkingModeDisabled") },
                ]}
              />
            </Form.Item>
          </Col>
          {watchedThinkingMode !== "disabled" && (
            <>
              <Col xs={24} sm={8}>
                <Form.Item
                  name={["thinkingConfig", "budgetTokens"]}
                  label={t("providers.fieldBudgetTokens")}
                  extra={t("providers.budgetTokensHint")}
                >
                  <InputNumber
                    min={1024}
                    max={131072}
                    step={1024}
                    placeholder="16000"
                    style={{ width: "100%" }}
                  />
                </Form.Item>
              </Col>
              <Col xs={24} sm={8}>
                <Form.Item
                  name={["thinkingConfig", "reasoningEffort"]}
                  label={t("providers.fieldReasoningEffort")}
                  extra={t("providers.reasoningEffortHint")}
                >
                  <Select
                    allowClear
                    placeholder="medium"
                    options={[
                      { value: "low", label: t("providers.reasoningEffortLow") },
                      { value: "medium", label: t("providers.reasoningEffortMedium") },
                      { value: "high", label: t("providers.reasoningEffortHigh") },
                    ]}
                  />
                </Form.Item>
              </Col>
            </>
          )}
        </Row>
        {watchedThinkingMode !== "disabled" && (
          <Form.Item
            name={["thinkingConfig", "prefixThought"]}
            valuePropName="checked"
            extra={t("providers.fieldPrefixThoughtHint")}
            style={{ marginBottom: 12 }}
          >
            <Checkbox>{t("providers.fieldPrefixThought")}</Checkbox>
          </Form.Item>
        )}

        <Typography.Title level={5} style={{ marginBlock: "4px 8px" }}>
          {t("providers.failoverSection")}
        </Typography.Title>
        <Typography.Paragraph type="secondary" style={{ marginBottom: 10, fontSize: 12 }}>
          {t("providers.failoverSectionHint")}
        </Typography.Paragraph>
        <Row gutter={[12, 0]}>
          <Col xs={24} sm={8}>
            <Form.Item
              name="failoverGroup"
              label={t("providers.fieldFailoverGroup")}
              extra={t("providers.failoverGroupHint")}
            >
              <InputNumber min={0} max={99} style={{ width: "100%" }} />
            </Form.Item>
          </Col>
          <Col xs={24} sm={16}>
            <Form.Item
              name="failoverModels"
              label={t("providers.fieldFailoverModels")}
              extra={t("providers.failoverModelsHint")}
            >
              <Select
                mode="tags"
                tokenSeparators={[","]}
                placeholder={t("providers.failoverModelsPlaceholder")}
                style={{ width: "100%" }}
              />
            </Form.Item>
          </Col>
        </Row>

        <Form.Item name="notes" label={t("providers.fieldNotes")}>
          <Input.TextArea rows={2} />
        </Form.Item>
      </Form>
    </Modal>
  );
}
