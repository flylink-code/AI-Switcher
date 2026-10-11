import { useEffect, useRef } from "react";
import { Tooltip } from "antd";
import { useTranslation } from "react-i18next";
import type { UsageTrendPoint } from "@/types/backend";
import { formatCompactNumber, formatFullNumber } from "@/utils/formatCompact";
import {
  localDateKey,
  usagePeriodHourKeys,
  usagePeriodToCalendarDays,
  type UsagePeriod,
} from "@/utils/usagePeriod";

export type TrendMetric = "requests" | "tokens" | "cost";

type OverviewPeriod = Extract<UsagePeriod, "today" | "24h" | 7 | 30>;

interface Bucket {
  key: string;
  row?: UsageTrendPoint;
  value: number;
  requests: number;
}

export function MetricTrendBars({
  data,
  period,
  metric,
}: {
  data: UsageTrendPoint[];
  period: OverviewPeriod;
  metric: TrendMetric;
}) {
  const { t } = useTranslation();
  const scrollerRef = useRef<HTMLDivElement>(null);
  const buckets = buildBuckets(data, period, metric);
  const max = Math.max(...buckets.map((item) => item.value), 0);
  const total = buckets.reduce((sum, item) => sum + item.value, 0);
  const average = buckets.length > 0 ? total / buckets.length : 0;
  const peakIndex = buckets.reduce((best, item, index) => (item.value > (buckets[best]?.value ?? -1) ? index : best), 0);
  const hourly = period === "today" || period === "24h";
  const labelStep = hourly ? 3 : Math.max(1, Math.ceil(buckets.length / 8));

  useEffect(() => {
    const node = scrollerRef.current;
    if (!node) return;
    node.scrollLeft = node.scrollWidth;
  }, [period, metric, buckets.length, data.length]);

  if (buckets.length === 0) return null;

  return (
    <div ref={scrollerRef} className={`metric-trend-bars ${hourly ? "metric-trend-bars-hour" : "metric-trend-bars-day"}`} style={{ marginTop: 8 }}>
      <div className="hourly-bars" role="img" aria-label={t("workbench.trendChart", { defaultValue: "用量趋势" })} style={{ height: 112 }}>
        {max > 0 && average > 0 ? (
          <span
            className="hourly-bars-refline"
            style={{ bottom: `calc((100% - var(--trend-label-band, 14px)) * ${average / max})` }}
          >
            <span className="hourly-bars-refline-label">
              {t("usage.chartAverage")} {formatMetric(average, metric)}
            </span>
          </span>
        ) : null}
        {buckets.map((item, index) => {
          const level = intensityLevel(item.value, max);
          const heightPct = max > 0 ? (item.value / max) * 100 : 0;
          const tooltip = trendTooltip(item, metric, t);
          const showPeak = index === peakIndex && item.value > 0;
          return (
            <Tooltip key={item.key} title={tooltip}>
              <button type="button" className="hourly-bars-col" aria-label={tooltip}>
                {showPeak ? <span className="hourly-bars-peak-label">{formatMetric(item.value, metric)}</span> : null}
                <span
                  className="hourly-bars-bar"
                  data-level={level || undefined}
                  style={{ height: `${heightPct}%`, minHeight: item.value > 0 ? 2 : 1 }}
                />
              </button>
            </Tooltip>
          );
        })}
      </div>
      <div className="hourly-bars-axis" aria-hidden>
        {buckets.map((item, index) => (
          <span key={item.key} className="hourly-bars-axis-label">
            {axisLabel(item.key, index, hourly, labelStep)}
          </span>
        ))}
      </div>
    </div>
  );
}

function buildBuckets(data: UsageTrendPoint[], period: OverviewPeriod, metric: TrendMetric): Bucket[] {
  const byDate = new Map(data.map((row) => [row.date, row]));
  const keys = period === "today" || period === "24h" ? usagePeriodHourKeys(period) : dayKeys(usagePeriodToCalendarDays(period));
  return keys.map((key) => {
    const row = byDate.get(key);
    return {
      key,
      row,
      value: row ? metricValue(row, metric) : 0,
      requests: row?.requestCount ?? 0,
    };
  });
}

function dayKeys(days: number): string[] {
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  start.setDate(start.getDate() - days + 1);
  return Array.from({ length: days }, (_, index) => {
    const date = new Date(start);
    date.setDate(start.getDate() + index);
    return localDateKey(date);
  });
}

function metricValue(row: UsageTrendPoint, metric: TrendMetric): number {
  switch (metric) {
    case "requests":
      return row.requestCount;
    case "tokens":
      return row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens + row.outputTokens;
    case "cost":
      return row.estimatedCost;
    default: {
      const _exhaustive: never = metric;
      return _exhaustive;
    }
  }
}

function formatMetric(value: number, metric: TrendMetric): string {
  if (metric === "cost") return value.toFixed(2);
  return formatCompactNumber(value);
}

function intensityLevel(value: number, max: number): number {
  if (value <= 0 || max <= 0) return 0;
  return Math.min(4, Math.ceil((value / max) * 4));
}

function axisLabel(key: string, index: number, hourly: boolean, labelStep: number): string {
  if (hourly) {
    const hour = Number(key.slice(11, 13));
    return Number.isFinite(hour) && hour % 3 === 0 ? String(hour).padStart(2, "0") : "";
  }
  return index % labelStep === 0 ? key.slice(5) : "";
}

function trendTooltip(
  item: Bucket,
  metric: TrendMetric,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  const value = formatMetric(item.value, metric);
  const exact = metric === "cost" ? item.value.toFixed(2) : formatFullNumber(item.value);
  return t("workbench.trendTooltip", {
    time: item.key,
    value: exact,
    compact: value,
    requests: item.requests,
    defaultValue: "{{time}} · {{value}} · {{requests}} 次请求",
  });
}
