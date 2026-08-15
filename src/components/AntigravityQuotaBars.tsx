import type { AntigravityAccountPublic, AntigravityQuotaBucket } from "@/services/api";

function quotaBarColor(percent: number | null | undefined): string {
  if (percent == null) return "var(--ant-color-text-quaternary, #bfbfbf)";
  if (percent >= 50) return "#34c759";
  if (percent >= 20) return "#ff9f0a";
  return "#ff3b30";
}

/// ISO resetTime → 紧凑本地时间：24h 内只显时分，跨天补日期。
export function formatQuotaResetTime(resetTime: string | null | undefined): string | null {
  if (!resetTime) return null;
  const date = new Date(resetTime);
  if (Number.isNaN(date.getTime())) return null;
  const pad = (n: number) => String(n).padStart(2, "0");
  const hm = `${pad(date.getHours())}:${pad(date.getMinutes())}`;
  if (date.getTime() - Date.now() < 24 * 3600 * 1000) return hm;
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${hm}`;
}

/// quotaUpdatedAt（unix 秒）→ 本地时分秒。
export function formatQuotaUpdatedAt(updatedAt: number | null | undefined): string | null {
  if (!updatedAt) return null;
  const date = new Date(updatedAt * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

export function QuotaMiniBar({
  label,
  percent,
  resetTime,
}: {
  label: string;
  percent: number | null | undefined;
  resetTime?: string | null;
}) {
  const value = percent == null ? null : Math.max(0, Math.min(100, percent));
  const color = quotaBarColor(value);
  const reset = formatQuotaResetTime(resetTime);
  const title = value == null ? label : `${label}: ${value}%${reset ? ` · ↻ ${reset}` : ""}`;
  return (
    <div className="ag-quota-bar" title={title}>
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
          {reset && (
            <span style={{ color: "var(--ant-color-text-quaternary, #bfbfbf)", fontWeight: 400, marginLeft: 4, fontSize: 10 }}>
              {reset}
            </span>
          )}
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

/// 与后端 normalize_quota_window 同逻辑：`weekly`/`7d`/`week` → weekly，
/// `5h`/`5hr`/`session` → 5h；window 为空时从 bucketId 推断。
function normalizeQuotaWindow(window: string, bucketId = ""): string {
  const haystack = `${window} ${bucketId}`.toLowerCase();
  if (/(weekly|\bweek\b|7d|7-day|7day|168h)/.test(haystack)) return "weekly";
  if (/(5h|5hr|five[-_]?hour|session)/.test(haystack)) return "5h";
  return window.trim().toLowerCase();
}

type QuotaFamily = "gemini" | "claudeGpt";

function bucketLooksGemini(bucketId: string): boolean {
  return bucketId.toLowerCase().includes("gemini");
}

function bucketLooksClaudeGpt(bucketId: string): boolean {
  const id = bucketId.toLowerCase();
  return (
    id.startsWith("3p-") ||
    id.includes("claude") ||
    id.includes("gpt") ||
    id.includes("openai")
  );
}

function bucketMatchesFamily(
  groupDisplayName: string,
  bucketId: string,
  family: QuotaFamily,
): boolean {
  const groupIsGemini = groupDisplayName.toLowerCase().includes("gemini");
  switch (family) {
    case "gemini":
      return bucketLooksGemini(bucketId) || groupIsGemini;
    case "claudeGpt":
      return (
        bucketLooksClaudeGpt(bucketId) ||
        (!groupIsGemini && !bucketLooksGemini(bucketId))
      );
    default: {
      const _exhaustive: never = family;
      return _exhaustive;
    }
  }
}

function bestBucket(
  account: AntigravityAccountPublic,
  window: string,
  family: QuotaFamily,
): AntigravityQuotaBucket | null {
  const groups = account.quota?.groups ?? [];
  let best: AntigravityQuotaBucket | null = null;
  for (const group of groups) {
    for (const bucket of group.buckets) {
      if (normalizeQuotaWindow(bucket.window, bucket.bucketId) !== normalizeQuotaWindow(window)) {
        continue;
      }
      if (!bucketMatchesFamily(group.displayName, bucket.bucketId, family)) continue;
      if (best == null || bucket.remainingFraction > best.remainingFraction) {
        best = bucket;
      }
    }
  }
  return best;
}

function percentFromGroups(
  account: AntigravityAccountPublic,
  window: string,
  family: QuotaFamily,
): number | null {
  const bucket = bestBucket(account, window, family);
  if (bucket == null) return null;
  return Math.round(bucket.remainingFraction * 100);
}

function windowResetTime(
  account: AntigravityAccountPublic,
  window: string,
  family: QuotaFamily,
): string | null {
  return bestBucket(account, window, family)?.resetTime || null;
}

export function accountQuotaSummary(account: AntigravityAccountPublic): {
  geminiFiveHour: number | null;
  geminiWeekly: number | null;
  claudeFiveHour: number | null;
  claudeWeekly: number | null;
  geminiFiveHourReset: string | null;
  geminiWeeklyReset: string | null;
  claudeFiveHourReset: string | null;
  claudeWeeklyReset: string | null;
  quotaUpdatedAt: number | null;
} {
  return {
    geminiFiveHour: account.quotaGemini5hPercent ?? percentFromGroups(account, "5h", "gemini"),
    geminiWeekly: account.quotaGeminiWeeklyPercent ?? percentFromGroups(account, "weekly", "gemini"),
    claudeFiveHour: account.quotaClaude5hPercent ?? percentFromGroups(account, "5h", "claudeGpt"),
    claudeWeekly: account.quotaClaudeWeeklyPercent ?? percentFromGroups(account, "weekly", "claudeGpt"),
    geminiFiveHourReset: windowResetTime(account, "5h", "gemini"),
    geminiWeeklyReset: windowResetTime(account, "weekly", "gemini"),
    claudeFiveHourReset: windowResetTime(account, "5h", "claudeGpt"),
    claudeWeeklyReset: windowResetTime(account, "weekly", "claudeGpt"),
    quotaUpdatedAt: account.quotaUpdatedAt ?? account.quota?.lastUpdated ?? null,
  };
}
