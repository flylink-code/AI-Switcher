export type UsagePeriod = "24h" | "today" | 2 | 4 | 7 | 30 | 90 | 365;
export type UsageTrendGranularity = "hour" | "day";

export type UsagePeriodQuery = {
  days?: number;
  hours?: number;
  today?: boolean;
};

export const USAGE_PERIOD_VALUES: UsagePeriod[] = ["24h", "today", 2, 4, 7, 30, 90, 365];

export function usagePeriodToQuery(period: UsagePeriod): UsagePeriodQuery {
  switch (period) {
    case "24h":
      return { hours: 24 };
    case "today":
      return { today: true };
    case 2:
    case 4:
    case 7:
    case 30:
    case 90:
    case 365:
      return { days: period };
    default: {
      const _exhaustive: never = period;
      return _exhaustive;
    }
  }
}

export function usagePeriodGranularity(period: UsagePeriod): UsageTrendGranularity {
  switch (period) {
    case "24h":
    case "today":
      return "hour";
    case 2:
    case 4:
    case 7:
    case 30:
    case 90:
    case 365:
      return "day";
    default: {
      const _exhaustive: never = period;
      return _exhaustive;
    }
  }
}

/** Calendar heatmap cell count for day-granularity periods. */
export function usagePeriodToCalendarDays(period: UsagePeriod): number {
  switch (period) {
    case "24h":
    case "today":
      return 1;
    case 2:
    case 4:
    case 7:
    case 30:
    case 90:
    case 365:
      return period;
    default: {
      const _exhaustive: never = period;
      return _exhaustive;
    }
  }
}

export function usagePeriodLabelKey(period: UsagePeriod): string {
  switch (period) {
    case "24h":
      return "usage.last24Hours";
    case "today":
      return "usage.today";
    case 2:
    case 4:
    case 7:
    case 30:
    case 90:
    case 365:
      return "usage.lastDays";
    default: {
      const _exhaustive: never = period;
      return _exhaustive;
    }
  }
}

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

export function localHourKey(value: Date): string {
  return `${value.getFullYear()}-${pad2(value.getMonth() + 1)}-${pad2(value.getDate())} ${pad2(value.getHours())}:00`;
}

export function localDateKey(value: Date): string {
  return `${value.getFullYear()}-${pad2(value.getMonth() + 1)}-${pad2(value.getDate())}`;
}

/** Build consecutive hour keys for the selected short period. */
export function usagePeriodHourKeys(period: Extract<UsagePeriod, "24h" | "today">): string[] {
  const now = new Date();
  now.setMinutes(0, 0, 0);
  if (period === "today") {
    const start = new Date(now);
    start.setHours(0, 0, 0, 0);
    const count = Math.max(now.getHours() - start.getHours() + 1, 1);
    return Array.from({ length: count }, (_, index) => {
      const hour = new Date(start);
      hour.setHours(start.getHours() + index);
      return localHourKey(hour);
    });
  }
  return Array.from({ length: 24 }, (_, index) => {
    const hour = new Date(now);
    hour.setHours(now.getHours() - (23 - index));
    return localHourKey(hour);
  });
}

export function trendBucketLabel(date: string, granularity: UsageTrendGranularity): string {
  if (granularity === "hour") {
    const match = date.match(/(\d{2}):00$/);
    if (match) return `${match[1]}:00`;
    return date.length >= 13 ? date.slice(11, 16) : date;
  }
  return date.length >= 10 ? date.slice(5) : date;
}
