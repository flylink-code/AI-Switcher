import { useMemo, useState } from "react";
import {
  Button,
  Card,
  Input,
  InputNumber,
  Paragraph,
  Select,
  Space,
  Tag,
  Typography,
  message,
} from "antd";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useTranslation } from "react-i18next";
import type { AntigravityGatewayStatus, CatalogModel } from "@/services/api";

const { Text } = Typography;

interface GatewayCardProps {
  status?: AntigravityGatewayStatus;
  models?: CatalogModel[];
  onStartGateway: (port: number, apiKey?: string, outboundMode?: string, outboundUrl?: string) => Promise<void>;
  onStopGateway: () => Promise<void>;
  onSaveOutbound: (mode: "direct" | "system" | "custom", url: string) => Promise<void>;
  onRefresh: () => void;
  isStarting?: boolean;
  isStopping?: boolean;
  isSavingOutbound?: boolean;
}

export function GatewayCard({
  status,
  models,
  onStartGateway,
  onStopGateway,
  onSaveOutbound,
  onRefresh,
  isStarting = false,
  isStopping = false,
  isSavingOutbound = false,
}: GatewayCardProps) {
  const { t } = useTranslation();

  const [portDraft, setPortDraft] = useState<number | null>(null);
  const [apiKeyDraft, setApiKeyDraft] = useState<string | null>(null);
  const [outboundModeDraft, setOutboundModeDraft] = useState<
    "direct" | "system" | "custom" | null
  >(null);
  const [outboundUrlDraft, setOutboundUrlDraft] = useState<string | null>(null);

  const port = portDraft ?? status?.port ?? 15830;
  const apiKey = apiKeyDraft ?? status?.apiKey ?? "";
  const outboundMode =
    outboundModeDraft ??
    (status?.outboundMode === "direct" || status?.outboundMode === "system"
      ? status.outboundMode
      : "custom");
  const outboundUrl =
    outboundUrlDraft ?? status?.outboundProxyUrl ?? "socks5://127.0.0.1:17891";

  const sampleModel =
    models?.find((model) => model.id === "claude-sonnet-4-6")?.id ??
    models?.[0]?.id ??
    "claude-sonnet-4-6";

  const curlSnippet = useMemo(() => {
    const base = status?.baseUrl ?? `http://127.0.0.1:${port}`;
    const key = apiKey || "sk-ai-switcher-antigravity";
    return `curl -s ${base}/v1/messages \\\n  -H "x-api-key: ${key}" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"${sampleModel}","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}'`;
  }, [apiKey, port, sampleModel, status?.baseUrl]);

  return (
    <Card title={t("antigravity.gateway")} size="small" style={{ marginBottom: 16 }}>
      <Space direction="vertical" style={{ width: "100%" }} size={12}>
        <Space wrap>
          <Tag color={status?.running ? "success" : "default"}>
            {status?.running ? t("antigravity.running") : t("antigravity.stoppedState")}
          </Tag>
          <Text type="secondary">
            {t("antigravity.accountsCount", { count: status?.accountCount ?? 0 })}
          </Text>
          <Text code>{status?.baseUrl ?? `http://127.0.0.1:${port}`}</Text>
        </Space>

        <Space wrap>
          <InputNumber
            min={1024}
            max={65535}
            value={port}
            onChange={(value) => setPortDraft(typeof value === "number" ? value : null)}
            addonBefore={t("antigravity.port")}
          />
          <Input.Password
            style={{ width: 280 }}
            value={apiKey}
            onChange={(event) => setApiKeyDraft(event.target.value)}
            placeholder="sk-..."
            addonBefore="API Key"
          />
        </Space>

        <Space direction="vertical" size={8} style={{ width: "100%" }}>
          <Text type="secondary">{t("antigravity.outboundHint")}</Text>
          <Space wrap>
            <Select
              style={{ minWidth: 200 }}
              value={outboundMode}
              onChange={(value) => setOutboundModeDraft(value)}
              options={[
                { value: "custom", label: t("antigravity.outboundCustom") },
                { value: "direct", label: t("antigravity.outboundDirect") },
                { value: "system", label: t("antigravity.outboundSystem") },
              ]}
            />
            <Input
              style={{ width: 280 }}
              disabled={outboundMode !== "custom"}
              value={outboundUrl}
              onChange={(event) => setOutboundUrlDraft(event.target.value)}
              placeholder="socks5://127.0.0.1:17891"
              addonBefore={t("antigravity.outboundProxy")}
            />
            <Button
              loading={isSavingOutbound}
              onClick={() => onSaveOutbound(outboundMode, outboundUrl)}
            >
              {t("antigravity.outboundSave")}
            </Button>
          </Space>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("antigravity.outboundEffective", {
              value: status?.effectiveOutboundProxy || t("antigravity.outboundNone"),
            })}
          </Text>
        </Space>

        <Space wrap style={{ marginTop: 4 }}>
          <Button
            type="primary"
            icon={<PlayCircleOutlined />}
            loading={isStarting}
            onClick={() => onStartGateway(port, apiKey, outboundMode, outboundUrl)}
          >
            {t("antigravity.start")}
          </Button>
          <Button
            icon={<StopOutlined />}
            loading={isStopping}
            onClick={() => onStopGateway()}
          >
            {t("antigravity.stop")}
          </Button>
          <Button icon={<ReloadOutlined />} onClick={onRefresh}>
            {t("common.refresh")}
          </Button>
          <Button
            icon={<CopyOutlined />}
            onClick={() => {
              void navigator.clipboard.writeText(curlSnippet).then(() => {
                message.success(t("antigravity.copied"));
              });
            }}
          >
            {t("antigravity.copyCurl")}
          </Button>
        </Space>

        <Paragraph style={{ marginBottom: 0 }}>
          <pre style={{ margin: 0, padding: 8, borderRadius: 6, background: "var(--ant-color-bg-layout, #f5f5f5)", whiteSpace: "pre-wrap", fontSize: 12 }}>
            {curlSnippet}
          </pre>
        </Paragraph>
      </Space>
    </Card>
  );
}
