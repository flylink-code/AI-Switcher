import React from "react";
import { Statistic, Tooltip } from "antd";
import QuestionCircleOutlined from "@ant-design/icons/es/icons/QuestionCircleOutlined";
import { useTranslation } from "react-i18next";
import { Surface } from "@/components/ui";
import { formatCompactNumber } from "@/utils/formatCompact";

export interface UsageMetricStripProps {
  requests: number;
  successRate: number;
  totalTokens: number;
  estimatedCost: number;
  costCurrencyPrefix?: string;
  className?: string;
  style?: React.CSSProperties;
}

function StripCell({
  label,
  children,
  help,
}: {
  label: React.ReactNode;
  children: React.ReactNode;
  help?: string;
}) {
  return (
    <div style={{ flex: "1 1 0", minWidth: 120, padding: "2px 16px" }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 4,
          fontSize: "var(--font-size-xs)",
          color: "var(--color-text-secondary)",
          marginBottom: 2,
        }}
      >
        {label}
        {help && (
          <Tooltip title={help}>
            <QuestionCircleOutlined style={{ fontSize: 12, color: "var(--color-text-tertiary)" }} />
          </Tooltip>
        )}
      </div>
      {children}
    </div>
  );
}

/** Single-surface KPI strip replacing the four independent metric cards. */
export const UsageMetricStrip: React.FC<UsageMetricStripProps> = ({
  requests,
  successRate,
  totalTokens,
  estimatedCost,
  costCurrencyPrefix,
  className = "",
  style,
}) => {
  const { t } = useTranslation();

  return (
    <Surface padding="sm" className={className} style={style}>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          rowGap: "var(--space-2)",
        }}
      >
        <StripCell label={t("usage.requests")}>
          <Statistic value={requests} valueStyle={{ fontSize: "var(--font-size-xl)" }} />
        </StripCell>
        <StripCell label={t("usage.successRate")}>
          <Statistic value={successRate} suffix="%" valueStyle={{ fontSize: "var(--font-size-xl)" }} />
        </StripCell>
        <StripCell label={t("usage.totalTokens")}>
          <Statistic
            value={totalTokens}
            formatter={(v) => formatCompactNumber(Number(v)) as unknown as string}
            valueStyle={{ fontSize: "var(--font-size-xl)" }}
          />
        </StripCell>
        <StripCell
          label={t("usage.estimatedCost")}
          help={t("usage.currencyLimit")}
        >
          <Statistic
            value={estimatedCost}
            precision={4}
            prefix={costCurrencyPrefix}
            valueStyle={{ fontSize: "var(--font-size-xl)" }}
          />
        </StripCell>
      </div>
    </Surface>
  );
};
