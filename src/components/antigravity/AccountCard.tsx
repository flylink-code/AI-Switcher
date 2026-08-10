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
import { StatusBadge, Surface } from "@/components/ui";

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
        background: account.disabled ? "var(--ant-color-bg-container-disabled, #fafafa)" : undefined,
        transition: "all 0.2s ease",
      }}
    >
      <Space direction="vertical" style={{ width: "100%" }} size={12}>
        {/* Identity & Status */}
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
          <Space align="center" wrap>
            <UserOutlined style={{ fontSize: 16, color: "var(--ant-color-text-secondary)" }} />
            <Text strong style={{ fontSize: 14 }}>
              {account.email}
            </Text>
            {tier && <Tag color={tierTagColor(account.subscriptionTier)}>{tier}</Tag>}
          </Space>

          <Space size={4} wrap>
            {account.isActive && (
              <StatusBadge status="running" text={t("antigravity.active")} />
            )}
            {account.disabled && (
              <StatusBadge status="error" text={t("antigravity.disabled")} />
            )}
            {cooling && (
              <StatusBadge status="warning" text={t("antigravity.cooling")} />
            )}
            {account.quotaForbidden && (
              <StatusBadge status="error" text={t("antigravity.forbidden")} />
            )}
          </Space>
        </div>

        {/* Quota Progress */}
        <Surface variant="inset" padding="sm">
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
        </Surface>

        {/* Card Footer Actions */}
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <Space size={12}>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("antigravity.health")}: {Math.round(account.healthScore * 100)}%
            </Text>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("antigravity.project")}:{" "}
              {account.hasProjectId ? (
                <Tag color="blue" style={{ margin: 0, fontSize: 10 }}>
                  OK
                </Tag>
              ) : (
                <Tag style={{ margin: 0, fontSize: 10 }}>{t("antigravity.pending")}</Tag>
              )}
            </Text>
            {quotaUpdated && (
              <Text type="secondary" style={{ fontSize: 12 }}>
                {t("antigravity.quotaUpdated")}: {quotaUpdated}
              </Text>
            )}
          </Space>

          <Space size={8}>
            {!account.isActive && !account.disabled && (
              <Button
                size="small"
                icon={<CheckOutlined />}
                loading={isPending}
                onClick={() => onSetActive(account.id)}
              >
                {t("antigravity.setActive")}
              </Button>
            )}
            <Popconfirm
              title={t("antigravity.confirmDeleteTitle")}
              description={t("antigravity.confirmDeleteDesc", { email: account.email })}
              okText={t("common.delete")}
              cancelText={t("common.cancel")}
              okButtonProps={{ danger: true }}
              onConfirm={() => onRemove(account.id)}
            >
              <Button size="small" danger icon={<DeleteOutlined />}>
                {t("common.delete")}
              </Button>
            </Popconfirm>
          </Space>
        </div>
      </Space>
    </Card>
  );
}
