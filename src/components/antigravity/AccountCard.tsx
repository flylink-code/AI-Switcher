import { useState } from "react";
import { Alert, Button, Card, Input, Modal, Popconfirm, Select, Space, Tag, Typography } from "antd";
import UserOutlined from "@ant-design/icons/es/icons/UserOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import CheckOutlined from "@ant-design/icons/es/icons/CheckOutlined";
import MessageOutlined from "@ant-design/icons/es/icons/MessageOutlined";
import { useTranslation } from "react-i18next";
import type {
  AntigravityAccountPublic,
  AntigravityAccountTestCategory,
  AntigravityAccountTestResult,
  AntigravityCatalogModel,
} from "@/types/backend";
import { testAntigravityAccount } from "@/services/api";
import {
  QuotaMiniBar,
  accountQuotaSummary,
  formatQuotaUpdatedAt,
  formatTierLabel,
  tierTagColor,
} from "@/components/AntigravityQuotaBars";
import { StatusBadge } from "@/components/ui";

const { Text, Paragraph } = Typography;
const DEFAULT_PROMPT = "hello";

interface AccountCardProps {
  account: AntigravityAccountPublic;
  onSetActive: (id: string) => void;
  onRemove: (id: string) => void;
  isPending?: boolean;
  quotaViewMode?: "all" | "5h" | "7d";
  models?: AntigravityCatalogModel[];
}

function defaultProbeModel(models: AntigravityCatalogModel[]): string {
  const ids = models.map((model) => model.id);
  return (
    ids.find((id) => id === "gemini-3.8-flash-high")
    ?? ids.find((id) => id.startsWith("gemini-3.8-flash"))
    ?? ids.find((id) => id.includes("flash") && id.startsWith("gemini-"))
    ?? ids[0]
    ?? "gemini-3.8-flash-high"
  );
}

function testChatAlertType(
  category: AntigravityAccountTestCategory,
): "success" | "info" | "warning" | "error" {
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

function errMsg(error: unknown): string {
  if (typeof error === "string" && error.trim()) return error;
  if (error instanceof Error && error.message.trim()) return error.message;
  return String(error ?? "");
}

export function AccountCard({
  account,
  onSetActive,
  onRemove,
  isPending = false,
  quotaViewMode = "all",
  models = [],
}: AccountCardProps) {
  const { t } = useTranslation();
  const [testOpen, setTestOpen] = useState(false);
  const [testModel, setTestModel] = useState(() => defaultProbeModel(models));
  const [testPrompt, setTestPrompt] = useState(DEFAULT_PROMPT);
  const [testBusy, setTestBusy] = useState(false);
  const [testResult, setTestResult] = useState<AntigravityAccountTestResult | null>(null);

  const tier = formatTierLabel(account.subscriptionTier);
  const cooling =
    account.cooldownUntil != null && account.cooldownUntil * 1000 > Date.now();
  const {
    geminiFiveHour,
    geminiWeekly,
    claudeFiveHour,
    claudeWeekly,
    geminiFiveHourReset,
    geminiWeeklyReset,
    claudeFiveHourReset,
    claudeWeeklyReset,
    quotaUpdatedAt,
  } = accountQuotaSummary(account);
  const quotaUpdated = formatQuotaUpdatedAt(quotaUpdatedAt);

  const openTest = () => {
    setTestModel(defaultProbeModel(models));
    setTestPrompt(DEFAULT_PROMPT);
    setTestResult(null);
    setTestOpen(true);
  };

  const runTest = async () => {
    setTestBusy(true);
    try {
      const result = await testAntigravityAccount(
        account.id,
        testModel.trim() || undefined,
        testPrompt.trim() || DEFAULT_PROMPT,
      );
      setTestResult(result);
    } catch (error) {
      setTestResult({
        ok: false,
        category: "error",
        model: testModel,
        latencyMs: 0,
        error: errMsg(error),
      });
    } finally {
      setTestBusy(false);
    }
  };

  return (
    <Card
      size="small"
      style={{
        borderColor: account.isActive
          ? "var(--ant-color-primary, #1677ff)"
          : undefined,
        background: account.disabled
          ? "var(--ant-color-bg-container-disabled, #fafafa)"
          : account.isActive
            ? "color-mix(in srgb, var(--ant-color-primary, #1677ff) 4%, var(--ant-color-bg-container, #ffffff))"
            : undefined,
        transition: "border-color 0.15s ease, background 0.15s ease",
        height: "100%",
      }}
      styles={{ body: { height: "100%" } }}
    >
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: 12,
          height: "100%",
          minHeight: 0,
        }}
      >
        {/* Identity & Status */}
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 8 }}>
          <Space align="center" wrap style={{ minWidth: 0 }}>
            <UserOutlined style={{ fontSize: 16, color: "var(--ant-color-text-secondary)" }} />
            <Text strong style={{ fontSize: 14 }} ellipsis>
              {account.email}
            </Text>
            {tier && <Tag color={tierTagColor(account.subscriptionTier)}>{tier}</Tag>}
          </Space>

          <Space size={4} wrap style={{ flexShrink: 0 }}>
            {account.isActive && (
              <StatusBadge status="running" label={t("antigravity.active")} />
            )}
            {account.disabled && (
              <StatusBadge status="error" label={t("antigravity.disabled")} />
            )}
            {cooling && (
              <StatusBadge status="warning" label={t("antigravity.cooling")} />
            )}
            {account.quotaForbidden && (
              <StatusBadge status="error" label={t("antigravity.forbidden")} />
            )}
          </Space>
        </div>

        {/* Quota Progress */}
        {account.disabledReason && (
          <Text type="danger" style={{ fontSize: 12 }}>
            {account.disabledReason}。{t("antigravity.reauthorizeHint")}
          </Text>
        )}
        {!account.hasProjectId && !account.disabled && (
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("antigravity.quotaNeedsProject")}
          </Text>
        )}
        <div
          style={{
            display: "grid",
            gridTemplateColumns: quotaViewMode === "all" ? "1fr 1fr" : "1fr 1fr",
            gap: 10,
          }}
        >
          {(quotaViewMode === "all" || quotaViewMode === "5h") && (
            <QuotaMiniBar
              label={t("antigravity.quotaGemini5h")}
              percent={geminiFiveHour}
              resetTime={geminiFiveHourReset}
            />
          )}
          {(quotaViewMode === "all" || quotaViewMode === "7d") && (
            <QuotaMiniBar
              label={t("antigravity.quotaGemini7d")}
              percent={geminiWeekly}
              resetTime={geminiWeeklyReset}
            />
          )}
          {(quotaViewMode === "all" || quotaViewMode === "5h") && (
            <QuotaMiniBar
              label={t("antigravity.quotaClaude5h")}
              percent={claudeFiveHour}
              resetTime={claudeFiveHourReset}
            />
          )}
          {(quotaViewMode === "all" || quotaViewMode === "7d") && (
            <QuotaMiniBar
              label={t("antigravity.quotaClaude7d")}
              percent={claudeWeekly}
              resetTime={claudeWeeklyReset}
            />
          )}
        </div>

        <div style={{ marginTop: "auto", display: "flex", flexDirection: "column", gap: 8 }}>
          <div
            style={{
              display: "flex",
              flexWrap: "wrap",
              alignItems: "center",
              gap: "4px 12px",
              minHeight: 22,
            }}
          >
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("antigravity.health")}: {Math.round(account.healthScore * 100)}%
            </Text>
            <Text type="secondary" style={{ fontSize: 12, display: "inline-flex", alignItems: "center", gap: 4 }}>
              {t("antigravity.project")}:{" "}
              {account.hasProjectId ? (
                <Tag color="blue" style={{ margin: 0, fontSize: 10 }}>
                  OK
                </Tag>
              ) : (
                <Tag style={{ margin: 0, fontSize: 10 }}>{t("antigravity.pending")}</Tag>
              )}
            </Text>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("antigravity.quotaUpdated")}: {quotaUpdated ?? "—"}
            </Text>
          </div>

          <div
            style={{
              display: "flex",
              justifyContent: "flex-end",
              alignItems: "center",
              gap: 8,
              minHeight: 24,
            }}
          >
            <Button size="small" icon={<MessageOutlined />} onClick={openTest}>
              {t("antigravity.testChat")}
            </Button>
            {!account.isActive && !account.disabled ? (
              <Button
                size="small"
                icon={<CheckOutlined />}
                loading={isPending}
                onClick={() => onSetActive(account.id)}
              >
                {t("antigravity.setActive")}
              </Button>
            ) : null}
            <Popconfirm
              title={t("antigravity.confirmDeleteTitle")}
              description={t("antigravity.confirmDeleteDesc", { email: account.email })}
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
      </div>

      <Modal
        title={t("antigravity.testChatTitle", { email: account.email })}
        open={testOpen}
        onCancel={() => setTestOpen(false)}
        footer={null}
        destroyOnHidden
      >
        <Space direction="vertical" size={12} style={{ width: "100%" }}>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("antigravity.testChatHint")}
          </Text>
          <div>
            <Text style={{ fontSize: 12 }}>{t("antigravity.testChatModel")}</Text>
            <Select
              showSearch
              value={testModel}
              onChange={(value) => setTestModel(String(value))}
              style={{ width: "100%", marginTop: 4 }}
              optionFilterProp="label"
              options={(models.length > 0 ? models : [{ id: testModel, displayName: testModel }]).map(
                (model) => ({
                  value: model.id,
                  label: model.displayName?.trim() ? `${model.displayName} (${model.id})` : model.id,
                }),
              )}
            />
          </div>
          <div>
            <Text style={{ fontSize: 12 }}>{t("antigravity.testChatPrompt")}</Text>
            <Input
              value={testPrompt}
              onChange={(event) => setTestPrompt(event.target.value)}
              onPressEnter={() => void runTest()}
              style={{ marginTop: 4 }}
            />
          </div>
          <Button type="primary" loading={testBusy} onClick={() => void runTest()} block>
            {t("antigravity.testChatSend")}
          </Button>
          {testResult ? (
            <Alert
              type={testChatAlertType(testResult.category)}
              showIcon
              message={t(`antigravity.testChatResult.${testResult.category}`, {
                ms: testResult.latencyMs,
                model: testResult.model,
              })}
              description={
                <Space direction="vertical" size={4} style={{ width: "100%" }}>
                  {testResult.reply ? (
                    <Paragraph style={{ margin: 0, whiteSpace: "pre-wrap" }}>
                      {testResult.reply}
                    </Paragraph>
                  ) : null}
                  {testResult.error ? (
                    <Text type="secondary" style={{ fontSize: 12, whiteSpace: "pre-wrap" }}>
                      {testResult.error}
                    </Text>
                  ) : null}
                </Space>
              }
            />
          ) : null}
        </Space>
      </Modal>
    </Card>
  );
}
