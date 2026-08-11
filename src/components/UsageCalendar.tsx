import { useEffect, useMemo, useRef, useState } from "react";
import { Empty, Space, Statistic, Tooltip, Typography, theme } from "antd";
import { useTranslation } from "react-i18next";
import type { UsageDashboard } from "@/types/backend";
import { formatCompactNumber, formatFullNumber } from "@/utils/formatCompact";
import {
  localDateKey,
  usagePeriodGranularity,
  usagePeriodHourKeys,
  usagePeriodToCalendarDays,
  type UsagePeriod,
} from "@/utils/usagePeriod";

const { Text } = Typography;

/** Industry-standard contribution-graph sizing (HeatmapKit / calendar-heatmap). */
const CELL_GAP = 3;
const WEEKDAY_GUTTER = 28;
const DAY_CELL = { min: 10, max: 18 } as const;

type Level = { color: string; label: string };

type DayCell = {
  date: Date;
  key: string;
  row: UsageDashboard["trend"][number] | undefined;
  tokens: number;
};

export function UsageCalendar({
  data,
  period,
  orientation = "horizontal",
  compact = false,
  maxCellSize,
}: {
  data: UsageDashboard["trend"];
  period: UsagePeriod;
  /** vertical = 7 weekday columns × N week rows, scrolls down (narrow rails). */
  orientation?: "horizontal" | "vertical";
  /** compact hides the summary Statistic row (workbench rail). */
  compact?: boolean;
  /** Caps day-cell growth below DAY_CELL.max (overview heatmap stays a quiet ~150-180px strip). */
  maxCellSize?: number;
}) {
  const { i18n, t } = useTranslation();
  const { token } = theme.useToken();
  const granularity = usagePeriodGranularity(period);
  const byDate = new Map(data.map((row) => [row.date, row]));
  const levels: Level[] = [
    { color: token.colorFillQuaternary, label: t("usage.calendarLevelNone") },
    { color: token.colorSuccessBg, label: t("usage.calendarLevelOne") },
    { color: token.colorSuccessBgHover, label: t("usage.calendarLevelTwo") },
    { color: token.colorSuccessBorder, label: t("usage.calendarLevelThree") },
    { color: token.colorSuccess, label: t("usage.calendarLevelFour") },
  ];
  const monthFormatter = useMemo(
    () => new Intl.DateTimeFormat(i18n.language || undefined, { month: "short" }),
    [i18n.language],
  );

  if (granularity === "hour") {
    return (
      <HourlyHeatmap
        period={period === "today" ? "today" : "24h"}
        byDate={byDate}
        t={t}
        compact={compact}
      />
    );
  }

  const days = usagePeriodToCalendarDays(period);
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  start.setDate(start.getDate() - days + 1);
  const daily = Array.from({ length: days }, (_, index) => {
    const date = new Date(start);
    date.setDate(start.getDate() + index);
    const key = localDateKey(date);
    const row = byDate.get(key);
    const tokens = row
      ? row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens + row.outputTokens
      : 0;
    return { date, key, row, tokens };
  });
  const max = Math.max(...daily.map((item) => item.tokens), 0);
  const activeDays = daily.filter((item) => item.tokens > 0).length;
  const total = daily.reduce((sum, item) => sum + item.tokens, 0);

  if (!data.length && daily.every((item) => item.tokens === 0)) {
    return <Empty description={t("usage.noData")} />;
  }

  return (
    <Space direction="vertical" size={14} style={{ width: "100%" }}>
      {compact ? null : (
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
      )}
      <ContributionHeatmap
        daily={daily}
        max={max}
        levels={levels}
        border={token.colorBorderSecondary}
        requestsLabel={t("usage.requests")}
        ariaLabel={t("usage.dailyStatistics")}
        orientation={orientation}
        maxCellSize={maxCellSize ?? DAY_CELL.max}
        weekdayLabels={[
          t("usage.weekdaySun"),
          t("usage.weekdayMon"),
          t("usage.weekdayTue"),
          t("usage.weekdayWed"),
          t("usage.weekdayThu"),
          t("usage.weekdayFri"),
          t("usage.weekdaySat"),
        ]}
        formatMonth={(date) => monthFormatter.format(date)}
      />
      <CalendarLegend
        levels={levels}
        legend={t("usage.calendarLegend")}
        border={token.colorBorderSecondary}
        compact={compact}
      />
    </Space>
  );
}

/** Max visible height of the vertical heatmap scroller (px). */
const VERTICAL_MAX_HEIGHT = 420;

/**
 * GitHub-style week columns with fit-to-width cell sizing.
 * Caps cell growth so short ranges stay proportional; scrolls + pins to the
 * latest week when the range is wider than the container.
 * Vertical orientation transposes the grid (7 weekday columns × N week rows)
 * for narrow rails, pinning the scroller to the newest week at the bottom.
 */
function ContributionHeatmap({
  daily,
  max,
  levels,
  border,
  requestsLabel,
  ariaLabel,
  weekdayLabels,
  formatMonth,
  orientation = "horizontal",
  maxCellSize = DAY_CELL.max,
}: {
  daily: DayCell[];
  max: number;
  levels: Level[];
  border: string;
  requestsLabel: string;
  ariaLabel: string;
  weekdayLabels: string[];
  formatMonth: (date: Date) => string;
  orientation?: "horizontal" | "vertical";
  maxCellSize?: number;
}) {
  const scrollerRef = useRef<HTMLDivElement>(null);
  const [viewportWidth, setViewportWidth] = useState(0);

  const isVertical = orientation === "vertical";
  const leading = daily[0]?.date.getDay() ?? 0;
  const weekCount = Math.ceil((leading + daily.length) / 7);
  // Horizontal: one column per week. Vertical: fixed 7 weekday columns.
  const trackCount = isVertical ? 7 : weekCount;

  useEffect(() => {
    const node = scrollerRef.current;
    if (!node) return;
    const update = () => setViewportWidth(node.clientWidth);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const cellSize = useMemo(() => {
    const usable = Math.max(viewportWidth - WEEKDAY_GUTTER, 0);
    return fitCellSize(usable, trackCount, CELL_GAP, DAY_CELL.min, maxCellSize);
  }, [trackCount, viewportWidth, maxCellSize]);

  const gridWidth = trackCount * cellSize + Math.max(trackCount - 1, 0) * CELL_GAP;
  const gridHeight = weekCount * cellSize + Math.max(weekCount - 1, 0) * CELL_GAP;
  const needsScroll =
    !isVertical && viewportWidth > 0 && gridWidth + WEEKDAY_GUTTER > viewportWidth + 1;

  useEffect(() => {
    if (isVertical) return;
    const node = scrollerRef.current;
    if (!node || !needsScroll) return;
    node.scrollLeft = node.scrollWidth;
  }, [isVertical, needsScroll, cellSize, weekCount, daily.length]);

  // Vertical: pin the scroller to the newest week (bottom) on first layout.
  useEffect(() => {
    if (!isVertical) return;
    const node = scrollerRef.current;
    if (!node) return;
    node.scrollTop = node.scrollHeight;
  }, [isVertical, cellSize, weekCount, daily.length]);

  const monthLabels = useMemo(
    () => buildMonthLabels(daily, leading, cellSize, formatMonth),
    [daily, leading, cellSize, formatMonth],
  );
  const verticalMonthLabels = useMemo(
    () => buildVerticalMonthLabels(daily, leading, weekCount, cellSize, formatMonth),
    [daily, leading, weekCount, cellSize, formatMonth],
  );

  const renderDayCell = (item: DayCell) => {
    const level = intensityLevel(item.tokens, max);
    const tooltip = item.row
      ? `${item.key}: ${formatCompactNumber(item.tokens)} Token (${formatFullNumber(item.tokens)}) · ${item.row.requestCount} ${requestsLabel}`
      : `${item.key}: 0 Token`;
    return (
      <Tooltip key={item.key} title={tooltip}>
        <button
          type="button"
          role="gridcell"
          aria-label={tooltip}
          style={{
            width: cellSize,
            height: cellSize,
            padding: 0,
            border: `1px solid ${border}`,
            borderRadius: 2,
            background: levels[level].color,
            cursor: "default",
          }}
        />
      </Tooltip>
    );
  };

  if (isVertical) {
    return (
      <div style={{ width: "100%", minWidth: 0 }}>
        <div
          ref={scrollerRef}
          style={{
            width: "100%",
            maxHeight: VERTICAL_MAX_HEIGHT,
            overflowY: "auto",
            overflowX: "hidden",
            paddingRight: 4,
          }}
        >
          <div
            style={{
              display: "grid",
              gridTemplateColumns: `${WEEKDAY_GUTTER}px ${gridWidth}px`,
              columnGap: 0,
              width: gridWidth + WEEKDAY_GUTTER,
              margin: "0 auto",
            }}
          >
            <span />
            <div
              aria-hidden
              style={{
                display: "grid",
                gridTemplateColumns: `repeat(7, ${cellSize}px)`,
                gap: CELL_GAP,
                height: 14,
                marginBottom: 6,
              }}
            >
              {weekdayLabels.map((label, index) => (
                <Text
                  key={label}
                  type="secondary"
                  style={{
                    fontSize: 10,
                    lineHeight: "14px",
                    textAlign: "center",
                    // Match GitHub: only Mon / Wed / Fri.
                    visibility: index === 1 || index === 3 || index === 5 ? "visible" : "hidden",
                  }}
                >
                  {label}
                </Text>
              ))}
            </div>
            <div aria-hidden style={{ position: "relative", height: gridHeight }}>
              {verticalMonthLabels.map((label) => (
                <Text
                  key={`${label.text}-${label.top}`}
                  type="secondary"
                  style={{
                    position: "absolute",
                    left: 0,
                    top: label.top,
                    fontSize: 10,
                    lineHeight: `${cellSize}px`,
                    whiteSpace: "nowrap",
                  }}
                >
                  {label.text}
                </Text>
              ))}
            </div>
            <div
              role="grid"
              aria-label={ariaLabel}
              style={{
                display: "grid",
                gridTemplateColumns: `repeat(7, ${cellSize}px)`,
                gridAutoFlow: "row",
                gap: CELL_GAP,
                width: gridWidth,
              }}
            >
              {Array.from({ length: leading }).map((_, index) => (
                <span key={`leading-${index}`} />
              ))}
              {daily.map(renderDayCell)}
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div style={{ width: "100%", minWidth: 0 }}>
      <div
        ref={scrollerRef}
        style={{
          width: "100%",
          overflowX: needsScroll ? "auto" : "hidden",
          /* overflow-x non-visible computes overflow-y to auto — pin it
             back or a phantom vertical scrollbar appears. */
          overflowY: "hidden",
          paddingBottom: needsScroll ? 4 : 0,
        }}
      >
        <div
          style={{
            display: "inline-flex",
            flexDirection: "column",
            gap: 6,
            minWidth: needsScroll ? gridWidth + WEEKDAY_GUTTER : "100%",
            width: needsScroll ? undefined : "100%",
            alignItems: needsScroll ? "flex-start" : "center",
          }}
        >
          <div
            style={{
              display: "grid",
              gridTemplateColumns: `${WEEKDAY_GUTTER}px ${gridWidth}px`,
              columnGap: 0,
              width: gridWidth + WEEKDAY_GUTTER,
            }}
          >
            <span />
            <div style={{ position: "relative", height: 14, width: gridWidth }}>
              {monthLabels.map((label) => (
                <Text
                  key={`${label.text}-${label.left}`}
                  type="secondary"
                  style={{
                    position: "absolute",
                    left: label.left,
                    top: 0,
                    fontSize: 10,
                    lineHeight: "14px",
                    whiteSpace: "nowrap",
                  }}
                >
                  {label.text}
                </Text>
              ))}
            </div>
            <div
              aria-hidden
              style={{
                display: "grid",
                gridTemplateRows: `repeat(7, ${cellSize}px)`,
                gap: CELL_GAP,
                alignContent: "start",
              }}
            >
              {weekdayLabels.map((label, index) => (
                <Text
                  key={label}
                  type="secondary"
                  style={{
                    fontSize: 10,
                    lineHeight: `${cellSize}px`,
                    height: cellSize,
                    // Match GitHub: only Mon / Wed / Fri.
                    visibility: index === 1 || index === 3 || index === 5 ? "visible" : "hidden",
                  }}
                >
                  {label}
                </Text>
              ))}
            </div>
            <div
              role="grid"
              aria-label={ariaLabel}
              style={{
                display: "grid",
                gridTemplateRows: `repeat(7, ${cellSize}px)`,
                gridTemplateColumns: `repeat(${weekCount}, ${cellSize}px)`,
                gridAutoFlow: "column",
                gap: CELL_GAP,
                width: gridWidth,
              }}
            >
              {Array.from({ length: leading }).map((_, index) => (
                <span key={`leading-${index}`} />
              ))}
              {daily.map(renderDayCell)}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * Guide §3.3: hourly usage as a bar chart — one bar per hour, color banded by
 * share of the period peak, peak value labeled, tooltip on hover.
 */
function HourlyHeatmap({
  period,
  byDate,
  t,
  compact = false,
}: {
  period: "24h" | "today";
  byDate: Map<string, UsageDashboard["trend"][number]>;
  t: (key: string, options?: Record<string, unknown>) => string;
  compact?: boolean;
}) {
  const keys = usagePeriodHourKeys(period);
  const hourly = keys.map((key) => {
    const row = byDate.get(key);
    const tokens = row
      ? row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens + row.outputTokens
      : 0;
    return { key, row, tokens };
  });
  const max = Math.max(...hourly.map((item) => item.tokens), 0);
  const activeHours = hourly.filter((item) => item.tokens > 0).length;
  const total = hourly.reduce((sum, item) => sum + item.tokens, 0);
  const avg = total / keys.length;

  return (
    <Space direction="vertical" size={14} style={{ width: "100%" }}>
      {compact ? null : (
        <Space wrap size={24}>
          <Statistic title={t("usage.activeHours")} value={activeHours} suffix={`/ ${keys.length}`} />
          <Statistic
            title={t("usage.hourlyPeak")}
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
      )}
      <div>
        <div
          role="img"
          aria-label={t("usage.hourlyStatistics")}
          className="hourly-bars"
        >
          {max > 0 && avg > 0 ? (
            <span
              className="hourly-bars-refline"
              style={{ bottom: `calc((100% - 14px) * ${(avg / max) * 100} / 100)` }}
            >
              <span className="hourly-bars-refline-label">
                {t("usage.chartAverage")} {formatCompactNumber(avg)}
              </span>
            </span>
          ) : null}
          {hourly.map((item) => {
            const level = intensityLevel(item.tokens, max);
            const isPeak = max > 0 && item.tokens === max;
            const heightPct = max > 0 ? (item.tokens / max) * 100 : 0;
            const peakPct = max > 0 ? Math.round((item.tokens / max) * 100) : 0;
            const tooltip = item.row
              ? `${item.key}: ${formatFullNumber(item.tokens)} Token · ${item.row.requestCount} ${t("usage.requests")} · ${t("usage.hourlyPeakShare", { pct: peakPct })}`
              : `${item.key}: 0 Token`;
            return (
              <Tooltip key={item.key} title={tooltip}>
                <button
                  type="button"
                  aria-label={tooltip}
                  className="hourly-bars-col"
                >
                  {isPeak ? (
                    <span className="hourly-bars-peak-label">
                      {formatCompactNumber(item.tokens)}
                    </span>
                  ) : null}
                  <span
                    className="hourly-bars-bar"
                    data-level={level}
                    style={{
                      height: `${heightPct}%`,
                      minHeight: item.tokens > 0 ? 2 : 1,
                    }}
                  />
                </button>
              </Tooltip>
            );
          })}
        </div>
        <div className="hourly-bars-axis" aria-hidden>
          {hourly.map((item) => {
            const hour = item.key.slice(11, 13);
            return (
              <span key={item.key} className="hourly-bars-axis-label">
                {Number(hour) % 3 === 0 ? hour : ""}
              </span>
            );
          })}
        </div>
      </div>
    </Space>
  );
}

/**
 * Period-following bar chart for the workbench stats rail: hourly bars for
 * 24h/today, daily bars for day-granularity periods.
 */
export function UsageTrendBars({
  data,
  period,
  compact = false,
}: {
  data: UsageDashboard["trend"];
  period: UsagePeriod;
  /** compact hides the summary Statistic row (workbench rail). */
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const byDate = new Map(data.map((row) => [row.date, row]));
  if (usagePeriodGranularity(period) === "hour") {
    return (
      <HourlyHeatmap
        period={period === "today" ? "today" : "24h"}
        byDate={byDate}
        t={t}
        compact={compact}
      />
    );
  }
  return <DailyBars period={period} byDate={byDate} t={t} compact={compact} />;
}

/**
 * Day-granularity bar chart: one bar per day, reusing the hourly-bars CSS
 * system. Scrolls horizontally when the period has more days than fit.
 */
function DailyBars({
  period,
  byDate,
  t,
  compact = false,
}: {
  period: UsagePeriod;
  byDate: Map<string, UsageDashboard["trend"][number]>;
  t: (key: string, options?: Record<string, unknown>) => string;
  compact?: boolean;
}) {
  const days = usagePeriodToCalendarDays(period);
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  start.setDate(start.getDate() - days + 1);
  const daily = Array.from({ length: days }, (_, index) => {
    const date = new Date(start);
    date.setDate(start.getDate() + index);
    const key = localDateKey(date);
    const row = byDate.get(key);
    const tokens = row
      ? row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens + row.outputTokens
      : 0;
    return { key, row, tokens };
  });
  const max = Math.max(...daily.map((item) => item.tokens), 0);
  const activeDays = daily.filter((item) => item.tokens > 0).length;
  const total = daily.reduce((sum, item) => sum + item.tokens, 0);
  const avg = total / days;
  const labelStep = Math.max(1, Math.ceil(days / 12));
  const scrollerRef = useRef<HTMLDivElement>(null);

  // Long ranges scroll horizontally — pin to the newest days on the right.
  // byDate.size in deps: the scroller only mounts after data arrives
  // (empty state renders <Empty/> first), so re-pin when data lands.
  useEffect(() => {
    const node = scrollerRef.current;
    if (!node) return;
    node.scrollLeft = node.scrollWidth;
  }, [days, byDate.size]);

  if (!byDate.size && daily.every((item) => item.tokens === 0)) {
    return <Empty description={t("usage.noData")} />;
  }

  return (
    <Space direction="vertical" size={14} style={{ width: "100%" }}>
      {compact ? null : (
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
      )}
      <div ref={scrollerRef} style={{ overflowX: "auto", overflowY: "hidden" }}>
        {/* 10px per day slot (8px min col + 2px gap) matches the axis labels. */}
        <div style={{ width: "100%", minWidth: days * 10 }}>
          <div
            role="img"
            aria-label={t("usage.dailyStatistics")}
            className="hourly-bars"
          >
            {max > 0 && avg > 0 ? (
              <span
                className="hourly-bars-refline"
                style={{ bottom: `calc((100% - 14px) * ${(avg / max) * 100} / 100)` }}
              >
                <span className="hourly-bars-refline-label">
                  {t("usage.chartAverage")} {formatCompactNumber(avg)}
                </span>
              </span>
            ) : null}
            {daily.map((item) => {
              const level = intensityLevel(item.tokens, max);
              const isPeak = max > 0 && item.tokens === max;
              const heightPct = max > 0 ? (item.tokens / max) * 100 : 0;
              const peakPct = max > 0 ? Math.round((item.tokens / max) * 100) : 0;
              const tooltip = item.row
                ? `${item.key}: ${formatFullNumber(item.tokens)} Token · ${item.row.requestCount} ${t("usage.requests")} · ${t("usage.hourlyPeakShare", { pct: peakPct })}`
                : `${item.key}: 0 Token`;
              return (
                <Tooltip key={item.key} title={tooltip}>
                  <button
                    type="button"
                    aria-label={tooltip}
                    className="hourly-bars-col"
                  >
                    {isPeak ? (
                      <span className="hourly-bars-peak-label">
                        {formatCompactNumber(item.tokens)}
                      </span>
                    ) : null}
                    <span
                      className="hourly-bars-bar"
                      data-level={level}
                      style={{
                        height: `${heightPct}%`,
                        minHeight: item.tokens > 0 ? 2 : 1,
                      }}
                    />
                  </button>
                </Tooltip>
              );
            })}
          </div>
          <div className="hourly-bars-axis" aria-hidden>
            {daily.map((item, index) => (
              <span key={item.key} className="hourly-bars-axis-label">
                {index % labelStep === 0 ? item.key.slice(5) : ""}
              </span>
            ))}
          </div>
        </div>
      </div>
    </Space>
  );
}

function fitCellSize(
  availableWidth: number,
  columns: number,
  gap: number,
  min: number,
  max: number,
): number {
  if (columns <= 0) return max;
  if (availableWidth <= 0) return max;
  const raw = Math.floor((availableWidth - gap * Math.max(columns - 1, 0)) / columns);
  return Math.min(max, Math.max(min, raw));
}

function intensityLevel(tokens: number, max: number): number {
  if (tokens <= 0) return 0;
  return Math.min(4, Math.ceil((tokens / Math.max(max, 1)) * 4));
}

function buildMonthLabels(
  daily: DayCell[],
  leading: number,
  cellSize: number,
  formatMonth: (date: Date) => string,
): Array<{ text: string; left: number }> {
  const labels: Array<{ text: string; left: number }> = [];
  let lastMonth = -1;
  daily.forEach((item, index) => {
    const month = item.date.getMonth();
    if (month === lastMonth) return;
    lastMonth = month;
    const column = Math.floor((leading + index) / 7);
    labels.push({
      text: formatMonth(item.date),
      left: column * (cellSize + CELL_GAP),
    });
  });
  return labels;
}

/** Month gutter labels for the vertical layout: one label per week row that opens a new month. */
function buildVerticalMonthLabels(
  daily: DayCell[],
  leading: number,
  weekCount: number,
  cellSize: number,
  formatMonth: (date: Date) => string,
): Array<{ text: string; top: number }> {
  const labels: Array<{ text: string; top: number }> = [];
  let lastMonth = -1;
  for (let row = 0; row < weekCount; row++) {
    // First existing day of this week row (row 0 may start mid-week).
    const dayIndex = Math.max(row * 7 - leading, 0);
    if (dayIndex >= daily.length) break;
    const month = daily[dayIndex].date.getMonth();
    if (month === lastMonth) continue;
    lastMonth = month;
    labels.push({
      text: formatMonth(daily[dayIndex].date),
      top: row * (cellSize + CELL_GAP),
    });
  }
  return labels;
}

function CalendarLegend({
  levels,
  legend,
  border,
  compact = false,
}: {
  levels: Level[];
  legend: string;
  border: string;
  /** compact drops the long prefix text and shrinks swatches (overview strip). */
  compact?: boolean;
}) {
  const swatch = compact ? 10 : 12;
  return (
    <Space wrap size={[10, 4]} align="center" style={{ alignSelf: "flex-end", justifyContent: "flex-end" }}>
      {compact ? null : (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {legend}
        </Text>
      )}
      {levels.map((level) => (
        <Space key={level.label} size={4} align="center">
          <span
            aria-hidden="true"
            style={{
              display: "inline-block",
              flex: `0 0 ${swatch}px`,
              width: swatch,
              height: swatch,
              borderRadius: 2,
              background: level.color,
              border: `1px solid ${border}`,
            }}
          />
          <Text type="secondary" style={{ fontSize: compact ? 11 : 12 }}>
            {level.label}
          </Text>
        </Space>
      ))}
    </Space>
  );
}
