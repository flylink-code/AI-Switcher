import { Empty, Space, Statistic, Tooltip, Typography, theme } from "antd";
import { useTranslation } from "react-i18next";
import type { UsageDashboard } from "@/types/backend";

const { Text } = Typography;

export function UsageCalendar({ data, days }: { data: UsageDashboard["trend"]; days: number }) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const byDate = new Map(data.map((row) => [row.date, row]));
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  start.setDate(start.getDate() - days + 1);
  const daily = Array.from({ length: days }, (_, index) => {
    const date = new Date(start);
    date.setDate(start.getDate() + index);
    const key = localDateKey(date);
    const row = byDate.get(key);
    const tokens = row ? row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens + row.outputTokens : 0;
    return { date, key, row, tokens };
  });
  const max = Math.max(...daily.map((item) => item.tokens), 0);
  const activeDays = daily.filter((item) => item.tokens > 0).length;
  const total = daily.reduce((sum, item) => sum + item.tokens, 0);
  const leading = Array.from({ length: daily[0]?.date.getDay() ?? 0 });
  const colors = [token.colorFillQuaternary, "#9be9a8", "#40c463", "#30a14e", "#216e39"];

  if (!data.length) return <Empty description={t("usage.noData")} />;

  return (
    <Space direction="vertical" size={14} style={{ width: "100%" }}>
      <Space wrap size={24}>
        <Statistic title={t("usage.activeDays")} value={activeDays} suffix={`/ ${days}`} />
        <Statistic title={t("usage.dailyPeak")} value={max} formatter={(value) => formatNumber(Number(value))} />
        <Statistic title={t("usage.calendarTotal")} value={total} formatter={(value) => formatNumber(Number(value))} />
      </Space>
      <div style={{ overflowX: "auto", paddingBottom: 2 }}>
        <div
          style={{
            display: "grid",
            gridTemplateRows: "repeat(7, 22px)",
            gridAutoFlow: "column",
            gridAutoColumns: "22px",
            gap: 6,
            width: "max-content",
          }}
        >
          {leading.map((_, index) => <span key={`leading-${index}`} />)}
          {daily.map((item) => {
            const level = item.tokens === 0 ? 0 : Math.min(4, Math.ceil((item.tokens / Math.max(max, 1)) * 4));
            const tooltip = item.row
              ? `${item.key}: ${formatNumber(item.tokens)} Token · ${item.row.requestCount} ${t("usage.requests")}`
              : `${item.key}: 0 Token`;
            return (
              <Tooltip key={item.key} title={tooltip}>
                <span
                  aria-label={tooltip}
                  style={{ width: 22, height: 22, borderRadius: 4, background: colors[level], outline: `1px solid ${token.colorBorderSecondary}` }}
                />
              </Tooltip>
            );
          })}
        </div>
      </div>
      <Space size={6} align="center" style={{ alignSelf: "flex-end" }}>
        <Text type="secondary" style={{ fontSize: 12 }}>{t("usage.calendarLess")}</Text>
        {colors.map((color, index) => (
          <span key={index} style={{ width: 12, height: 12, borderRadius: 2, background: color, outline: `1px solid ${token.colorBorderSecondary}` }} />
        ))}
        <Text type="secondary" style={{ fontSize: 12 }}>{t("usage.calendarMore")}</Text>
      </Space>
    </Space>
  );
}

function localDateKey(value: Date) {
  const year = value.getFullYear();
  const month = String(value.getMonth() + 1).padStart(2, "0");
  const day = String(value.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function formatNumber(value: number) {
  return new Intl.NumberFormat().format(value);
}
