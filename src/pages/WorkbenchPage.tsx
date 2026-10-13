import { useEffect, useMemo, useState } from "react";
import { Alert, Button, Card, Popover, Segmented, Tooltip, Typography } from "antd";
import ArrowRightOutlined from "@ant-design/icons/es/icons/ArrowRightOutlined";
import ApiOutlined from "@ant-design/icons/es/icons/ApiOutlined";
import CloudServerOutlined from "@ant-design/icons/es/icons/CloudServerOutlined";
import DownOutlined from "@ant-design/icons/es/icons/DownOutlined";
import RobotOutlined from "@ant-design/icons/es/icons/RobotOutlined";
import InfoCircleOutlined from "@ant-design/icons/es/icons/InfoCircleOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { UsageCalendar } from "@/components/UsageCalendar";
import { MetricTrendBars, type TrendMetric } from "@/components/usage/MetricTrendBars";
import { UsageSourceFilterSelect } from "@/components/UsageSourceFilterSelect";
import { isUsageSourceFilter, usageSourceLabelKey } from "@/components/UsageSourceIcons";
import { LABEL_KEYS } from "@/components/AgentTargetSwitcher";
import { Metric } from "@/components/ui";
import {
  managedAppsRuntimeStatusOptions,
  providerListOptions,
  proxyStatusOptions,
  usageDashboardOptions,
  usageTrendOptions,
} from "@/lib/appQueries";
import { useNavigatePage } from "@/lib/navigation";
import type { PageKey } from "@/lib/pageRegistry";
import { errMsg } from "@/lib/errMsg";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { ProviderTarget, UsageBreakdown, UsageTrendPoint } from "@/types/backend";
import { formatCompactNumber } from "@/utils/formatCompact";
import { usagePeriodHourKeys } from "@/utils/usagePeriod";
import {
  getAgentConfigDrift,
  getSmartGatewayStatus,
  listGatewayUpstreamHealth,
  listSmartGatewayBindings,
} from "@/services/providers";
import { getAntigravityGatewayStatus } from "@/services/antigravity";
import { getKiroGatewayStatus } from "@/services/kiro";
import { listProxyRequestLogs } from "@/services/usage";
import { filterUiAgents } from "@/lib/agentVisibility";

const { Text } = Typography;

type OverviewPeriod = "today" | "24h" | 7 | 30;

const OVERVIEW_PERIODS: OverviewPeriod[] = ["today", "24h", 7, 30];

/**
 * 概览：状态条（异常明细在浮层）→ 所选时间范围的用量统计与趋势（弹性高度）
 * → Agent 活动 | 模型与供应商排行 → 过去一年日历。
 * 页面撑满外壳可用高度，大窗口下不出现纵向滚动；窗口过小时按最小高度滚动。
 * 连接关系在 Agent 页；请求明细在用量统计。
 */
export default function WorkbenchPage() {
  const { t } = useTranslation();
  const navigate = useNavigatePage();
  const [period, setPeriod] = useState<OverviewPeriod>("24h");
  const [trendMetric, setTrendMetric] = useState<TrendMetric>("tokens");

  const heatmapSource = usePagePreferencesStore((state) => state.heatmapSource);
  const setHeatmapSource = usePagePreferencesStore((state) => state.setHeatmapSource);

  const visibleAgents = usePagePreferencesStore((state) => state.visibleAgents);
  const visibleTargets = useMemo(() => filterUiAgents(visibleAgents), [visibleAgents]);

  const runtimeQuery = useQuery(managedAppsRuntimeStatusOptions);
  const codeProxyQuery = useQuery(proxyStatusOptions("claude_code"));
  const codexProxyQuery = useQuery(proxyStatusOptions("codex"));
  const opencodeProxyQuery = useQuery(proxyStatusOptions("opencode"));
  const piProxyQuery = useQuery(proxyStatusOptions("pi"));
  const desktopProxyQuery = useQuery(proxyStatusOptions("claude_desktop"));

  const proxyQueryMap: Partial<Record<ProviderTarget, typeof codeProxyQuery>> = {
    claude_code: codeProxyQuery,
    claude_desktop: desktopProxyQuery,
    codex: codexProxyQuery,
    opencode: opencodeProxyQuery,
    pi: piProxyQuery,
  };

  const providerQueries = [
    useQuery(providerListOptions("claude_code")),
    useQuery(providerListOptions("codex")),
    useQuery(providerListOptions("opencode")),
  ];

  const gatewayQuery = useQuery({
    queryKey: ["smart-gateway-status"],
    queryFn: getSmartGatewayStatus,
  });
  const bindingsQuery = useQuery({
    queryKey: ["smart-gateway-bindings"],
    queryFn: listSmartGatewayBindings,
  });
  const healthQuery = useQuery({
    queryKey: ["gateway-upstream-health"],
    queryFn: listGatewayUpstreamHealth,
  });
  const antigravityQuery = useQuery({
    queryKey: ["antigravity-gateway"],
    queryFn: getAntigravityGatewayStatus,
  });
  const kiroQuery = useQuery({
    queryKey: ["kiro-gateway"],
    queryFn: getKiroGatewayStatus,
  });
  const codeDriftQuery = useQuery({
    queryKey: ["agent-config-drift", "claude_code"],
    queryFn: () => getAgentConfigDrift("claude_code"),
    enabled: visibleTargets.includes("claude_code"),
  });
  const codexDriftQuery = useQuery({
    queryKey: ["agent-config-drift", "codex"],
    queryFn: () => getAgentConfigDrift("codex"),
    enabled: visibleTargets.includes("codex"),
  });

  const bindings = useMemo(() => bindingsQuery.data ?? [], [bindingsQuery.data]);
  const dashboardQuery = useQuery(usageDashboardOptions(period, heatmapSource));
  const activityQuery = useQuery({
    queryKey: ["workbench-activity", period, heatmapSource],
    queryFn: () =>
      listProxyRequestLogs({
        ...(period === "24h"
          ? { hours: 24 }
          : period === "today"
            ? { today: true }
            : { days: period }),
        page: 0,
        pageSize: 80,
        targetApp: heatmapSource === "all" ? undefined : heatmapSource,
      }),
  });
  const yearTrendQuery = useQuery(usageTrendOptions(365, heatmapSource));

  const providerCount = providerQueries.reduce((sum, query) => sum + (query.data?.length ?? 0), 0);
  const providersLoaded = providerQueries.every((query) => query.data !== undefined);
  const reverseAccountCount =
    (antigravityQuery.data?.accountCount ?? 0) + (kiroQuery.data?.accountCount ?? 0);
  const appStatus = runtimeQuery.data;

  const isAgentProcessRunning = (target: ProviderTarget): boolean => {
    if (!appStatus) return false;
    if (target === "claude_code") return Boolean(appStatus.claudeCode);
    if (target === "codex") return Boolean(appStatus.codex);
    if (target === "opencode") return Boolean(appStatus.opencode);
    if (target === "claude_desktop") return Boolean(appStatus.claudeDesktop);
    return false;
  };

  const agentRunningCount = appStatus
    ? visibleTargets.filter(isAgentProcessRunning).length
    : null;

  const summary = dashboardQuery.data?.summary;
  const requestCount = summary?.requestCount ?? 0;
  const totalTokens = tokenSum(summary);
  const successRate =
    requestCount > 0
      ? Number((((summary?.successfulRequestCount ?? 0) / requestCount) * 100).toFixed(1))
      : null;

  const trend = dashboardQuery.data?.trend ?? [];
  const deltas = halfWindowDeltas(period, trend);
  const deltaText = (value: number | null, tone: "neutral" | "positiveWhenDown" = "neutral") => {
    if (value == null) return undefined;
    const positive = tone === "positiveWhenDown" && value < 0;
    return (
      <span style={positive ? { color: "var(--color-success, #52c41a)" } : undefined}>
        {`${value >= 0 ? "↑" : "↓"}${Math.abs(value)}% ${t("workbench.vsPrevHalf", { defaultValue: "较前半段" })}`}
      </span>
    );
  };

  const activityRows = activityQuery.data?.data ?? [];
  const timedRows = activityRows.filter((row) => row.durationMs > 0);
  const avgDurationMs =
    timedRows.length > 0
      ? timedRows.reduce((sum, row) => sum + row.durationMs, 0) / timedRows.length
      : null;

  type ActivityGroup = {
    targetApp: string | null;
    tokens: number;
    requests: number;
    usageAvailable: boolean;
    lastAt: number;
  };
  const activityGroups = (() => {
    const groups = new Map<string, ActivityGroup>();
    for (const row of activityRows) {
      if (row.usageCounted === false) continue;
      const key = row.targetApp ?? "__unknown__";
      const tokens =
        row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens + row.outputTokens;
      const existing = groups.get(key);
      if (existing) {
        existing.tokens += tokens;
        existing.requests += 1;
        existing.usageAvailable = existing.usageAvailable || row.usageAvailable;
        existing.lastAt = Math.max(existing.lastAt, row.createdAt);
      } else {
        groups.set(key, {
          targetApp: row.targetApp,
          tokens,
          requests: 1,
          usageAvailable: row.usageAvailable,
          lastAt: row.createdAt,
        });
      }
    }
    return groups;
  })();

  const relativeTime = (ms: number) => {
    const minutes = Math.max(0, Math.floor((Date.now() - ms) / 60000));
    if (minutes < 1) return t("workbench.activityJustNow", { defaultValue: "刚刚" });
    if (minutes < 60) {
      return t("workbench.activityMinutesAgo", { minutes, defaultValue: "{{minutes}} 分钟前" });
    }
    const hours = Math.floor(minutes / 60);
    if (hours < 24) {
      return t("workbench.activityHoursAgo", { hours, defaultValue: "{{hours}} 小时前" });
    }
    return t("workbench.activityDaysAgo", { days: Math.floor(hours / 24), defaultValue: "{{days}} 天前" });
  };
  const activityAgentLabel = (targetApp: string | null) => {
    if (targetApp && targetApp in LABEL_KEYS) return t(LABEL_KEYS[targetApp as ProviderTarget]);
    if (targetApp && isUsageSourceFilter(targetApp)) return t(usageSourceLabelKey(targetApp));
    return targetApp ?? "—";
  };

  const coolingCount = (healthQuery.data ?? []).filter(
    (item) => item.status === "cooling" || item.status === "rate_limited",
  ).length;
  const failingCount = (healthQuery.data ?? []).filter(
    (item) => item.status !== "ok" && item.status !== "unknown" && item.status !== "cooling" && item.status !== "rate_limited",
  ).length;
  const driftedAgents = [
    visibleTargets.includes("claude_code") && codeDriftQuery.data?.status === "drifted" ? "claude_code" : null,
    visibleTargets.includes("codex") && codexDriftQuery.data?.status === "drifted" ? "codex" : null,
  ].filter((target): target is ProviderTarget => target != null);

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
  if (coolingCount > 0) {
    attentionItems.push({
      key: "cooling",
      text: t("workbench.attentionCooling", {
        count: coolingCount,
        defaultValue: "{{count}} 个供应商冷却中",
      }),
      page: "providers",
      action: t("workbench.viewProvidersLink", { defaultValue: "查看供应商" }),
    });
  }
  if (failingCount > 0) {
    attentionItems.push({
      key: "failing",
      text: t("workbench.attentionUpstreamFailing", {
        count: failingCount,
        defaultValue: "{{count}} 个供应商异常",
      }),
      page: "providers",
      action: t("workbench.viewProvidersLink", { defaultValue: "查看供应商" }),
    });
  }
  if (driftedAgents.length > 0) {
    attentionItems.push({
      key: "drift",
      text: t("workbench.attentionDrift", {
        agents: driftedAgents.map((target) => t(LABEL_KEYS[target])).join("、"),
        defaultValue: "{{agents}} 配置已被外部修改",
      }),
      page: "agentHub",
      action: t("workbench.viewAgentsLink", { defaultValue: "查看 Agent" }),
    });
  }
  visibleTargets.forEach((target) => {
    const binding = bindings.find((item) => item.targetApp === target);
    if (binding?.mode === "gateway") return;
    const proxyQuery = proxyQueryMap[target];
    if (proxyQuery?.data?.phase === "error") {
      attentionItems.push({
        key: `proxy-${target}`,
        text: t("workbench.attentionProxyError", {
          agent: t(LABEL_KEYS[target] ?? `workspace.${target}`),
          defaultValue: "{{agent}} 代理异常",
        }),
        page: "localProxy",
        action: t("workbench.viewProxyLink", { defaultValue: "查看代理" }),
      });
    }
  });
  const hasVisibleGatewayBinding = bindings.some(
    (binding) => binding.mode === "gateway" && visibleTargets.includes(binding.targetApp),
  );
  if (
    gatewayQuery.data &&
    (gatewayQuery.data.bindingCount > 0 || hasVisibleGatewayBinding) &&
    (gatewayQuery.data.phase === "error" || !gatewayQuery.data.running)
  ) {
    attentionItems.push({
      key: "smart-gateway",
      text: t("workbench.attentionGatewayError", { defaultValue: "已绑定应用的智能网关未运行" }),
      page: "providers",
      action: t("workbench.viewProvidersLink", { defaultValue: "查看供应商" }),
    });
  }
  if (providersLoaded && providerCount === 0) {
    attentionItems.push({
      key: "no-providers",
      text: t("workbench.attentionNoProviders", { defaultValue: "尚未配置供应商" }),
      page: "providers",
      action: t("workbench.viewProvidersLink", { defaultValue: "查看供应商" }),
    });
  }
  const healthy = attentionItems.length === 0;
  const heroEmpty = dashboardQuery.data != null && requestCount === 0;
  // 窗口高度较矮时切到紧凑档：日历格更小（其余间距由 CSS 媒体查询处理）
  const compactHeight = useMediaQuery("(max-height: 800px)");

  const attentionContent = (
    <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 260 }}>
      {attentionItems.map((item) => (
        <div key={item.key} style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, minHeight: 22 }}>
          <span aria-hidden style={{ width: 6, height: 6, borderRadius: "50%", flex: "0 0 auto", backgroundColor: "var(--color-warning, #faad14)" }} />
          <span>{item.text}</span>
          <Button type="link" size="small" icon={<ArrowRightOutlined />} iconPosition="end" onClick={() => navigate(item.page)} style={{ marginLeft: "auto", fontSize: 12, padding: 0 }}>
            {item.action}
          </Button>
        </div>
      ))}
    </div>
  );

  return (
    <div className="dashboard-container">
      <div className="overview-status-strip" role="group" aria-label={t("workbench.stripAria", { defaultValue: "概览状态" })}>
        {healthy ? (
          <span className="overview-status-pill" data-tone="ok">
            <span className="overview-status-dot" aria-hidden />
            {t("workbench.stripHealthy", { defaultValue: "系统运行正常" })}
          </span>
        ) : (
          <Popover trigger={["click", "hover"]} placement="bottomLeft" content={attentionContent}>
            <Button type="text" className="overview-status-pill" data-tone="warn" aria-haspopup="dialog">
              <span className="overview-status-dot" aria-hidden />
              {`${t("workbench.stripAttention", { defaultValue: "需要关注" })} · ${attentionItems.length}`}
              <DownOutlined aria-hidden className="overview-status-caret" />
            </Button>
          </Popover>
        )}
        <div className="overview-status-stats">
          {/* 统计 chip：窄条时标签视觉隐藏、改显图标；title 给完整句子 */}
          <span
            className="overview-stat-chip"
            title={providersLoaded ? t("workbench.stripProviders", { count: providerCount, defaultValue: "{{count}} 个供应商" }) : undefined}
          >
            <CloudServerOutlined className="overview-stat-icon" aria-hidden />
            <span className="overview-stat-label">{t("workbench.stripChipProviders", { defaultValue: "供应商" })}</span>
            <b className="overview-stat-num">{providersLoaded ? providerCount : "…"}</b>
          </span>
          <span
            className="overview-stat-chip"
            title={t("workbench.stripReverseAccounts", { count: reverseAccountCount, defaultValue: "{{count}} 个反代账号" })}
          >
            <ApiOutlined className="overview-stat-icon" aria-hidden />
            <span className="overview-stat-label">{t("workbench.stripChipReverse", { defaultValue: "反代账号" })}</span>
            <b className="overview-stat-num">{reverseAccountCount}</b>
          </span>
          <span
            className="overview-stat-chip"
            title={
              agentRunningCount != null
                ? t("workbench.stripAgentsActive", { count: agentRunningCount, defaultValue: "{{count}} 个 Agent 活跃" })
                : undefined
            }
          >
            <RobotOutlined className="overview-stat-icon" aria-hidden />
            <span className="overview-stat-label">{t("workbench.stripChipAgents", { defaultValue: "Agent 活跃" })}</span>
            <b className="overview-stat-num">{agentRunningCount ?? "…"}</b>
          </span>
        </div>
        <Button
          type="text"
          size="small"
          className="overview-status-link"
          icon={<ArrowRightOutlined />}
          iconPosition="end"
          onClick={() => navigate("providers")}
        >
          <span className="overview-status-link-text">{t("workbench.stripViewStatus", { defaultValue: "查看状态" })}</span>
        </Button>
      </div>
      <Card size="small" className="page-surface overview-trend-card">
        <div className="overview-trend-head">
          <div className="overview-trend-filters">
            <Segmented<OverviewPeriod>
              size="small"
              value={period}
              onChange={setPeriod}
              aria-label={t("usage.timeRange", { defaultValue: "时间范围" })}
              options={OVERVIEW_PERIODS.map((value) => ({
                value,
                label: t(overviewPeriodKey(value), { defaultValue: overviewPeriodFallback(value) }),
              }))}
            />
            <UsageSourceFilterSelect value={heatmapSource} onChange={setHeatmapSource} t={t} />
          </div>
          <div className="overview-trend-actions">
            {dashboardQuery.error || heroEmpty ? null : (
              <Segmented<TrendMetric>
                size="small"
                value={trendMetric}
                onChange={setTrendMetric}
                aria-label={t("usage.metric", { defaultValue: "图表指标" })}
                options={[
                  { value: "requests", label: t("usage.requests", { defaultValue: "请求" }) },
                  { value: "tokens", label: "Tokens" },
                  { value: "cost", label: t("usage.estimatedCost", { defaultValue: "成本" }) },
                ]}
              />
            )}
            <Button
              type="link"
              size="small"
              className="overview-trend-link"
              icon={<ArrowRightOutlined />}
              iconPosition="end"
              onClick={() => navigate("usage")}
            >
              {t("dashboard.viewUsage", { defaultValue: "用量详情" })}
            </Button>
          </div>
        </div>
        {dashboardQuery.error ? (
          <Alert type="error" showIcon message={errMsg(dashboardQuery.error)} />
        ) : heroEmpty ? (
          <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 8, textAlign: "center" }}>
            <span style={{ fontSize: 14, fontWeight: 600 }}>
              {t("workbench.emptyUsageTitle", { defaultValue: "所选时间范围内暂无请求" })}
            </span>
            <Button type="link" size="small" onClick={() => navigate("providers")}>
              {t("workbench.emptyUsageAction", { defaultValue: "查看供应商" })}
            </Button>
          </div>
        ) : (
          <>
            <div className="overview-metric-grid">
              <Metric label={t("usage.requests", { defaultValue: "请求数" })} value={formatCompactNumber(requestCount)} supporting={deltaText(deltas.requests)} />
              <Metric label={t("usage.totalTokens", { defaultValue: "Tokens" })} value={formatCompactNumber(totalTokens)} supporting={deltaText(deltas.tokens)} />
              <Metric
                label={
                  <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                    {t("usage.estimatedCost", { defaultValue: "预估成本" })}
                    <Tooltip title={t("dashboard.costTooltip")}>
                      <InfoCircleOutlined style={{ fontSize: 10, color: "var(--color-text-tertiary)" }} />
                    </Tooltip>
                  </span>
                }
                value={`${currencyPrefix(summary?.estimatedCostCurrency)}${(summary?.estimatedCost ?? 0).toFixed(2)}`}
                supporting={deltaText(deltas.cost, "positiveWhenDown")}
              />
              <Metric
                label={t("usage.successRate", { defaultValue: "成功率" })}
                value={successRate != null ? <span style={successRate < 95 ? { color: "var(--color-warning, #faad14)" } : undefined}>{successRate}%</span> : "—"}
              />
              <Metric
                label={t("workbench.avgDuration", { defaultValue: "平均耗时" })}
                value={avgDurationMs == null ? "—" : formatDuration(avgDurationMs)}
                supporting={
                  avgDurationMs != null && (activityQuery.data?.total ?? 0) > activityRows.length
                    ? t("workbench.avgDurationSample", {
                        count: timedRows.length,
                        defaultValue: "最近 {{count}} 条",
                      })
                    : undefined
                }
              />
            </div>
            <MetricTrendBars data={trend} period={period} metric={trendMetric} fill />
          </>
        )}
      </Card>

      <div className="overview-mid-grid">
        <Card size="small" className="page-surface overview-panel-card">
          <span style={{ fontSize: 13, fontWeight: 600 }}>{t("workbench.agentActivity", { defaultValue: "Agent 活动" })}</span>
          <div className="overview-agent-list">
            {visibleTargets.map((target) => {
              const group = activityGroups.get(target);
              const running = isAgentProcessRunning(target);
              const name = activityAgentLabel(target);
              const statusText = running
                ? t("workbench.agentRunning", { defaultValue: "活跃" })
                : t("workbench.agentIdle", { defaultValue: "空闲" });
              const tokensText = group ? formatCompactNumber(group.tokens) : "0";
              const requestsText = String(group?.requests ?? 0);
              const lastText = group ? relativeTime(group.lastAt) : "—";
              // 窄卡隐藏的列通过整行 title 悬停可见
              const rowTitle = [
                name,
                statusText,
                `${t("usage.totalTokens", { defaultValue: "总 Token" })} ${tokensText}`,
                `${t("usage.requests", { defaultValue: "请求数" })} ${requestsText}`,
                lastText,
              ].join(" · ");
              return (
                <div key={target} className="overview-agent-row" data-running={running} title={rowTitle}>
                  <span className="overview-agent-dot" aria-hidden />
                  <span className="overview-agent-name">{name}</span>
                  <span className="overview-agent-status">{statusText}</span>
                  <span className="overview-agent-num overview-agent-tokens">{tokensText}</span>
                  <span className="overview-agent-num overview-agent-count">{requestsText}</span>
                  <span className="overview-agent-num overview-agent-last">{lastText}</span>
                </div>
              );            })}
          </div>
        </Card>
        <Card size="small" className="page-surface overview-panel-card">
          <span style={{ fontSize: 13, fontWeight: 600 }}>{t("workbench.rankings", { defaultValue: "排行" })}</span>
          <div className="overview-rank-grid">
            <RankingList
              title={t("usage.byModel", { defaultValue: "按模型" })}
              rows={dashboardQuery.data?.byModel ?? []}
            />
            <RankingList
              title={t("usage.byProvider", { defaultValue: "按供应商" })}
              rows={dashboardQuery.data?.byProvider ?? []}
            />
          </div>
        </Card>
      </div>

      <Card size="small" className="page-surface overview-calendar-card">
        <span style={{ fontSize: 13, fontWeight: 600 }}>{t("workbench.pastYear", { defaultValue: "过去一年" })}</span>
        {yearTrendQuery.error ? (
          <Alert type="error" showIcon message={errMsg(yearTrendQuery.error)} />
        ) : (
          <UsageCalendar data={yearTrendQuery.data?.trend ?? []} period={365} compact maxCellSize={compactHeight ? 11 : 16} />
        )}
      </Card>
    </div>
  );
}

/** 订阅 CSS 媒体查询结果（仅用于需要在 JS 里取值的紧凑档）。 */
function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const media = window.matchMedia(query);
    const update = () => setMatches(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [query]);
  return matches;
}

function RankingList({ title, rows }: { title: string; rows: UsageBreakdown[] }) {
  const top = [...rows]
    .sort((a, b) => b.requestCount - a.requestCount)
    .slice(0, 5);
  return (
    <div style={{ minWidth: 0 }}>
      <div style={{ fontSize: 12, color: "var(--color-text-tertiary)", marginBottom: 4 }}>{title}</div>
      {top.length === 0 ? (
        <div style={{ fontSize: 12, color: "var(--color-text-tertiary)" }}>—</div>
      ) : (
        top.map((row) => (
          <div key={row.key} style={{ display: "flex", gap: 8, fontSize: 12, lineHeight: "22px" }}>
            <Text ellipsis style={{ flex: 1, fontSize: 12 }}>{row.key || "—"}</Text>
            <span>{formatCompactNumber(row.requestCount)}</span>
            <span style={{ color: "var(--color-text-tertiary)" }}>
              {formatCompactNumber(row.inputTokens + row.outputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens)}
            </span>
          </div>
        ))
      )}
    </div>
  );
}

function tokenSum(summary: { inputTokens: number; cacheReadInputTokens: number; cacheCreationInputTokens: number; outputTokens: number } | undefined): number {
  if (!summary) return 0;
  return summary.inputTokens + summary.cacheReadInputTokens + summary.cacheCreationInputTokens + summary.outputTokens;
}

function halfWindowDeltas(period: OverviewPeriod, trend: UsageTrendPoint[]) {
  const buckets = period === "24h" || period === "today"
    ? usagePeriodHourKeys(period).map((key) => trend.find((row) => row.date === key))
    : [...trend].sort((a, b) => a.date.localeCompare(b.date));
  const mid = Math.floor(buckets.length / 2);
  const sum = (rows: Array<UsageTrendPoint | undefined>) =>
    rows.reduce(
      (acc, row) => {
        if (!row) return acc;
        return {
          requests: acc.requests + row.requestCount,
          tokens: acc.tokens + row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens + row.outputTokens,
          cost: acc.cost + row.estimatedCost,
        };
      },
      { requests: 0, tokens: 0, cost: 0 },
    );
  const previous = sum(buckets.slice(0, mid));
  const current = sum(buckets.slice(mid));
  const pct = (next: number, prev: number) => (prev > 0 ? Math.round(((next - prev) / prev) * 1000) / 10 : null);
  return {
    requests: pct(current.requests, previous.requests),
    tokens: pct(current.tokens, previous.tokens),
    cost: pct(current.cost, previous.cost),
  };
}

function overviewPeriodKey(period: OverviewPeriod): string {
  switch (period) {
    case "today":
      return "usage.today";
    case "24h":
      return "usage.last24Hours";
    case 7:
      return "usage.last7Days";
    case 30:
      return "usage.last30Days";
    default: {
      const _exhaustive: never = period;
      return _exhaustive;
    }
  }
}

function overviewPeriodFallback(period: OverviewPeriod): string {
  switch (period) {
    case "today":
      return "今天";
    case "24h":
      return "24 小时";
    case 7:
      return "7 天";
    case 30:
      return "30 天";
    default: {
      const _exhaustive: never = period;
      return _exhaustive;
    }
  }
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(1)} s`;
}

function currencyPrefix(currency?: string | null) {
  const normalized = (currency ?? "USD").trim().toUpperCase();
  if (normalized === "CNY" || normalized === "RMB") return "¥";
  if (normalized === "EUR") return "€";
  if (normalized === "GBP") return "£";
  if (normalized === "USD" || normalized === "") return "$";
  return `${normalized} `;
}
