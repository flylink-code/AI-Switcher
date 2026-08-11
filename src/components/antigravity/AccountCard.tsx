import { Button, Card, Popconfirm, Space, Tag, Typography } from "antd";
import UserOutlined from "@ant-design/icons/es/icons/UserOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import CheckOutlined from "@ant-design/icons/es/icons/CheckOutlined";
import { useTranslation } from "react-i18next";
import type { AntigravityAccountPublic } from "@/services/api";
import {
  QuotaMiniBar,
  accountQuotaSummary,
  formatQuotaUpdatedAt,
  formatTierLabel,
  tierTagColor,
} from "@/components/AntigravityQuotaBars";
import { StatusBadge } from "@/components/ui";

const { Text } = Typography;

interface AccountCardProps {
  account: AntigravityAccountPublic;
  onSetActive: (id: string) => void;
  onRemove: (id: string) => void;
  isPending?: boolean;
}

export function AccountCard({
  account,
  onSetActive,
  onRemove,
  isPending = false,
}: AccountCardProps) {
  const { t } = useTranslation();

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
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
          <QuotaMiniBar
            label={t("antigravity.quotaGemini5h")}
            percent={geminiFiveHour}
            resetTime={geminiFiveHourReset}
          />
          <QuotaMiniBar
            label={t("antigravity.quotaGemini7d")}
            percent={geminiWeekly}
            resetTime={geminiWeeklyReset}
          />
          <QuotaMiniBar
            label={t("antigravity.quotaClaude5h")}
            percent={claudeFiveHour}
            resetTime={claudeFiveHourReset}
          />
          <QuotaMiniBar
            label={t("antigravity.quotaClaude7d")}
            percent={claudeWeekly}
            resetTime={claudeWeeklyReset}
          />
        </div>

        {/* Footer: meta row + actions row — same structure whether active or not */}
        <div
          style={{
            marginTop: "auto",
            display: "flex",
            flexDirection: "column",
            gap: 8,
          }}
        >
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
    </Card>
  );
}
