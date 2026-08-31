import { useState } from "react";
import { ReloadOutlined } from "@ant-design/icons";
import { Space, Spin, Tag, Tooltip, Typography } from "antd";
import { useTranslation } from "react-i18next";
import { useInvalidateQuota, useOfficialQuota, useProviderQuota } from "@/lib/quotaQueries";
import type { ProviderQuotaResult, ProviderTarget, QuotaTier } from "@/types/backend";

const { Text } = Typography;

function formatResetCountdown(resetsAt?: string | null): string | null {
  if (!resetsAt) return null;
  const target = new Date(resetsAt).getTime();
  if (Number.isNaN(target)) return null;
  const now = Date.now();
  const diffMs = target - now;
  if (diffMs <= 0) return "已就绪";

  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 60) {
    return `${diffMin}m`;
  }
  const hours = Math.floor(diffMin / 60);
  const mins = diffMin % 60;
  if (hours < 24) {
    return mins > 0 ? `${hours}h ${mins}m` : `${hours}h`;
  }
  const days = Math.floor(hours / 24);
  const remHours = hours % 24;
  return remHours > 0 ? `${days}d ${remHours}h` : `${days}d`;
}

function tierDisplayName(
  name: string,
  t: (key: string, options?: { defaultValue: string }) => string,
): string {
  switch (name.toLowerCase()) {
    case "five_hour":
      return "5h";
    case "seven_day":
      return "7d";
    case "seven_day_opus":
      return "7d Opus";
    case "seven_day_sonnet":
      return "7d Sonnet";
    case "30_day":
      return "30d";
    case "1d":
    case "daily":
      return t("quota.dailyWindow", { defaultValue: "日额度" });
    case "weekly_limit":
    case "weekly":
      return t("quota.weeklyWindow", { defaultValue: "周额度" });
    case "monthly":
      return t("quota.monthlyWindow", { defaultValue: "月额度" });
    case "credits":
      return t("quota.creditsWindow", { defaultValue: "额度" });
    default:
      return name;
  }
}

function getTierTagColor(utilization: number): string {
  if (utilization >= 90) return "error";
  if (utilization >= 70) return "warning";
  return "success";
}

function formatCurrency(amount: number, currency: string): string {
  const cur = currency.toUpperCase();
  if (cur === "CNY" || cur === "RMB") {
    return `¥${amount.toFixed(2)}`;
  }
  if (cur === "USD") {
    return `$${amount.toFixed(2)}`;
  }
  if (cur === "CREDIT" || cur === "CREDITS") {
    return `${amount.toFixed(2)} Credits`;
  }
  return `${amount.toFixed(2)} ${currency}`;
}

function formatQueryTime(timestamp: number): string {
  const d = new Date(timestamp);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export function ProviderQuotaView({
  providerId,
  target,
}: {
  providerId?: string;
  target?: ProviderTarget;
}) {
  const { t } = useTranslation();
  const [isRefreshing, setIsRefreshing] = useState(false);
  const { invalidateProviderQuota, invalidateOfficialQuota } = useInvalidateQuota();

  const providerQuery = useProviderQuota(providerId ?? "", Boolean(providerId));
  const officialQuery = useOfficialQuota(target ?? "claude_code", Boolean(target && !providerId));

  const query = providerId ? providerQuery : officialQuery;
  const { data, isLoading, isFetching } = query;

  const handleRefresh = async (e: React.MouseEvent) => {
    e.stopPropagation();
    setIsRefreshing(true);
    try {
      if (providerId) {
        await invalidateProviderQuota(providerId);
      } else if (target) {
        await invalidateOfficialQuota(target);
      }
    } finally {
      setIsRefreshing(false);
    }
  };

  if (isLoading) {
    return (
      <Tag style={{ margin: 0, borderRadius: 4, fontSize: 11, background: "var(--color-bg-subtle, rgba(0,0,0,0.04))" }}>
        <Space orientation="horizontal" size={4}>
          <Spin size="small" />
          <span style={{ color: "var(--ant-color-text-tertiary)" }}>{t("quota.querying", { defaultValue: "查询额度..." })}</span>
        </Space>
      </Tag>
    );
  }

  if (!data) return null;

  if (data.kind === "unsupported") {
    return null;
  }

  if (data.kind === "error") {
    const isAuth = data.code === "AUTH_FAILED";
    return (
      <Tooltip
        title={
          <Space orientation="vertical" size={2}>
            <span>{data.message}</span>
            <span style={{ fontSize: 10, opacity: 0.75 }}>
              {t("quota.queriedAt", { time: formatQueryTime(data.queried_at), defaultValue: `查询时间: ${formatQueryTime(data.queried_at)}` })}
            </span>
          </Space>
        }
      >
        <Tag
          color={isAuth ? "error" : "warning"}
          style={{ margin: 0, borderRadius: 4, fontSize: 11, cursor: "pointer" }}
          onClick={handleRefresh}
        >
          <Space orientation="horizontal" size={4}>
            <span>{isAuth ? t("quota.authFailed", { defaultValue: "Key 鉴权失效" }) : t("quota.queryFailed", { defaultValue: "查询失败" })}</span>
            <ReloadOutlined spin={isRefreshing || isFetching} style={{ fontSize: 10 }} />
          </Space>
        </Tag>
      </Tooltip>
    );
  }

  if (data.kind === "balance") {
    const unlimited = data.total_balance < 0;
    const color = unlimited
      ? "blue"
      : data.is_available && data.total_balance > 0
        ? "blue"
        : data.total_balance <= 0
          ? "error"
          : "default";
    const amountLabel = unlimited
      ? t("quota.unlimited", { defaultValue: "不限" })
      : formatCurrency(data.total_balance, data.currency);
    return (
      <Tooltip
        title={
          <Space orientation="vertical" size={2}>
            <div>
              <strong>{t("quota.balance", { defaultValue: "总余额" })}: </strong>
              {amountLabel}
            </div>
            {!unlimited && data.topped_up_balance != null && (
              <div>
                {t("quota.toppedUpBalance", { defaultValue: "充值金额" })}: {formatCurrency(data.topped_up_balance, data.currency)}
              </div>
            )}
            {!unlimited && data.granted_balance != null && (
              <div>
                {t("quota.grantedBalance", { defaultValue: "赠送金额" })}: {formatCurrency(data.granted_balance, data.currency)}
              </div>
            )}
            <div style={{ fontSize: 10, opacity: 0.75, marginTop: 4 }}>
              {t("quota.queriedAt", { time: formatQueryTime(data.queried_at), defaultValue: `查询时间: ${formatQueryTime(data.queried_at)}` })} · 点击刷新
            </div>
          </Space>
        }
      >
        <Tag
          color={color}
          style={{
            margin: 0,
            borderRadius: 4,
            fontSize: 11,
            fontWeight: 500,
            cursor: "pointer",
            display: "inline-flex",
            alignItems: "center",
            gap: 4,
          }}
          onClick={handleRefresh}
        >
          <span>💰 {amountLabel}</span>
          <ReloadOutlined spin={isRefreshing || isFetching} style={{ fontSize: 10, opacity: 0.8 }} />
        </Tag>
      </Tooltip>
    );
  }

  if (data.kind === "subscription") {
    if (!data.tiers || data.tiers.length === 0) {
      return null;
    }

    return (
      <Space orientation="horizontal" size={4} wrap style={{ minWidth: 0 }}>
        {data.tiers.map((tier: QuotaTier) => {
          const util = Math.round(tier.utilization);
          const tagColor = getTierTagColor(util);
          const cd = formatResetCountdown(tier.resets_at);
          const label = tierDisplayName(tier.name, t);

          return (
            <Tooltip
              key={tier.name}
              title={
                <Space orientation="vertical" size={2}>
                  <div>
                    <strong>{label}</strong>: 已用 {util}%
                    {tier.used_value != null && tier.max_value != null && (
                      <span> ({tier.used_value} / {tier.max_value})</span>
                    )}
                  </div>
                  {tier.resets_at && (
                    <div>
                      {t("quota.resetsIn", {
                        time: cd ?? tier.resets_at,
                        defaultValue: `${cd ?? tier.resets_at} 后重置`,
                      })}
                    </div>
                  )}
                  {data.plan_name && <div>套餐: {data.plan_name}</div>}
                  {data.extra_usage?.is_enabled && (
                    <div>
                      {t("quota.extraUsage", { defaultValue: "超额用量" })}:{" "}
                      {data.extra_usage.used ?? 0} / {data.extra_usage.monthly_limit ?? "∞"} {data.extra_usage.currency ?? ""}
                    </div>
                  )}
                  <div style={{ fontSize: 10, opacity: 0.75, marginTop: 4 }}>
                    {t("quota.queriedAt", { time: formatQueryTime(data.queried_at), defaultValue: `查询时间: ${formatQueryTime(data.queried_at)}` })} · 点击刷新
                  </div>
                </Space>
              }
            >
              <Tag
                color={tagColor}
                style={{
                  margin: 0,
                  borderRadius: 4,
                  fontSize: 11,
                  cursor: "pointer",
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 3,
                }}
                onClick={handleRefresh}
              >
                <span>{label}: {util}%{cd ? ` (${cd})` : ""}</span>
              </Tag>
            </Tooltip>
          );
        })}
        <Tooltip title={t("quota.refreshQuota", { defaultValue: "刷新额度" })}>
          <ReloadOutlined
            spin={isRefreshing || isFetching}
            style={{ fontSize: 11, color: "var(--ant-color-text-tertiary)", cursor: "pointer", marginLeft: 2 }}
            onClick={handleRefresh}
          />
        </Tooltip>
      </Space>
    );
  }

  return null;
}
