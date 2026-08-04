import { Alert, Card, Col, Row, Select, Space, Typography } from "antd";
import CalendarOutlined from "@ant-design/icons/es/icons/CalendarOutlined";
import DollarOutlined from "@ant-design/icons/es/icons/DollarOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { OnboardingTip } from "@/components/OnboardingTip";
import { UsageBreakdownCard } from "@/components/UsageBreakdownCard";
import { UsageCalendar } from "@/components/UsageCalendar";
import { UsageMetric } from "@/components/UsageMetric";
import { UsageSourceFilterSegmented } from "@/components/UsageSourceFilterSegmented";
import { usageDashboardOptions, usageTrendOptions } from "@/lib/appQueries";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import {
  USAGE_PERIOD_VALUES,
  usagePeriodLabelKey,
} from "@/utils/usagePeriod";

const { Text, Title } = Typography;

export default function OverviewPage() {
  const { t } = useTranslation();
  const heatmapPeriod = usePagePreferencesStore((state) => state.heatmapPeriod);
  const setHeatmapPeriod = usePagePreferencesStore((state) => state.setHeatmapPeriod);
  const heatmapSource = usePagePreferencesStore((state) => state.heatmapSource);
  const setHeatmapSource = usePagePreferencesStore((state) => state.setHeatmapSource);

  const dashboardQuery = useQuery(usageDashboardOptions(heatmapPeriod, heatmapSource));
  const trendQuery = useQuery(usageTrendOptions(heatmapPeriod, heatmapSource));

  const summary = dashboardQuery.data?.summary;
  const byProvider = dashboardQuery.data?.byProvider ?? [];
  const byModel = dashboardQuery.data?.byModel ?? [];
  const totalTokens =
    (summary?.inputTokens ?? 0) +
    (summary?.cacheReadInputTokens ?? 0) +
    (summary?.cacheCreationInputTokens ?? 0) +
    (summary?.outputTokens ?? 0);

  const periodSourceFilters = (
    <Space wrap size={8} align="center">
      <Select
        size="middle"
        value={heatmapPeriod}
        style={{ width: 160 }}
        options={USAGE_PERIOD_VALUES.map((value) => ({
          value,
          label:
            typeof value === "number"
              ? t("usage.lastDays", { days: value })
              : t(usagePeriodLabelKey(value)),
        }))}
        onChange={setHeatmapPeriod}
      />
      <UsageSourceFilterSegmented
        value={heatmapSource}
        onChange={setHeatmapSource}
        t={t}
      />
    </Space>
  );

  return (
    <Space direction="vertical" size={16} style={{ width: "100%" }}>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: 12,
          alignItems: "flex-start",
          justifyContent: "space-between",
        }}
      >
        <div>
          <Title level={4} style={{ margin: 0 }}>
            {t("overview.title")}
          </Title>
          <Text type="secondary">{t("overview.description")}</Text>
        </div>
        {periodSourceFilters}
      </div>

      <OnboardingTip tipKey="overview" message={t("overview.title")} description={t("overview.description")} />

      <Card
        size="small"
        title={t("overview.usageSummary")}
        extra={<Text type="secondary">{t("overview.usageHint")}</Text>}
      >
        {dashboardQuery.error ? (
          <Alert type="error" showIcon message={errMsg(dashboardQuery.error)} />
        ) : (
          <Space direction="vertical" size={12} style={{ width: "100%" }}>
            <Row gutter={[16, 16]}>
              <UsageMetric
                title={t("usage.requests")}
                value={summary?.requestCount ?? 0}
                icon={<ThunderboltOutlined />}
              />
              <UsageMetric
                title={t("usage.successRate")}
                value={successRate(summary?.requestCount, summary?.successfulRequestCount)}
                suffix="%"
              />
              <UsageMetric title={t("usage.totalTokens")} value={totalTokens} compact />
              <UsageMetric
                title={t("usage.estimatedCost")}
                value={summary?.estimatedCost ?? 0}
                precision={4}
                prefix={currencyPrefix(summary?.estimatedCostCurrency)}
                icon={<DollarOutlined />}
              />
            </Row>
            {(summary?.estimatedCostsByCurrency?.length ?? 0) > 1 && (
              <Alert
                type="info"
                showIcon
                message={t("usage.multiCurrencyTitle")}
                description={summary?.estimatedCostsByCurrency
                  .map((entry) => `${currencyPrefix(entry.currency)}${entry.amount.toFixed(4)} (${entry.currency})`)
                  .join(" · ")}
              />
            )}
          </Space>
        )}
      </Card>

      <Row gutter={[16, 16]}>
        <Col xs={24} lg={12}>
          <UsageBreakdownCard
            title={t("usage.byProvider")}
            data={byProvider}
            maxRows={10}
            loading={dashboardQuery.isLoading}
          />
        </Col>
        <Col xs={24} lg={12}>
          <UsageBreakdownCard
            title={t("usage.byModel")}
            data={byModel}
            maxRows={10}
            loading={dashboardQuery.isLoading}
          />
        </Col>
      </Row>

      <Card
        size="small"
        title={
          <Space>
            <CalendarOutlined />
            {t("usage.dailyStatistics")}
          </Space>
        }
      >
        {trendQuery.error ? (
          <Alert type="error" showIcon message={errMsg(trendQuery.error)} />
        ) : (
          <UsageCalendar data={trendQuery.data?.trend ?? []} period={heatmapPeriod} />
        )}
      </Card>
    </Space>
  );
}

function successRate(total?: number, successful?: number) {
  return total ? Number((((successful ?? 0) / total) * 100).toFixed(1)) : 0;
}

function currencyPrefix(currency?: string | null) {
  const normalized = (currency ?? "USD").trim().toUpperCase();
  if (normalized === "CNY" || normalized === "RMB") return "¥";
  if (normalized === "EUR") return "€";
  if (normalized === "GBP") return "£";
  if (normalized === "USD" || normalized === "") return "$";
  return `${normalized} `;
}

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
