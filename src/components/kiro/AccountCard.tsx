import { useState } from "react";
import { Alert, Button, Card, Input, Modal, Popconfirm, Progress, Select, Space, Tag, Typography, message } from "antd";
import UserOutlined from "@ant-design/icons/es/icons/UserOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import MessageOutlined from "@ant-design/icons/es/icons/MessageOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { StatusBadge } from "@/components/ui";
import { KIRO_CATALOG } from "@/components/kiro/models";
import {
  refreshKiroAccountQuota,
  testKiroAccount,
  type KiroAccountPublic,
  type KiroAccountTestCategory,
  type KiroAccountTestResult,
  type KiroQuotaSnapshot,
} from "@/services/kiro";

const { Text } = Typography;
const DEFAULT_MODEL = "claude-sonnet-4.6";

interface AccountCardProps {
  account: KiroAccountPublic;
  onRemove: (id: string) => void;
  isPending?: boolean;
}

function authLabel(method: string, provider: string): string {
  const value = method.trim().toLowerCase();
  if (value === "idc" || value === "builder-id" || value === "builderid") return "Builder ID";
  if (value === "social") return provider.trim() || "Social";
  return method.trim() || provider.trim() || "Kiro";
}

function formatCredit(value: number): string {
  if (!Number.isFinite(value)) return "0";
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

function quotaPercent(quota: KiroQuotaSnapshot): number {
  if (quota.limit <= 0) return 0;
  return Math.min(100, Math.max(0, (quota.used / quota.limit) * 100));
}

function testChatAlertType(category: KiroAccountTestCategory): "success" | "info" | "warning" | "error" {
  switch (category) {
    case "ok":
      return "success";
    case "rate_limit":
      return "warning";
    case "network":
      return "info";
    case "auth":
    case "quota":
    case "error":
      return "error";
    default: {
      const _exhaustive: never = category;
      return _exhaustive;
    }
  }
}

function isTestCategory(value: string): value is KiroAccountTestCategory {
  switch (value) {
    case "ok":
    case "rate_limit":
    case "network":
    case "auth":
    case "quota":
    case "error":
      return true;
    default:
      return false;
  }
}

export function AccountCard({ account, onRemove, isPending = false }: AccountCardProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const title = account.label.trim() || account.id;
  const [testOpen, setTestOpen] = useState(false);
  const [testModel, setTestModel] = useState(DEFAULT_MODEL);
  const [testPrompt, setTestPrompt] = useState("hello");
  const [testResult, setTestResult] = useState<KiroAccountTestResult | null>(null);

  const quotaMutation = useMutation({
    mutationFn: () => refreshKiroAccountQuota(account.id),
    onSuccess: async () => {
      message.success(t("kiro.quotaRefreshed"));
      await queryClient.invalidateQueries({ queryKey: ["kiro-accounts"] });
    },
    onError: (error: unknown) => message.error(error instanceof Error ? error.message : String(error)),
  });

  const testMutation = useMutation({
    mutationFn: () => testKiroAccount(account.id, testModel, testPrompt),
    onSuccess: (result) => {
      setTestResult({
        ...result,
        category: isTestCategory(result.category) ? result.category : "error",
      });
    },
    onError: (error: unknown) => {
      setTestResult({
        ok: false,
        category: "error",
        status: null,
        model: testModel,
        latencyMs: 0,
        reply: null,
        error: error instanceof Error ? error.message : String(error),
      });
    },
  });

  const quota = account.quota;

  return (
    <Card
      size="small"
      style={{
        background: account.disabled
          ? "var(--ant-color-bg-container-disabled, #fafafa)"
          : undefined,
        transition: "border-color 0.15s ease, background 0.15s ease",
        height: "100%",
      }}
      styles={{ body: { height: "100%" } }}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 12, height: "100%" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 8 }}>
          <Space align="center" wrap style={{ minWidth: 0 }}>
            <UserOutlined style={{ fontSize: 16, color: "var(--ant-color-text-secondary)" }} />
            <Text strong style={{ fontSize: 14 }} ellipsis>
              {title}
            </Text>
            <Tag style={{ margin: 0 }}>{authLabel(account.authMethod, account.provider)}</Tag>
            {account.region ? <Tag style={{ margin: 0 }}>{account.region}</Tag> : null}
            {quota?.plan ? <Tag style={{ margin: 0 }}>{quota.plan}</Tag> : null}
          </Space>
          {account.disabled ? <StatusBadge status="error" label={t("kiro.disabled")} /> : null}
        </div>

        {quota && quota.limit > 0 ? (
          <div>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("kiro.quotaUsed", { used: formatCredit(quota.used), limit: formatCredit(quota.limit) })}
            </Text>
            <Progress
              percent={Math.round(quotaPercent(quota))}
              size="small"
              showInfo={false}
              status={quotaPercent(quota) >= 100 ? "exception" : "normal"}
            />
            <Space size={8} wrap>
              {quota.resetAt ? (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {t("kiro.quotaReset", { time: quota.resetAt })}
                </Text>
              ) : null}
              {quota.overage ? (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {t("kiro.quotaOverage", { status: quota.overage })}
                </Text>
              ) : null}
              {quota.trialLimit != null ? (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {t("kiro.quotaTrial", {
                    used: formatCredit(quota.trialUsed ?? 0),
                    limit: formatCredit(quota.trialLimit),
                  })}
                </Text>
              ) : null}
            </Space>
          </div>
        ) : (
          <Text type="secondary" style={{ fontSize: 12 }}>
            {quota?.error ? quota.error : t("kiro.quotaEmpty")}
          </Text>
        )}

        {quota?.error && quota.limit > 0 ? (
          <Text type="danger" style={{ fontSize: 12 }}>
            {quota.error}
          </Text>
        ) : null}

        {account.disableReason ? (
          <Text type="danger" style={{ fontSize: 12 }}>
            {account.disableReason}
          </Text>
        ) : (
          <Text type="secondary" style={{ fontSize: 12 }}>
            {account.hasRefreshToken ? t("kiro.hasRefresh") : t("kiro.missingRefresh")}
          </Text>
        )}

        <div style={{ marginTop: "auto", display: "flex", justifyContent: "flex-end", gap: 8, flexWrap: "wrap" }}>
          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={quotaMutation.isPending}
            onClick={() => quotaMutation.mutate()}
          >
            {t("kiro.refreshQuota")}
          </Button>
          <Button size="small" icon={<MessageOutlined />} onClick={() => setTestOpen(true)}>
            {t("kiro.testChat")}
          </Button>
          <Popconfirm
            title={t("kiro.confirmDelete")}
            okText={t("common.delete")}
            cancelText={t("common.cancel")}
            okButtonProps={{ danger: true }}
            onConfirm={() => onRemove(account.id)}
          >
            <Button size="small" danger icon={<DeleteOutlined />} loading={isPending}>
              {t("common.delete")}
            </Button>
          </Popconfirm>
        </div>
      </div>

      <Modal
        title={t("kiro.testChatTitle", { label: title })}
        open={testOpen}
        onCancel={() => setTestOpen(false)}
        footer={null}
        destroyOnHidden
      >
        <Space direction="vertical" size={12} style={{ width: "100%" }}>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("kiro.testChatHint")}
          </Text>
          <div>
            <Text style={{ fontSize: 12 }}>{t("kiro.testChatModel")}</Text>
            <Select
              showSearch
              value={testModel}
              onChange={(value) => setTestModel(String(value))}
              style={{ width: "100%", marginTop: 4 }}
              options={KIRO_CATALOG.map((model) => ({ value: model.id, label: model.id }))}
            />
          </div>
          <div>
            <Text style={{ fontSize: 12 }}>{t("kiro.testChatPrompt")}</Text>
            <Input.TextArea
              value={testPrompt}
              onChange={(event) => setTestPrompt(event.target.value)}
              autoSize={{ minRows: 2, maxRows: 4 }}
              style={{ marginTop: 4 }}
            />
          </div>
          <Button type="primary" loading={testMutation.isPending} onClick={() => testMutation.mutate()}>
            {t("kiro.testChatSend")}
          </Button>
          {testResult ? (
            <Alert
              type={testChatAlertType(testResult.category)}
              showIcon
              message={t(`kiro.testChatResult.${testResult.category}`)}
              description={
                <div>
                  {testResult.reply ? <div>{testResult.reply}</div> : null}
                  {testResult.error ? <div>{testResult.error}</div> : null}
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {testResult.model}
                    {testResult.latencyMs ? ` · ${testResult.latencyMs} ms` : ""}
                  </Text>
                </div>
              }
            />
          ) : null}
        </Space>
      </Modal>
    </Card>
  );
}
