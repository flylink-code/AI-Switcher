export type UsagePeriod = "24h" | "today" | 7 | 30 | 90 | 365;

export type UsagePeriodQuery = {
  days?: number;
  hours?: number;
  today?: boolean;
};

export const USAGE_PERIOD_VALUES: UsagePeriod[] = ["24h", "today", 7, 30, 90, 365];

export function usagePeriodToQuery(period: UsagePeriod): UsagePeriodQuery {
  switch (period) {
    case "24h":
      return { hours: 24 };
    case "today":
      return { today: true };
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

/** Calendar heatmap cell count for the selected period. */
export function usagePeriodToCalendarDays(period: UsagePeriod): number {
  switch (period) {
    case "24h":
    case "today":
      return 1;
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
