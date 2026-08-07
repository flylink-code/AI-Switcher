import { useMemo, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Input,
  InputNumber,
  Select,
  Space,
  Table,
  Tag,
  Typography,
  message,
} from "antd";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import LoginOutlined from "@ant-design/icons/es/icons/LoginOutlined";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  ensureAntigravityProvider,
  getAntigravityDefaults,
  getAntigravityGatewayStatus,
  importAntigravityAccounts,
  listAntigravityAccounts,
  listAntigravityModels,
  refreshAntigravityQuotas,
  removeAntigravityAccount,
  setAntigravityActiveAccount,
  setAntigravityGatewayApiKey,
  setAntigravityGatewayPort,
  setAntigravityOutboundProxy,
  setAntigravityReasoningLevel,
  startAntigravityGateway,
  startAntigravityOauthLogin,
  stopAntigravityGateway,
  type AntigravityAccountPublic,
} from "@/services/api";
import {
  QuotaMiniBar,
  accountQuotaSummary,
  formatTierLabel,
  tierTagColor,
} from "@/components/AntigravityQuotaBars";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";

const { Text, Paragraph, Title } = Typography;
const { TextArea } = Input;

function errMsg(error: unknown): string {
  if (typeof error === "string" && error.trim()) return error;
  if (error instanceof Error && error.message.trim()) return error.message;
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return message;
  }
  return String(error ?? "未知错误");
}

export default function AntigravityPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const workspaceTarget = usePagePreferencesStore((s) => s.workspaceTarget);
  const [importJson, setImportJson] = useState("");
  const [portDraft, setPortDraft] = useState<number | null>(null);
  const [apiKeyDraft, setApiKeyDraft] = useState<string | null>(null);
  const [outboundModeDraft, setOutboundModeDraft] = useState<
    "direct" | "system" | "custom" | null
  >(null);
  const [outboundUrlDraft, setOutboundUrlDraft] = useState<string | null>(null);

  const accountsQuery = useQuery({
    queryKey: ["antigravity-accounts"],
    queryFn: listAntigravityAccounts,
  });
  const statusQuery = useQuery({
    queryKey: ["antigravity-gateway"],
    queryFn: getAntigravityGatewayStatus,
    refetchInterval: 5_000,
  });
  const modelsQuery = useQuery({
    queryKey: ["antigravity-models"],
    queryFn: listAntigravityModels,
  });
  const defaultsQuery = useQuery({
    queryKey: ["antigravity-defaults"],
    queryFn: getAntigravityDefaults,
  });

  const status = statusQuery.data;
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
    modelsQuery.data?.find((model) => model.id === "claude-sonnet-4-6")?.id ??
    modelsQuery.data?.[0]?.id ??
    "claude-sonnet-4-6";

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["antigravity-accounts"] }),
      queryClient.invalidateQueries({ queryKey: ["antigravity-gateway"] }),
      queryClient.invalidateQueries({ queryKey: ["antigravity-models"] }),
    ]);
  };

  const oauthMutation = useMutation({
    mutationFn: startAntigravityOauthLogin,
    onSuccess: async (account) => {
      message.success(t("antigravity.oauthSuccess", { email: account.email }));
      await refresh();
    },
    onError: (error: unknown) => {
      message.error(errMsg(error), 10);
    },
  });

  const importMutation = useMutation({
    mutationFn: () => importAntigravityAccounts(importJson),
    onSuccess: async (count) => {
      message.success(t("antigravity.importSuccess", { count }));
      setImportJson("");
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const startMutation = useMutation({
    mutationFn: async () => {
      if (portDraft != null) await setAntigravityGatewayPort(port);
      if (apiKeyDraft != null && apiKeyDraft.trim()) {
        await setAntigravityGatewayApiKey(apiKeyDraft.trim());
      }
      if (outboundModeDraft != null || outboundUrlDraft != null) {
        await setAntigravityOutboundProxy(outboundMode, outboundUrl);
      }
      return startAntigravityGateway(port);
    },
    onSuccess: async () => {
      message.success(t("antigravity.started"));
      setPortDraft(null);
      setApiKeyDraft(null);
      setOutboundModeDraft(null);
      setOutboundUrlDraft(null);
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const outboundMutation = useMutation({
    mutationFn: () => setAntigravityOutboundProxy(outboundMode, outboundUrl),
    onSuccess: async () => {
      message.success(t("antigravity.outboundSaved"));
      setOutboundModeDraft(null);
      setOutboundUrlDraft(null);
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const reasoningLevel =
    (defaultsQuery.data?.reasoningLevel as "low" | "medium" | "high" | null | undefined) ??
    null;
  const levelMutation = useMutation({
    mutationFn: (level: "low" | "medium" | "high" | null) =>
      setAntigravityReasoningLevel(level),
    onSuccess: async () => {
      message.success(t("antigravity.reasoningLevelSaved"));
      await queryClient.invalidateQueries({ queryKey: ["antigravity-defaults"] });
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const stopMutation = useMutation({
    mutationFn: stopAntigravityGateway,
    onSuccess: async () => {
      message.success(t("antigravity.stopped"));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const ensureMutation = useMutation({
    mutationFn: () => ensureAntigravityProvider(workspaceTarget),
    onSuccess: async () => {
      message.success(t("antigravity.providerReady"));
      await refresh();
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const quotaMutation = useMutation({
    mutationFn: refreshAntigravityQuotas,
    onSuccess: async () => {
      message.success(t("antigravity.quotaRefreshed"));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error), 10),
  });

  const curlSnippet = useMemo(() => {
    const base = status?.baseUrl ?? `http://127.0.0.1:${port}`;
    const key = apiKey || "sk-ai-switcher-antigravity";
    return `curl -s ${base}/v1/messages \\\n  -H "x-api-key: ${key}" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"${sampleModel}","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}'`;
  }, [apiKey, port, sampleModel, status?.baseUrl]);

  const columns = [
    {
      title: t("antigravity.email"),
      dataIndex: "email",
      key: "email",
      render: (_: unknown, row: AntigravityAccountPublic) => {
        const tier = formatTierLabel(row.subscriptionTier);
        const cooling =
          row.cooldownUntil != null && row.cooldownUntil * 1000 > Date.now();
        return (
          <Space direction="vertical" size={2}>
            <Space wrap>
              <Text>{row.email}</Text>
              {tier && <Tag color={tierTagColor(row.subscriptionTier)}>{tier}</Tag>}
              {row.isActive && <Tag color="green">{t("antigravity.active")}</Tag>}
              {row.disabled && <Tag color="red">{t("antigravity.disabled")}</Tag>}
              {cooling && <Tag color="orange">{t("antigravity.cooling")}</Tag>}
              {row.quotaForbidden && <Tag color="red">{t("antigravity.forbidden")}</Tag>}
            </Space>
          </Space>
        );
      },
    },
    {
      title: t("antigravity.quota"),
      key: "quota",
      width: 280,
      render: (_: unknown, row: AntigravityAccountPublic) => {
        const { geminiFiveHour, geminiWeekly, claudeFiveHour, claudeWeekly } =
          accountQuotaSummary(row);
        return (
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            <QuotaMiniBar
              label={t("antigravity.quotaGemini5h")}
              percent={geminiFiveHour}
            />
            <QuotaMiniBar
              label={t("antigravity.quotaGemini7d")}
              percent={geminiWeekly}
            />
            <QuotaMiniBar
              label={t("antigravity.quotaClaude5h")}
              percent={claudeFiveHour}
            />
            <QuotaMiniBar
              label={t("antigravity.quotaClaude7d")}
              percent={claudeWeekly}
            />
          </div>
        );
      },
    },
    {
      title: t("antigravity.health"),
      dataIndex: "healthScore",
      key: "health",
      width: 90,
      render: (value: number) => `${Math.round(value * 100)}%`,
    },
    {
      title: t("antigravity.project"),
      dataIndex: "hasProjectId",
      key: "project",
      width: 90,
      render: (value: boolean) =>
        value ? <Tag color="blue">OK</Tag> : <Tag>{t("antigravity.pending")}</Tag>,
    },
    {
      title: t("antigravity.actions"),
      key: "actions",
      width: 200,
      render: (_: unknown, row: AntigravityAccountPublic) => (
        <Space>
          <Button
            size="small"
            disabled={row.isActive || row.disabled}
            onClick={() => {
              void setAntigravityActiveAccount(row.id)
                .then(refresh)
                .catch((error: unknown) => message.error(errMsg(error)));
            }}
          >
            {t("antigravity.setActive")}
          </Button>
          <Button
            size="small"
            danger
            onClick={() => {
              void removeAntigravityAccount(row.id)
                .then(async () => {
                  message.success(t("antigravity.removed"));
                  await refresh();
                })
                .catch((error: unknown) => message.error(errMsg(error)));
            }}
          >
            {t("common.delete")}
          </Button>
        </Space>
      ),
    },
  ];

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      <div>
        <Title level={4} style={{ margin: 0 }}>
          {t("antigravity.title")}
        </Title>
        <Paragraph type="secondary" style={{ marginBottom: 0 }}>
          {t("antigravity.subtitle")}
        </Paragraph>
      </div>

      <Alert type="info" showIcon message={t("antigravity.personalUseNotice")} />

      <Card title={t("antigravity.gateway")} size="small">
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
                loading={outboundMutation.isPending}
                onClick={() => outboundMutation.mutate()}
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
          <Space wrap>
            <Button
              type="primary"
              icon={<PlayCircleOutlined />}
              loading={startMutation.isPending}
              onClick={() => startMutation.mutate()}
            >
              {t("antigravity.start")}
            </Button>
            <Button
              icon={<StopOutlined />}
              loading={stopMutation.isPending}
              onClick={() => stopMutation.mutate()}
            >
              {t("antigravity.stop")}
            </Button>
            <Button icon={<ReloadOutlined />} onClick={() => void refresh()}>
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
          <Paragraph>
            <pre style={{ margin: 0, whiteSpace: "pre-wrap" }}>{curlSnippet}</pre>
          </Paragraph>
        </Space>
      </Card>

      <Card title={t("antigravity.models")} size="small">
        <Space direction="vertical" style={{ width: "100%" }} size={8}>
          <Text type="secondary">{t("antigravity.modelsHint")}</Text>
          <Space wrap>
            <Text>{t("antigravity.reasoningLevel")}</Text>
            <Select
              style={{ minWidth: 180 }}
              loading={defaultsQuery.isLoading || levelMutation.isPending}
              value={reasoningLevel ?? ""}
              onChange={(value) =>
                levelMutation.mutate(value === "" ? null : (value as "low" | "medium" | "high"))
              }
              options={[
                { value: "", label: t("antigravity.reasoningLevelAuto") },
                { value: "low", label: t("antigravity.reasoningLevelLow") },
                { value: "medium", label: t("antigravity.reasoningLevelMedium") },
                { value: "high", label: t("antigravity.reasoningLevelHigh") },
              ]}
            />
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("antigravity.reasoningLevelHint")}
            </Text>
          </Space>
          {(modelsQuery.data?.length ?? 0) === 0 ? (
            <Text type="secondary">{t("antigravity.modelsEmpty")}</Text>
          ) : (
            <Space wrap size={[4, 4]}>
              {(modelsQuery.data ?? []).map((model) => (
                <Tag key={model.id} color={model.id.startsWith("gemini") ? "blue" : "purple"}>
                  {model.displayName?.trim() || model.id}
                </Tag>
              ))}
            </Space>
          )}
        </Space>
      </Card>

      <Card
        title={t("antigravity.accounts")}
        size="small"
        extra={
          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={quotaMutation.isPending}
            disabled={(accountsQuery.data?.length ?? 0) === 0}
            onClick={() => quotaMutation.mutate()}
          >
            {t("antigravity.refreshQuota")}
          </Button>
        }
      >
        <Space direction="vertical" style={{ width: "100%" }} size={12}>
          <Alert
            type="warning"
            showIcon
            message={t("antigravity.howToAddTitle")}
            description={
              <div>
                <p style={{ marginBottom: 8 }}>{t("antigravity.howToAddOauth")}</p>
                <p style={{ marginBottom: 8 }}>{t("antigravity.howToAddNotIde")}</p>
                <p style={{ marginBottom: 0 }}>{t("antigravity.howToAddJson")}</p>
              </div>
            }
          />
          <Button
            type="primary"
            icon={<LoginOutlined />}
            loading={oauthMutation.isPending}
            onClick={() => oauthMutation.mutate()}
          >
            {oauthMutation.isPending
              ? t("antigravity.oauthWaiting")
              : t("antigravity.oauthLogin")}
          </Button>
          {(accountsQuery.data?.length ?? 0) === 0 && (
            <Text type="secondary">{t("antigravity.emptyAccounts")}</Text>
          )}
          <Table
            size="small"
            rowKey="id"
            loading={accountsQuery.isLoading || quotaMutation.isPending}
            dataSource={accountsQuery.data ?? []}
            columns={columns}
            pagination={false}
            locale={{ emptyText: t("antigravity.emptyAccounts") }}
          />
          <Button
            loading={ensureMutation.isPending}
            disabled={(accountsQuery.data?.length ?? 0) === 0}
            onClick={() => ensureMutation.mutate()}
          >
            {t("antigravity.bindCurrentApp")}
          </Button>
          {(accountsQuery.data?.length ?? 0) === 0 && (
            <Text type="danger">{t("antigravity.bindNeedsAccount")}</Text>
          )}
          <Paragraph type="secondary" style={{ marginBottom: 0 }}>
            {t("antigravity.importOptional")}
          </Paragraph>
          <TextArea
            rows={5}
            value={importJson}
            onChange={(event) => setImportJson(event.target.value)}
            placeholder={t("antigravity.importPlaceholder")}
          />
          <Button
            loading={importMutation.isPending}
            disabled={!importJson.trim()}
            onClick={() => importMutation.mutate()}
          >
            {t("antigravity.import")}
          </Button>
        </Space>
      </Card>
    </div>
  );
}
