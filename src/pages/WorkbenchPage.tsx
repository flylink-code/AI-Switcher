import { useMemo, useState } from "react";
import { Alert, Button, Card, Segmented, Tooltip, Typography } from "antd";
import ArrowRightOutlined from "@ant-design/icons/es/icons/ArrowRightOutlined";
import CheckCircleFilled from "@ant-design/icons/es/icons/CheckCircleFilled";
import InfoCircleOutlined from "@ant-design/icons/es/icons/InfoCircleOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { UsageCalendar } from "@/components/UsageCalendar";
import { MetricTrendBars, type TrendMetric } from "@/components/usage/MetricTrendBars";
import { UsageSourceFilterSelect } from "@/components/UsageSourceFilterSelect";
import { usageSourceIcon, isUsageSourceFilter, usageSourceLabelKey } from "@/components/UsageSourceIcons";
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
 * 概览：状态条 → 所选时间范围的用量统计与趋势 → Agent 活动与排行 → 需要关注。
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
  const clineProxyQuery = useQuery(proxyStatusOptions("cline"));

  const proxyQueryMap: Partial<Record<ProviderTarget, typeof codeProxyQuery>> = {
    claude_code: codeProxyQuery,
    codex: codexProxyQuery,
    opencode: opencodeProxyQuery,
    pi: piProxyQuery,
    cline: clineProxyQuery,
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
  const recentGroups = [...activityGroups.values()].sort((a, b) => b.lastAt - a.lastAt).slice(0, 6);

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
      <div
        style={{
          minHeight: 48,
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "0 4px",
          fontSize: 12.5,
          color: "var(--color-text-secondary)",
          flexWrap: "wrap",
        }}
      >
        <span
          aria-hidden
          style={{
            width: 8,
            height: 8,
            borderRadius: "50%",
            flex: "0 0 auto",
            backgroundColor: healthy ? "var(--color-success, #52c41a)" : "var(--color-warning, #faad14)",
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
          {t("workbench.stripReverseAccounts", {
            count: reverseAccountCount,
            defaultValue: "{{count}} 个反代账号",
          })}
        </span>
        <span aria-hidden style={{ color: "var(--color-text-tertiary)" }}>·</span>
        <span>
          {agentRunningCount != null
            ? t("workbench.stripAgentsActive", {
                count: agentRunningCount,
                defaultValue: "{{count}} 个 Agent 活跃",
              })
            : "…"}
        </span>
        <Button
          type="link"
          size="small"
          icon={<ArrowRightOutlined />}
          iconPosition="end"
          onClick={() => navigate("providers")}
          style={{ marginLeft: "auto", fontSize: 12, padding: 0 }}
        >
          {t("workbench.stripViewStatus", { defaultValue: "查看状态" })}
        </Button>
      </div>

      <Card size="small" className="page-surface workbench-chart-card" styles={{ body: { padding: "14px 20px 12px" } }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, marginBottom: 8, flexWrap: "wrap" }}>
          <Segmented<OverviewPeriod>
            size="small"
            value={period}
            onChange={setPeriod}
            options={OVERVIEW_PERIODS.map((value) => ({
              value,
              label: t(overviewPeriodKey(value), { defaultValue: overviewPeriodFallback(value) }),
            }))}
          />
          <div style={{ display: "inline-flex", alignItems: "center", gap: 10 }}>
            <UsageSourceFilterSelect value={heatmapSource} onChange={setHeatmapSource} t={t} />
            <Button type="link" size="small" icon={<ArrowRightOutlined />} iconPosition="end" onClick={() => navigate("usage")} style={{ fontSize: 12, padding: 0 }}>
              {t("dashboard.viewUsage", { defaultValue: "用量详情" })}
            </Button>
          </div>
        </div>

        {dashboardQuery.error ? (
          <Alert type="error" showIcon message={errMsg(dashboardQuery.error)} />
        ) : heroEmpty ? (
          <div style={{ padding: "36px 0 32px", display: "flex", flexDirection: "column", alignItems: "center", gap: 8, textAlign: "center" }}>
            <span style={{ fontSize: 14, fontWeight: 600 }}>
              {t("workbench.emptyUsageTitle", { defaultValue: "所选时间范围内暂无请求" })}
            </span>
            <Button type="link" size="small" onClick={() => navigate("providers")}>
              {t("workbench.emptyUsageAction", { defaultValue: "查看供应商" })}
            </Button>
          </div>
        ) : (
          <>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(140px, 1fr))", gap: 12 }}>
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
            <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 8 }}>
              <Segmented<TrendMetric>
                size="small"
                value={trendMetric}
                onChange={setTrendMetric}
                options={[
                  { value: "requests", label: t("usage.requests", { defaultValue: "请求" }) },
                  { value: "tokens", label: "Tokens" },
                  { value: "cost", label: t("usage.estimatedCost", { defaultValue: "成本" }) },
                ]}
              />
            </div>
            <MetricTrendBars data={trend} period={period} metric={trendMetric} />
          </>
        )}
      </Card>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.2fr) minmax(0, 1fr)", gap: 14 }}>
        <Card size="small" className="page-surface" styles={{ body: { padding: "14px 20px" } }}>
          <span style={{ fontSize: 13, fontWeight: 600 }}>{t("workbench.agentActivity", { defaultValue: "Agent 活动" })}</span>
          <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 10 }}>
            {visibleTargets.map((target) => {
              const group = activityGroups.get(target);
              const running = isAgentProcessRunning(target);
              return (
                <div key={target} style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12 }}>
                  <span aria-hidden style={{ width: 8, height: 8, borderRadius: "50%", background: running ? "var(--color-success, #52c41a)" : "var(--color-text-tertiary)" }} />
                  <Text ellipsis style={{ flex: "1 1 auto", fontSize: 12 }}>{activityAgentLabel(target)}</Text>
                  <span style={{ color: "var(--color-text-tertiary)" }}>
                    {running
                      ? t("workbench.agentRunning", { defaultValue: "活跃" })
                      : t("workbench.agentIdle", { defaultValue: "空闲" })}
                  </span>
                  <span style={{ minWidth: 72, textAlign: "right", color: "var(--color-text-secondary)" }}>
                    {group ? `${formatCompactNumber(group.tokens)}` : "0"}
                  </span>
                  <span style={{ minWidth: 48, textAlign: "right", color: "var(--color-text-tertiary)" }}>
                    {group?.requests ?? 0}
                  </span>
                  <span style={{ minWidth: 64, textAlign: "right", color: "var(--color-text-tertiary)" }}>
                    {group ? relativeTime(group.lastAt) : "—"}
                  </span>
                </div>
              );
            })}
          </div>
        </Card>
        <Card size="small" className="page-surface" styles={{ body: { padding: "14px 20px" } }}>
          <span style={{ fontSize: 13, fontWeight: 600 }}>{t("workbench.rankings", { defaultValue: "排行" })}</span>
          <RankingList
            title={t("usage.byModel", { defaultValue: "按模型" })}
            rows={dashboardQuery.data?.byModel ?? []}
          />
          <RankingList
            title={t("usage.byProvider", { defaultValue: "按供应商" })}
            rows={dashboardQuery.data?.byProvider ?? []}
          />
        </Card>
      </div>

      <Card size="small" className="page-surface" styles={{ body: { padding: 0 } }}>
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr" }}>
          <div style={{ padding: "14px 20px", display: "flex", flexDirection: "column", gap: 8, minWidth: 0 }}>
            <span style={{ fontSize: 13, fontWeight: 600 }}>{t("workbench.attentionTitle", { defaultValue: "需要关注" })}</span>
            {healthy ? (
              <div style={{ display: "flex", alignItems: "center", gap: 6, color: "var(--color-text-secondary)", fontSize: 12 }}>
                <CheckCircleFilled style={{ color: "var(--color-success, #52c41a)", fontSize: 12 }} />
                {t("workbench.attentionAllClear", { defaultValue: "当前无异常" })}
              </div>
            ) : (
              attentionItems.map((item) => (
                <div key={item.key} style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, minHeight: 22 }}>
                  <span aria-hidden style={{ width: 6, height: 6, borderRadius: "50%", backgroundColor: "var(--color-warning, #faad14)" }} />
                  <span>{item.text}</span>
                  <Button type="link" size="small" icon={<ArrowRightOutlined />} iconPosition="end" onClick={() => navigate(item.page)} style={{ marginLeft: "auto", fontSize: 12, padding: 0 }}>
                    {item.action}
                  </Button>
                </div>
              ))
            )}
          </div>
          <div style={{ padding: "14px 20px", display: "flex", flexDirection: "column", gap: 6, minWidth: 0, borderLeft: "1px solid var(--color-border-subtle, rgba(0,0,0,0.06))" }}>
            <span style={{ fontSize: 13, fontWeight: 600 }}>{t("workbench.activityTitle", { defaultValue: "最近活动" })}</span>
            {activityQuery.error ? (
              <Alert type="error" showIcon message={errMsg(activityQuery.error)} />
            ) : recentGroups.length === 0 ? (
              <span style={{ fontSize: 12, color: "var(--color-text-tertiary)" }}>{t("workbench.activityEmpty", { defaultValue: "暂无活动记录" })}</span>
            ) : (
              recentGroups.map((group) => (
                <div key={group.targetApp ?? "__unknown__"} style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12 }}>
                  {group.targetApp && isUsageSourceFilter(group.targetApp) ? usageSourceIcon(group.targetApp, { size: 13 }) : null}
                  <Text ellipsis style={{ fontSize: 12, flex: "1 1 auto" }}>{activityAgentLabel(group.targetApp)}</Text>
                  <span style={{ color: "var(--color-text-secondary)" }}>
                    {group.usageAvailable ? `${formatCompactNumber(group.tokens)} Token` : "—"}
                  </span>
                  <span style={{ color: "var(--color-text-tertiary)", minWidth: 52, textAlign: "right" }}>{relativeTime(group.lastAt)}</span>
                </div>
              ))
            )}
          </div>
        </div>
      </Card>

      <section style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <span style={{ fontSize: 14, fontWeight: 600 }}>{t("workbench.pastYear", { defaultValue: "过去一年" })}</span>
        <Card size="small" className="page-surface workbench-chart-card" styles={{ body: { padding: "10px 16px 12px" } }}>
          {yearTrendQuery.error ? (
            <Alert type="error" showIcon message={errMsg(yearTrendQuery.error)} />
          ) : (
            <UsageCalendar data={yearTrendQuery.data?.trend ?? []} period={365} compact maxCellSize={16} />
          )}
        </Card>
      </section>
    </div>
  );
}

function RankingList({ title, rows }: { title: string; rows: UsageBreakdown[] }) {
  const top = [...rows]
    .sort((a, b) => b.requestCount - a.requestCount)
    .slice(0, 5);
  return (
    <div style={{ marginTop: 10 }}>
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
