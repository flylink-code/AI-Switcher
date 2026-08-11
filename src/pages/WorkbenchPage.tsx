import { Alert, Button, Card, Tooltip, Typography } from "antd";
import ArrowRightOutlined from "@ant-design/icons/es/icons/ArrowRightOutlined";
import CheckCircleFilled from "@ant-design/icons/es/icons/CheckCircleFilled";
import InfoCircleOutlined from "@ant-design/icons/es/icons/InfoCircleOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { UsageCalendar, UsageTrendBars } from "@/components/UsageCalendar";
import { UsageSourceFilterSelect } from "@/components/UsageSourceFilterSelect";
import { usageSourceIcon, type UsageSourceFilter } from "@/components/UsageSourceIcons";
import { LABEL_KEYS } from "@/components/AgentTargetSwitcher";
import { Metric } from "@/components/ui";
import {
  managedAppsRuntimeStatusOptions,
  providerListOptions,
  proxyStatusOptions,
  usageDashboardOptions,
  usageLogsOptions,
  usageTrendOptions,
} from "@/lib/appQueries";
import { useNavigatePage } from "@/lib/navigation";
import type { PageKey } from "@/lib/pageRegistry";
import { errMsg } from "@/lib/useProviderActions";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { ProviderTarget } from "@/types/backend";
import { formatCompactNumber } from "@/utils/formatCompact";
import { usagePeriodHourKeys } from "@/utils/usagePeriod";

const { Text } = Typography;

/**
 * Overview — Plan B: minimal Usage Intelligence.
 * Structure: Status Strip (<=64px) → Today Usage Hero (KPI + 24h trend) →
 * Bottom Surface (Needs Attention | Recent Activity).
 * No Agent→Provider details, no provider switch, no proxy start/stop here;
 * those live on the Providers / Proxy pages. All data comes from existing
 * query options — no new queryFn, no polling added on this page.
 */
export default function WorkbenchPage() {
  const { t } = useTranslation();
  const navigate = useNavigatePage();

  const heatmapSource = usePagePreferencesStore((state) => state.heatmapSource);
  const setHeatmapSource = usePagePreferencesStore((state) => state.setHeatmapSource);

  // Existing queries only. proxyStatusOptions / managedAppsRuntimeStatusOptions
  // were already mounted here by the former runtime rail; providerListOptions
  // reuses the providers store's query keys (staleTime 30s, no interval).
  const runtimeQuery = useQuery(managedAppsRuntimeStatusOptions);
  const proxyQueries = [
    useQuery(proxyStatusOptions("claude_code")),
    useQuery(proxyStatusOptions("claude_desktop")),
    useQuery(proxyStatusOptions("codex")),
    useQuery(proxyStatusOptions("opencode")),
  ];
  const providerQueries = [
    useQuery(providerListOptions("claude_code")),
    useQuery(providerListOptions("claude_desktop")),
    useQuery(providerListOptions("codex")),
    useQuery(providerListOptions("opencode")),
  ];

  const dashboardQuery = useQuery(usageDashboardOptions("24h", heatmapSource));
  const trendQuery = useQuery(usageTrendOptions("24h", heatmapSource));
  const activityQuery = useQuery(usageLogsOptions("24h", 0, heatmapSource));
  // Yearly heatmap — same query option the old overview used (no new API/polling).
  const yearTrendQuery = useQuery(usageTrendOptions(365, heatmapSource));

  // ----- Aggregate status strip -----
  const proxyTargets: ProviderTarget[] = ["claude_code", "claude_desktop", "codex", "opencode"];
  const proxyRunningCount = proxyQueries.filter((q) => q.data?.running).length;
  const providerCount = providerQueries.reduce((sum, q) => sum + (q.data?.length ?? 0), 0);
  const providersLoaded = providerQueries.every((q) => q.data !== undefined);
  const appStatus = runtimeQuery.data;
  const agentRunningCount = appStatus
    ? [appStatus.claudeCode, appStatus.claudeDesktop, appStatus.codex, appStatus.opencode].filter(Boolean)
        .length
    : null;

  // ----- Today (24h) usage hero -----
  const summary = dashboardQuery.data?.summary;
  const requestCount = summary?.requestCount ?? 0;
  const totalTokens =
    (summary?.inputTokens ?? 0) +
    (summary?.cacheReadInputTokens ?? 0) +
    (summary?.cacheCreationInputTokens ?? 0) +
    (summary?.outputTokens ?? 0);
  const successRate =
    requestCount > 0
      ? Number((((summary?.successfulRequestCount ?? 0) / requestCount) * 100).toFixed(1))
      : null;

  // Deltas: last 12h vs previous 12h, computed from the existing 24h hourly
  // trend. Hidden whenever the previous window has no data (never fabricated).
  const hourKeys = usagePeriodHourKeys("24h");
  const trendByHour = new Map((trendQuery.data?.trend ?? []).map((row) => [row.date, row]));
  const sumHours = (keys: string[]) =>
    keys.reduce(
      (acc, key) => {
        const row = trendByHour.get(key);
        if (!row) return acc;
        return {
          requests: acc.requests + row.requestCount,
          tokens:
            acc.tokens +
            row.inputTokens +
            row.cacheReadInputTokens +
            row.cacheCreationInputTokens +
            row.outputTokens,
          cost: acc.cost + row.estimatedCost,
        };
      },
      { requests: 0, tokens: 0, cost: 0 },
    );
  const prevWindow = sumHours(hourKeys.slice(0, 12));
  const lastWindow = sumHours(hourKeys.slice(12));
  const pctDelta = (current: number, previous: number) =>
    trendQuery.data && previous > 0
      ? Math.round(((current - previous) / previous) * 1000) / 10
      : null;
  const deltas = {
    requests: pctDelta(lastWindow.requests, prevWindow.requests),
    tokens: pctDelta(lastWindow.tokens, prevWindow.tokens),
    cost: pctDelta(lastWindow.cost, prevWindow.cost),
  };
  const deltaText = (value: number | null) =>
    value == null
      ? undefined
      : `${value >= 0 ? "↑" : "↓"}${Math.abs(value)}% ${t("workbench.vsPrev12h", { defaultValue: "vs 前 12 小时" })}`;

  const heroEmpty = dashboardQuery.data != null && requestCount === 0;

  // ----- Needs attention (existing data only) -----
  type AttentionItem = { key: string; text: string; page: PageKey; action: string };
  const attentionItems: AttentionItem[] = [];
  if (successRate != null && successRate < 95) {
    attentionItems.push({
      key: "success-rate",
      text: t("workbench.attentionSuccessRate", {
        rate: successRate,
        defaultValue: "成功率下降至 {{rate}}%",
      }),
      page: "usage",
      action: t("workbench.viewUsageLink", { defaultValue: "查看用量" }),
    });
  }
  proxyTargets.forEach((target, index) => {
    if (proxyQueries[index].data?.phase === "error") {
      attentionItems.push({
        key: `proxy-${target}`,
        text: t("workbench.attentionProxyError", {
          agent: t(LABEL_KEYS[target]),
          defaultValue: "{{agent}} 代理异常",
        }),
        page: "proxy",
        action: t("workbench.viewProxyLink", { defaultValue: "查看代理" }),
      });
    }
  });
  if (providersLoaded && providerCount === 0) {
    attentionItems.push({
      key: "no-providers",
      text: t("workbench.attentionNoProviders", { defaultValue: "尚未配置供应商" }),
      page: "providers",
      action: t("workbench.viewProvidersLink", { defaultValue: "查看供应商" }),
    });
  }
  const healthy = attentionItems.length === 0;

  // ----- Recent activity -----
  const activityRows = (activityQuery.data?.data ?? []).slice(0, 6);
  const formatTime = (ms: number) => {
    const d = new Date(ms);
    return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  };
  const activityAgentLabel = (targetApp: string | null) => {
    if (targetApp && targetApp in LABEL_KEYS) return t(LABEL_KEYS[targetApp as ProviderTarget]);
    return targetApp ?? "—";
  };
  const activityTokens = (row: (typeof activityRows)[number]) => {
    if (!row.usageAvailable) return "—";
    return formatCompactNumber(
      row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens + row.outputTokens,
    );
  };

  return (
    <div
      className="dashboard-container"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "14px",
        maxWidth: 1360,
        margin: "0 auto",
        width: "100%",
        boxSizing: "border-box",
      }}
    >
      {/* 1. Minimal Status Strip (<=64px): aggregate status + counts, no operations */}
      <div
        style={{
          minHeight: 48,
          maxHeight: 64,
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "0 4px",
          fontSize: 12.5,
          color: "var(--color-text-secondary)",
        }}
      >
        <span
          aria-hidden
          style={{
            width: 8,
            height: 8,
            borderRadius: "50%",
            flex: "0 0 auto",
            backgroundColor: healthy
              ? "var(--color-success, #52c41a)"
              : "var(--color-warning, #faad14)",
          }}
        />
        <span style={{ fontWeight: 600, color: "var(--color-text-primary)" }}>
          {healthy
            ? t("workbench.stripHealthy", { defaultValue: "系统运行正常" })
            : t("workbench.stripAttention", { defaultValue: "需要关注" })}
        </span>
        <span aria-hidden style={{ color: "var(--color-text-tertiary)" }}>·</span>
        <span>
          {providersLoaded
            ? t("workbench.stripProviders", { count: providerCount, defaultValue: "{{count}} 个供应商" })
            : "…"}
        </span>
        <span aria-hidden style={{ color: "var(--color-text-tertiary)" }}>·</span>
        <span>
          {t("workbench.stripProxies", { count: proxyRunningCount, defaultValue: "{{count}} 个代理运行" })}
        </span>
        <span aria-hidden style={{ color: "var(--color-text-tertiary)" }}>·</span>
        <span>
          {agentRunningCount != null
            ? t("workbench.stripAgents", {
                running: agentRunningCount,
                total: 4,
                defaultValue: "{{running}}/{{total}} 个 Agent",
              })
            : "…"}
        </span>
        <Button
          type="link"
          size="small"
          icon={<ArrowRightOutlined />}
          iconPosition="end"
          onClick={() => navigate("proxy")}
          style={{ marginLeft: "auto", fontSize: 12, padding: 0 }}
        >
          {t("workbench.stripViewStatus", { defaultValue: "查看状态" })}
        </Button>
      </div>

      {/* 2. Today Usage hero: 4 KPIs in one surface + 24h trend chart */}
      <Card
        size="small"
        className="page-surface workbench-chart-card"
        style={{ marginBottom: 0 }}
        styles={{ body: { padding: "12px 16px 14px" } }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            marginBottom: 10,
          }}
        >
          <span style={{ fontSize: 14, fontWeight: 600 }}>
            {t("dashboard.usageTitle", { defaultValue: "最近 24 小时" })}
          </span>
          <div style={{ display: "inline-flex", alignItems: "center", gap: 10 }}>
            <UsageSourceFilterSelect value={heatmapSource} onChange={setHeatmapSource} t={t} />
            <Button
              type="link"
              size="small"
              icon={<ArrowRightOutlined />}
              iconPosition="end"
              onClick={() => navigate("usage")}
              style={{ fontSize: 12, padding: 0 }}
            >
              {t("dashboard.viewUsage", { defaultValue: "用量详情" })}
            </Button>
          </div>
        </div>

        {dashboardQuery.error || trendQuery.error ? (
          <Alert type="error" showIcon message={errMsg(dashboardQuery.error ?? trendQuery.error)} />
        ) : heroEmpty ? (
          <div
            style={{
              padding: "36px 0 32px",
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              gap: 8,
              textAlign: "center",
            }}
          >
            <span style={{ fontSize: 14, fontWeight: 600 }}>
              {t("workbench.emptyUsageTitle", { defaultValue: "最近 24 小时暂无请求" })}
            </span>
            <span style={{ fontSize: 12, color: "var(--color-text-secondary)", maxWidth: 420 }}>
              {t("workbench.emptyUsageBody", {
                defaultValue:
                  "开始使用 Claude Code、Codex 或其他 Agent 后，这里会显示请求、Token 和成本趋势。",
              })}
            </span>
            <Button
              type="link"
              size="small"
              icon={<ArrowRightOutlined />}
              iconPosition="end"
              onClick={() => navigate("providers")}
              style={{ fontSize: 12, padding: 0 }}
            >
              {t("workbench.emptyUsageAction", { defaultValue: "查看供应商" })}
            </Button>
          </div>
        ) : (
          <>
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "repeat(auto-fit, minmax(140px, 1fr))",
                gap: 12,
              }}
            >
              <Metric
                label={t("usage.requests", { defaultValue: "请求数" })}
                value={formatCompactNumber(requestCount)}
                supporting={deltaText(deltas.requests)}
              />
              <Metric
                label={t("usage.totalTokens", { defaultValue: "Tokens" })}
                value={formatCompactNumber(totalTokens)}
                supporting={deltaText(deltas.tokens)}
              />
              <Metric
                label={
                  <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                    {t("usage.estimatedCost", { defaultValue: "预估成本" })}
                    <Tooltip title={t("dashboard.costTooltip")}>
                      <InfoCircleOutlined style={{ fontSize: 10, color: "var(--color-text-tertiary)" }} />
                    </Tooltip>
                  </span>
                }
                value={`${currencyPrefix(summary?.estimatedCostCurrency)}${(summary?.estimatedCost ?? 0).toFixed(3)}`}
                supporting={deltaText(deltas.cost)}
              />
              <Metric
                label={t("usage.successRate", { defaultValue: "成功率" })}
                value={successRate != null ? `${successRate}%` : "—"}
              />
            </div>
            <div className="workbench-hero-chart" style={{ marginTop: 10 }}>
              <UsageTrendBars data={trendQuery.data?.trend ?? []} period="24h" compact />
            </div>
          </>
        )}
      </Card>

      {/* 3. Bottom surface: Needs Attention | Recent Activity */}
      <Card
        size="small"
        className="page-surface"
        style={{ marginBottom: 0 }}
        styles={{ body: { padding: 0 } }}
      >
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr" }}>
          {/* Needs Attention */}
          <div style={{ padding: "12px 16px", display: "flex", flexDirection: "column", gap: 8, minWidth: 0 }}>
            <span style={{ fontSize: 13, fontWeight: 600 }}>
              {t("workbench.attentionTitle", { defaultValue: "需要关注" })}
            </span>
            {healthy ? (
              <div style={{ display: "flex", alignItems: "center", gap: 6, color: "var(--color-text-secondary)", fontSize: 12 }}>
                <CheckCircleFilled style={{ color: "var(--color-success, #52c41a)", fontSize: 12 }} />
                {t("workbench.attentionAllClear", { defaultValue: "暂无需要关注的项目" })}
              </div>
            ) : (
              attentionItems.map((item) => (
                <div
                  key={item.key}
                  style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, minHeight: 22 }}
                >
                  <span
                    aria-hidden
                    style={{
                      width: 6,
                      height: 6,
                      borderRadius: "50%",
                      flex: "0 0 auto",
                      backgroundColor: "var(--color-warning, #faad14)",
                    }}
                  />
                  <span style={{ color: "var(--color-text-primary)" }}>{item.text}</span>
                  <Button
                    type="link"
                    size="small"
                    icon={<ArrowRightOutlined />}
                    iconPosition="end"
                    onClick={() => navigate(item.page)}
                    style={{ marginLeft: "auto", fontSize: 12, padding: 0 }}
                  >
                    {item.action}
                  </Button>
                </div>
              ))
            )}
          </div>

          {/* Recent Activity */}
          <div
            style={{
              padding: "12px 16px",
              display: "flex",
              flexDirection: "column",
              gap: 6,
              minWidth: 0,
              borderLeft: "1px solid var(--color-border-subtle, rgba(0,0,0,0.06))",
            }}
          >
            <span style={{ fontSize: 13, fontWeight: 600, marginBottom: 2 }}>
              {t("workbench.activityTitle", { defaultValue: "最近活动" })}
            </span>
            {activityQuery.error ? (
              <Alert type="error" showIcon message={errMsg(activityQuery.error)} />
            ) : activityRows.length === 0 ? (
              <span style={{ fontSize: 12, color: "var(--color-text-tertiary)" }}>
                {t("workbench.activityEmpty", { defaultValue: "暂无活动记录" })}
              </span>
            ) : (
              activityRows.map((row) => (
                <div
                  key={row.id}
                  style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, minHeight: 20 }}
                >
                  <span
                    style={{
                      fontFamily: "monospace",
                      fontSize: 11,
                      color: "var(--color-text-tertiary)",
                      flex: "0 0 auto",
                    }}
                  >
                    {formatTime(row.createdAt)}
                  </span>
                  {row.targetApp && row.targetApp in LABEL_KEYS
                    ? usageSourceIcon(row.targetApp as UsageSourceFilter, { size: 13 })
                    : null}
                  <Text ellipsis style={{ fontSize: 12, flex: "1 1 auto", minWidth: 0 }}>
                    {activityAgentLabel(row.targetApp)}
                  </Text>
                  <span style={{ fontSize: 12, color: "var(--color-text-secondary)", flex: "0 0 auto" }}>
                    {activityTokens(row)}
                  </span>
                </div>
              ))
            )}
          </div>
        </div>
      </Card>

      {/* 4. Past Year — quiet long-term context, full width aligned with the cards above */}
      <section style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
        <span style={{ fontSize: "14px", fontWeight: 600 }}>
          {t("workbench.pastYear", { defaultValue: "过去一年" })}
        </span>
        <Card
          size="small"
          className="page-surface workbench-chart-card"
          style={{ marginBottom: 0 }}
          styles={{ body: { padding: "8px 12px" } }}
        >
          {yearTrendQuery.error ? (
            <Alert type="error" showIcon message={errMsg(yearTrendQuery.error)} />
          ) : (
            <div style={{ width: "100%", overflow: "hidden" }}>
              <UsageCalendar
                data={yearTrendQuery.data?.trend ?? []}
                period={365}
                compact
                maxCellSize={14}
              />
            </div>
          )}
        </Card>
      </section>
    </div>
  );
}

function currencyPrefix(currency?: string | null) {
  const normalized = (currency ?? "USD").trim().toUpperCase();
  if (normalized === "CNY" || normalized === "RMB") return "¥";
  if (normalized === "EUR") return "€";
  if (normalized === "GBP") return "£";
  if (normalized === "USD" || normalized === "") return "$";
  return `${normalized} `;
}
