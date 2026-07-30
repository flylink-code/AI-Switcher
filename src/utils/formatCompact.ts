/** Compact display for large token / count values (1.2k, 3.4M). */
export function formatCompactNumber(value: number): string {
  const absolute = Math.abs(value);
  if (!Number.isFinite(value)) return String(value);
  if (absolute >= 1_000_000_000) {
    return trimCompact(value / 1_000_000_000) + "B";
  }
  if (absolute >= 1_000_000) {
    return trimCompact(value / 1_000_000) + "M";
  }
  if (absolute >= 1_000) {
    return trimCompact(value / 1_000) + "k";
  }
  return new Intl.NumberFormat().format(value);
}

function trimCompact(value: number): string {
  const rounded = Math.abs(value) >= 100 ? value.toFixed(0) : value.toFixed(1);
  return rounded.replace(/\.0$/, "");
}

/** Full locale grouping for tooltips / exact values. */
export function formatFullNumber(value: number): string {
  return new Intl.NumberFormat().format(value);
}
