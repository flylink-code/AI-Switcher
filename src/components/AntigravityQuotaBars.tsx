import type { AntigravityAccountPublic } from "@/services/api";

function quotaBarColor(percent: number | null | undefined): string {
  if (percent == null) return "var(--ant-color-text-quaternary, #bfbfbf)";
  if (percent >= 50) return "#34c759";
  if (percent >= 20) return "#ff9f0a";
  return "#ff3b30";
}

export function QuotaMiniBar({
  label,
  percent,
}: {
  label: string;
  percent: number | null | undefined;
}) {
  const value = percent == null ? null : Math.max(0, Math.min(100, percent));
  const color = quotaBarColor(value);
  return (
    <div className="ag-quota-bar" title={value == null ? label : `${label}: ${value}%`}>
      <div className="ag-quota-bar-track">
        <div
          className="ag-quota-bar-fill"
          style={{
            width: value == null ? "0%" : `${value}%`,
            background: color,
          }}
        />
      </div>
      <div className="ag-quota-bar-meta">
        <span>{label}</span>
        <span style={{ color, fontWeight: 600 }}>
          {value == null ? "—" : `${value}%`}
        </span>
      </div>
    </div>
  );
}

export function tierTagColor(tier: string | null | undefined): string {
  const value = (tier ?? "").toLowerCase();
  if (value.includes("ultra")) return "magenta";
  if (value.includes("pro")) return "blue";
  if (value.includes("free")) return "default";
  return "default";
}

export function formatTierLabel(tier: string | null | undefined): string | null {
  if (!tier?.trim()) return null;
  const upper = tier.trim().toUpperCase();
  if (upper.includes("ULTRA")) return "ULTRA";
  if (upper.includes("PRO")) return "PRO";
  if (upper.includes("FREE")) return "FREE";
  return tier.trim();
}

export function accountQuotaSummary(account: AntigravityAccountPublic): {
  geminiFiveHour: number | null;
  geminiWeekly: number | null;
  claudeFiveHour: number | null;
  claudeWeekly: number | null;
} {
  return {
    geminiFiveHour: account.quotaGemini5hPercent ?? null,
    geminiWeekly: account.quotaGeminiWeeklyPercent ?? null,
    claudeFiveHour: account.quotaClaude5hPercent ?? null,
    claudeWeekly: account.quotaClaudeWeeklyPercent ?? null,
  };
}
