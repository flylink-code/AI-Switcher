import { useMemo, useState } from "react";
import { Alert, Button, Input, InputNumber, Modal, Segmented, Space, Typography, message } from "antd";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ExpandableSection } from "@/components/common/ExpandableSection";
import { GatewayLimitsCard } from "@/components/proxy/GatewayLimitsCard";
import { GatewayUpstreamPanel } from "@/components/proxy/GatewayUpstreamPanel";
import { errMsg } from "@/lib/errMsg";
import {
  listGatewayCatalogEntries,
  rotateSmartGatewayApiKey,
  setSmartGatewayApiKey,
  setSmartGatewayPort,
  startSmartGateway,
  stopSmartGateway,
  getSmartGatewayStatus,
} from "@/services/providers";

const { Paragraph, Text } = Typography;

type SnippetKind = "sdk" | "openai" | "anthropic" | "responses";

export function SmartGatewaySection() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [port, setPort] = useState(15828);
  const [apiKeyDraft, setApiKeyDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [accessOpen, setAccessOpen] = useState(false);
  const [limitsOpen, setLimitsOpen] = useState(false);
  const [snippetKind, setSnippetKind] = useState<SnippetKind>("sdk");

  const statusQuery = useQuery({
    queryKey: ["smart-gateway-status"],
    queryFn: getSmartGatewayStatus,
  });
  const catalogQuery = useQuery({
    queryKey: ["gateway-catalog-entries", "claude_code"],
    queryFn: () => listGatewayCatalogEntries("claude_code"),
  });

  const status = statusQuery.data;
  const running = status?.running ?? false;
  const apiKey = apiKeyDraft ?? status?.apiKey ?? "";
  const listenUrl = (status?.baseUrl ?? `http://127.0.0.1:${status?.port ?? port}`).replace(/\/+$/, "");
  const openaiUrl = `${listenUrl}/v1`;

  // Same-name models from several providers arrive as separate rows whose public id
  // already carries the provider; a model with one source keeps its bare name.
  const models = useMemo(
    () =>
      (catalogQuery.data ?? []).map((entry) => ({
        id: entry.publicId || entry.displayName,
        label: entry.displayName || entry.publicId,
      })),
    [catalogQuery.data],
  );

  const curlSnippet = useMemo(() => {
    const key = apiKey || "sk-aisw-your-key";
    switch (snippetKind) {
      case "sdk":
        return [
          `OpenAI Base URL: ${openaiUrl}`,
          `Anthropic Base URL: ${listenUrl}`,
          `API Key: ${key}`,
          `Model: 聚合目录里的模型 id`,
        ].join("\n");
      case "anthropic":
        return `curl -s ${listenUrl}/v1/messages \\\n -H "x-api-key: ${key}" \\\n -H "content-type: application/json" \\\n -d '{"model":"<catalog-model-id>","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}'`;
      case "responses":
        return `curl -s ${listenUrl}/v1/responses \\\n -H "Authorization: Bearer ${key}" \\\n -H "x-ai-switcher-target: codex" \\\n -H "content-type: application/json" \\\n -d '{"model":"<catalog-model-id>","input":"hi"}'`;
      case "openai":
        return `curl -s ${openaiUrl}/chat/completions \\\n -H "Authorization: Bearer ${key}" \\\n -H "content-type: application/json" \\\n -d '{"model":"<catalog-model-id>","messages":[{"role":"user","content":"hi"}]}'`;
      default: {
        const _exhaustive: never = snippetKind;
        return _exhaustive;
      }
    }
  }, [apiKey, listenUrl, openaiUrl, snippetKind]);

  const copyText = async (value: string) => {
    try {
      await navigator.clipboard.writeText(value);
      void message.success(t("antigravity.copied", { defaultValue: "已复制" }));
    } catch (error) {
      void message.error(errMsg(error));
    }
  };

  const handleStart = async () => {
    setBusy(true);
    try {
      await setSmartGatewayPort(port);
      const next = await startSmartGateway(port);
      queryClient.setQueryData(["smart-gateway-status"], next);
      void message.success(t("gateway.started", { port: next.port, defaultValue: "智能网关已启动 :{{port}}" }));
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setBusy(false);
    }
  };

  const handleStop = async () => {
    setBusy(true);
    try {
      const next = await stopSmartGateway();
      queryClient.setQueryData(["smart-gateway-status"], next);
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Space direction="vertical" size={16} style={{ width: "100%" }}>
      <div className="page-strip" role="group" aria-label={t("gateway.tabSmart", { defaultValue: "智能网关" })}>
        <div className="page-strip-main">
          <span className="page-strip-status" data-tone={running ? "ok" : "idle"}>
            <span className="page-strip-dot" aria-hidden />
            {t("gateway.tabSmart", { defaultValue: "智能网关" })}
            <span className="page-strip-meta">{running ? t("proxy.running") : t("proxy.stopped")}</span>
          </span>
          <span className="overview-stat-chip">:{status?.port ?? 15828}</span>
        </div>
        <div className="page-strip-actions">
          {running ? (
            <Button danger size="small" icon={<StopOutlined />} loading={busy} onClick={() => void handleStop()}>
              {t("proxy.stop")}
            </Button>
          ) : (
            <Button type="primary" size="small" icon={<PlayCircleOutlined />} loading={busy} onClick={() => void handleStart()}>
              {t("proxy.start")}
            </Button>
          )}
          <Button type="text" size="small" className="page-strip-quiet" onClick={() => setAccessOpen(true)}>
            {t("providers.accessInfo", { defaultValue: "接入信息" })}
          </Button>
          <Button type="text" size="small" className="page-strip-quiet" onClick={() => setLimitsOpen(true)}>
            {t("providers.limits", { defaultValue: "限额" })}
          </Button>
        </div>
      </div>
      {status?.lastError ? <Alert type="error" showIcon message={status.lastError} /> : null}
      <GatewayUpstreamPanel allowlistTarget="claude_code" />
      <ExpandableSection title={t("providers.catalogModels", { count: models.length, defaultValue: "模型（{{count}}）" })}>
        <Space direction="vertical" size={4} style={{ width: "100%" }}>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("providers.modelPriorityHint", {
              defaultValue: "多个供应商提供同名模型时，模型 ID 会带供应商前缀，按所选 ID 调用对应供应商；不会自动换来源。",
            })}
          </Text>
          {models.map((model) => (
            <div key={model.id} style={{ display: "flex", justifyContent: "space-between", gap: 8 }}>
              <Text>{model.label}</Text>
              <Text type="secondary" style={{ fontSize: 12 }} copyable>
                {model.id}
              </Text>
            </div>
          ))}
        </Space>
      </ExpandableSection>

      <Modal
        open={accessOpen}
        title={t("providers.accessInfo", { defaultValue: "接入信息" })}
        onCancel={() => setAccessOpen(false)}
        footer={null}
        width={720}
      >
        <Space direction="vertical" size={12} style={{ width: "100%" }}>
          <Input readOnly value={listenUrl} addonBefore={t("gateway.accessUrl", { defaultValue: "访问地址" })} />
          <Input readOnly value={openaiUrl} addonBefore={t("gateway.openaiBaseUrl", { defaultValue: "OpenAI Base URL" })} />
          <Space wrap>
            <InputNumber
              min={1024}
              max={65535}
              value={status?.port ?? port}
              disabled={running}
              onChange={(value) => setPort(value ?? 15828)}
              addonBefore={t("gateway.port", { defaultValue: "端口" })}
            />
            <Input.Password
              style={{ width: 280 }}
              value={apiKey}
              onChange={(event) => setApiKeyDraft(event.target.value)}
              addonBefore="API Key"
            />
            <Button size="small" icon={<CopyOutlined />} disabled={!apiKey} onClick={() => void copyText(apiKey)}>
              {t("gateway.copyApiKey", { defaultValue: "复制 Key" })}
            </Button>
            <Button
              size="small"
              loading={busy}
              onClick={() => {
                setBusy(true);
                void setSmartGatewayApiKey(apiKey)
                  .then((next) => {
                    queryClient.setQueryData(["smart-gateway-status"], next);
                    setApiKeyDraft(null);
                    void message.success(t("gateway.apiKeySaved", { defaultValue: "对外 API Key 已保存" }));
                  })
                  .catch((error: unknown) => message.error(errMsg(error)))
                  .finally(() => setBusy(false));
              }}
            >
              {t("gateway.saveApiKey", { defaultValue: "保存 Key" })}
            </Button>
            <Button
              size="small"
              icon={<ReloadOutlined />}
              loading={busy}
              onClick={() => {
                setBusy(true);
                void rotateSmartGatewayApiKey()
                  .then((next) => {
                    queryClient.setQueryData(["smart-gateway-status"], next);
                    setApiKeyDraft(null);
                    void message.success(t("gateway.apiKeyRotated", { defaultValue: "已生成新的对外 API Key" }));
                  })
                  .catch((error: unknown) => message.error(errMsg(error)))
                  .finally(() => setBusy(false));
              }}
            >
              {t("gateway.rotateApiKey", { defaultValue: "换新 Key" })}
            </Button>
          </Space>
          <Segmented<SnippetKind>
            size="small"
            value={snippetKind}
            onChange={setSnippetKind}
            options={[
              { value: "sdk", label: t("gateway.snippetSdk", { defaultValue: "接入配置" }) },
              { value: "openai", label: "OpenAI Chat" },
              { value: "anthropic", label: "Anthropic" },
              { value: "responses", label: "Responses" },
            ]}
          />
          <Paragraph>
            <pre style={{ margin: 0, whiteSpace: "pre-wrap", fontSize: 12 }}>{curlSnippet}</pre>
          </Paragraph>
          <Button size="small" icon={<CopyOutlined />} onClick={() => void copyText(curlSnippet)}>
            {t("gateway.copySdk", { defaultValue: "复制配置" })}
          </Button>
        </Space>
      </Modal>

      <Modal
        open={limitsOpen}
        title={t("providers.limits", { defaultValue: "限额" })}
        onCancel={() => setLimitsOpen(false)}
        footer={null}
        width={640}
      >
        <GatewayLimitsCard />
      </Modal>
    </Space>
  );
}
