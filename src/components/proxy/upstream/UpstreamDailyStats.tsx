import { Tag, Typography } from "antd";
import { useTranslation } from "react-i18next";
import type { UpstreamDailyUsageStat } from "@/types/backend";

export interface UpstreamDailyStatsProps {
  /**
   * 父级查询提升并聚合后的当日用量统计信息（子行仅负责展示）
   */
  stat?: UpstreamDailyUsageStat | null;
  isLoading?: boolean;
  isError?: boolean;
  upstreamId?: string;
}

export function UpstreamDailyStats({ stat, isLoading, isError }: UpstreamDailyStatsProps) {
  const { t } = useTranslation();

  if (isError) {
    return (
      <Typography.Text type="secondary">
        {t("upstreamStats.unknown", { defaultValue: "未知" })}
      </Typography.Text>
    );
  }
  if (isLoading && !stat) {
    return <Typography.Text type="secondary">—</Typography.Text>;
  }
  if (!stat || stat.requestCount === 0) {
    return (
      <Typography.Text type="secondary">
        {t("upstreamStats.empty", { defaultValue: "暂无数据" })}
      </Typography.Text>
    );
  }
  return (
    <div>
      <Tag color={stat.successRate >= 0.95 ? "success" : "warning"}>
        {t("upstreamStats.successRate", {
          rate: (stat.successRate * 100).toFixed(1),
          count: stat.requestCount,
          defaultValue: `成功率 {(stat.successRate * 100).toFixed(1)}% ({stat.requestCount})`,
        })}
      </Tag>
      <Typography.Text type="secondary">
        {stat.estimatedCostCurrency} {stat.estimatedCost.toFixed(4)}
      </Typography.Text>
    </div>
  );
}
