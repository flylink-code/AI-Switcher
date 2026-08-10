import React from "react";
import { Button, Tooltip, Typography } from "antd";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import ArrowRightOutlined from "@ant-design/icons/es/icons/ArrowRightOutlined";
import InfoCircleOutlined from "@ant-design/icons/es/icons/InfoCircleOutlined";
import { useTranslation } from "react-i18next";
import { Surface, Inline, Stack, Metric } from "@/components/ui";
import { useNavigatePage } from "@/lib/navigation";
import { formatCompactNumber } from "@/utils/formatCompact";

const { Text } = Typography;

export interface UsageSnapshotProps {
  requestCount?: number;
  totalTokens?: number;
  estimatedCost?: number;
  costCurrency?: string | null;
  successfulRequestCount?: number;
  tokensVsYesterday?: number | null;
  className?: string;
  style?: React.CSSProperties;
}

export const UsageSnapshot: React.FC<UsageSnapshotProps> = ({
  requestCount = 0,
  totalTokens = 0,
  estimatedCost = 0,
  costCurrency,
  successfulRequestCount = 0,
  tokensVsYesterday,
  className = "",
  style,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigatePage();

  const successRate = requestCount ? Number((((successfulRequestCount ?? 0) / requestCount) * 100).toFixed(1)) : 0;
  const currencySymbol = currencyPrefix(costCurrency);

  return (
    <Surface padding="md" className={className} style={style}>
      <Stack gap="md">
        {/* Header Title */}
        <Inline justify="space-between" align="center">
          <Inline gap="sm">
            <BarChartOutlined style={{ fontSize: 18, color: "var(--color-brand)" }} />
            <Text strong style={{ fontSize: "var(--font-size-md)" }}>
              {t("dashboard.usageTitle", { defaultValue: "最近 24h 用量 (24h Usage)" })}
            </Text>
          </Inline>

          <Button
            type="link"
            size="small"
            icon={<ArrowRightOutlined />}
            onClick={() => navigate("usage")}
            style={{ fontSize: "var(--font-size-xs)", padding: 0 }}
          >
            {t("dashboard.viewUsage", { defaultValue: "用量详情" })}
          </Button>
        </Inline>

        {/* Metrics Grid */}
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fit, minmax(120px, 1fr))",
            gap: "var(--space-4)",
          }}
        >
          <Metric
            label={t("usage.requests", { defaultValue: "请求数" })}
            value={formatCompactNumber(requestCount)}
          />

          <Metric
            label={t("usage.totalTokens", { defaultValue: "Tokens" })}
            value={formatCompactNumber(totalTokens)}
            supporting={
              tokensVsYesterday != null
                ? t(
                    tokensVsYesterday >= 0
                      ? "usage.totalTokensVsYesterdayUp"
                      : "usage.totalTokensVsYesterdayDown",
                    { pct: Math.abs(tokensVsYesterday) },
                  )
                : undefined
            }
          />

          <Metric
            label={
              <Inline gap="xs" align="center">
                <span>{t("usage.estimatedCost", { defaultValue: "预估成本" })}</span>
                <Tooltip title={t("dashboard.costTooltip", { defaultValue: "根据本地记录的模型 Token 与配置价格估算，不代表供应商最终账单。" })}>
                  <InfoCircleOutlined style={{ fontSize: 10, color: "var(--color-text-tertiary)" }} />
                </Tooltip>
              </Inline>
            }
            value={`${currencySymbol}${estimatedCost.toFixed(3)}`}
          />

          <Metric
            label={t("usage.successRate", { defaultValue: "成功率" })}
            value={`${successRate}%`}
          />
        </div>
      </Stack>
    </Surface>
  );
};

function currencyPrefix(currency?: string | null) {
  const normalized = (currency ?? "USD").trim().toUpperCase();
  if (normalized === "CNY" || normalized === "RMB") return "¥";
  if (normalized === "EUR") return "€";
  if (normalized === "GBP") return "£";
  if (normalized === "USD" || normalized === "") return "$";
  return `${normalized} `;
}
