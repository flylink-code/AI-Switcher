import React from "react";
import { Tooltip } from "antd";
import QuestionCircleOutlined from "@ant-design/icons/es/icons/QuestionCircleOutlined";
import { useTranslation } from "react-i18next";
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

function Tile({
  label,
  value,
  help,
}: {
  label: React.ReactNode;
  value: React.ReactNode;
  help?: string;
}) {
  return (
    <div className="usage-metric-tile">
      <div className="usage-metric-tile-label">
        {label}
        {help ? (
          <Tooltip title={help}>
            <QuestionCircleOutlined style={{ fontSize: 12, color: "var(--color-text-tertiary)" }} />
          </Tooltip>
        ) : null}
      </div>
      <div className="usage-metric-tile-value">{value}</div>
    </div>
  );
}

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
  const costText = `${costCurrencyPrefix ?? ""}${estimatedCost.toFixed(2)}`;

  return (
    <div className={`usage-kpi-grid ${className}`.trim()} style={style}>
      <Tile label={t("usage.requests")} value={formatCompactNumber(requests)} />
      <Tile label={t("usage.successRate")} value={`${successRate.toFixed(1)} %`} />
      <Tile label={t("usage.totalTokens")} value={formatCompactNumber(totalTokens)} />
      <Tile
        label={t("usage.estimatedCost")}
        help={t("usage.currencyLimit")}
        value={costText}
      />
    </div>
  );
};
