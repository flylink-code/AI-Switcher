import { Card, Table } from "antd";
import { useTranslation } from "react-i18next";
import type { UsageBreakdown } from "@/types/backend";
import { formatCompactNumber } from "@/utils/formatCompact";

function currencyPrefix(currency?: string | null) {
  const normalized = (currency ?? "USD").trim().toUpperCase();
  if (normalized === "CNY" || normalized === "RMB") return "¥";
  if (normalized === "EUR") return "€";
  if (normalized === "GBP") return "£";
  if (normalized === "USD" || normalized === "") return "$";
  return `${normalized} `;
}

function formatCost(value: number, currency?: string | null) {
  return `${currencyPrefix(currency)}${value.toFixed(4)}`;
}

interface UsageBreakdownCardProps {
  title: string;
  data: UsageBreakdown[];
  /** Cap rows on dense surfaces like Overview; omit for full tables. */
  maxRows?: number;
  loading?: boolean;
}

export function UsageBreakdownCard({
  title,
  data,
  maxRows,
  loading = false,
}: UsageBreakdownCardProps) {
  const { t } = useTranslation();
  const rows = maxRows && data.length > maxRows ? data.slice(0, maxRows) : data;

  return (
    <Card size="small" className="page-surface" title={title} loading={loading}>
      <Table
        size="small"
        pagination={false}
        rowKey="key"
        locale={{ emptyText: t("usage.noData") }}
        dataSource={rows}
        scroll={rows.length > 6 ? { y: 240 } : undefined}
        columns={[
          { title: t("usage.name"), dataIndex: "key", ellipsis: true },
          { title: t("usage.requests"), dataIndex: "requestCount", width: 88 },
          {
            title: t("usage.totalTokens"),
            width: 100,
            render: (_: unknown, row: UsageBreakdown) =>
              formatCompactNumber(
                row.inputTokens +
                  row.cacheReadInputTokens +
                  row.cacheCreationInputTokens +
                  row.outputTokens,
              ),
          },
          {
            title: t("usage.estimatedCost"),
            width: 120,
            dataIndex: "estimatedCost",
            render: (value: number, row: UsageBreakdown) =>
              row.currency === "MIXED"
                ? t("usage.mixedCurrency")
                : formatCost(value, row.currency),
          },
        ]}
      />
    </Card>
  );
}
