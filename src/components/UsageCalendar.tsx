import { Empty, Space, Statistic, Tooltip, Typography, theme } from "antd";
import { useTranslation } from "react-i18next";
import type { UsageDashboard } from "@/types/backend";
import { formatCompactNumber, formatFullNumber } from "@/utils/formatCompact";

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
  const columns = Math.ceil((leading.length + daily.length) / 7);
  const columnGaps = Math.max(columns - 1, 0);
  const gridMaxWidth = columns * 36 + columnGaps * 6;
  const levels = [
    { color: token.colorFillQuaternary, label: t("usage.calendarLevelNone") },
    { color: token.colorSuccessBg, label: t("usage.calendarLevelOne") },
    { color: token.colorSuccessBgHover, label: t("usage.calendarLevelTwo") },
    { color: token.colorSuccessBorder, label: t("usage.calendarLevelThree") },
    { color: token.colorSuccess, label: t("usage.calendarLevelFour") },
  ];

  if (!data.length) return <Empty description={t("usage.noData")} />;

  return (
    <Space direction="vertical" size={14} style={{ width: "100%" }}>
      <Space wrap size={24}>
        <Statistic title={t("usage.activeDays")} value={activeDays} suffix={`/ ${days}`} />
        <Statistic
          title={t("usage.dailyPeak")}
          value={max}
          formatter={(value) => (
            <Tooltip title={formatFullNumber(Number(value))}>{formatCompactNumber(Number(value))}</Tooltip>
          )}
        />
        <Statistic
          title={t("usage.calendarTotal")}
          value={total}
          formatter={(value) => (
            <Tooltip title={formatFullNumber(Number(value))}>{formatCompactNumber(Number(value))}</Tooltip>
          )}
        />
      </Space>
      <div style={{ width: "100%", minWidth: 0, overflow: "hidden" }}>
        <div
          role="grid"
          aria-label={t("usage.dailyStatistics")}
          style={{
            display: "grid",
            gridTemplateRows: "repeat(7, auto)",
            gridTemplateColumns: `repeat(${columns}, minmax(8px, 1fr))`,
            gridAutoFlow: "column",
            gap: "clamp(2px, 0.35vw, 6px)",
            width: "100%",
            maxWidth: gridMaxWidth,
          }}
        >
          {leading.map((_, index) => <span key={`leading-${index}`} />)}
          {daily.map((item) => {
            const level = item.tokens === 0 ? 0 : Math.min(4, Math.ceil((item.tokens / Math.max(max, 1)) * 4));
            const tooltip = item.row
              ? `${item.key}: ${formatCompactNumber(item.tokens)} Token (${formatFullNumber(item.tokens)}) · ${item.row.requestCount} ${t("usage.requests")}`
              : `${item.key}: 0 Token`;
            return (
              <Tooltip key={item.key} title={tooltip}>
                <button
                  type="button"
                  role="gridcell"
                  aria-label={tooltip}
                  style={{
                    display: "block",
                    width: "100%",
                    height: "auto",
                    aspectRatio: "1 / 1",
                    padding: 0,
                    border: `1px solid ${token.colorBorderSecondary}`,
                    borderRadius: 3,
                    background: levels[level].color,
                    cursor: "default",
                  }}
                />
              </Tooltip>
            );
          })}
        </div>
      </div>
      <Space wrap size={[10, 6]} align="center" style={{ alignSelf: "flex-end", justifyContent: "flex-end" }}>
        <Text type="secondary" style={{ fontSize: 12 }}>{t("usage.calendarLegend")}</Text>
        {levels.map((level) => (
          <Space key={level.label} size={4} align="center">
            <span
              aria-hidden="true"
              style={{
                display: "inline-block",
                flex: "0 0 12px",
                width: 12,
                height: 12,
                borderRadius: 2,
                background: level.color,
                border: `1px solid ${token.colorBorderSecondary}`,
              }}
            />
            <Text type="secondary" style={{ fontSize: 12 }}>{level.label}</Text>
          </Space>
        ))}
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
