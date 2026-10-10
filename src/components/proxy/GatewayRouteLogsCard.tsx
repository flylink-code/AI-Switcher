import { useEffect, useMemo, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Drawer,
  Input,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  Tooltip,
  Typography,
} from "antd";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import InfoCircleOutlined from "@ant-design/icons/es/icons/InfoCircleOutlined";
import CheckCircleOutlined from "@ant-design/icons/es/icons/CheckCircleOutlined";
import CloseCircleOutlined from "@ant-design/icons/es/icons/CloseCircleOutlined";
import WarningOutlined from "@ant-design/icons/es/icons/WarningOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { refreshUsageQuery, useUsageLogRefresh } from "@/lib/useUsageLogRefresh";
import { useTranslation } from "react-i18next";
import { listGatewayRouteLogs } from "@/services/providers";
import { filterUiAgents } from "@/lib/agentVisibility";
import { LABEL_KEYS, PROVIDER_TARGET_OPTIONS } from "@/components/AgentTargetSwitcher";
import type {
  GatewayRouteLog,
  GatewayRouteLogStatusFilter,
  ProxyRequestAttempt,
  ProviderTarget,
} from "@/types/backend";
import { formatCompactNumber } from "@/utils/formatCompact";
import { formatTokenRate } from "@/utils/usageRate";
import { catalogModelView } from "@/utils/catalogModelLabel";

const { Text } = Typography;

const ROUTE_LOG_PAGE_SIZE = 20;

/**
 * 路由模式对应的标签色彩映射
 */
export function routeModeColor(mode?: string | null): string {
  switch (mode) {
    case "long_context":
      return "purple";
    case "think":
      return "blue";
    case "plan":
      return "cyan";
    case "edit":
      return "orange";
    case "background":
      return "geekblue";
    case "web_search":
      return "green";
    case "vision":
      return "magenta";
    case "image_gen":
      return "gold";
    case "explicit_model":
      return "processing";
    case "rule":
      return "lime";
    case "default":
      return "default";
    default:
      return "default";
  }
}

/**
 * 从旧版或未结构化的路由依据中分类出模式键
 */
export function classifyRouteReasonMode(reason: string): string | null {
  if (reason === "explicit_model") return "explicit_model";
  if (reason === "rule" || reason.startsWith("rule:")) return "rule";
  if (reason.includes("规划") || reason === "plan") return "plan";
  if (reason.includes("改内容") || reason === "edit") return "edit";
  if (reason.includes("后台") || reason === "background" || reason === "role_subagent") {
    return "background";
  }
  if (reason.includes("思考") || reason === "think") return "think";
  if (reason.includes("长上下文") || reason === "long_context") return "long_context";
  if (reason.includes("联网") || reason === "web_search") return "web_search";
  if (reason.includes("视觉") || reason === "vision") return "vision";
  if (reason.includes("图像") || reason === "image_gen") return "image_gen";
  if (reason.includes("默认") || reason === "auto" || reason === "profile_default") {
    return "default";
  }
  return null;
}

/**
 * 提取日志行的模式键，优先读取结构化 route_mode 字段，回退依据分类
 */
export function routeLogModeKey(row: GatewayRouteLog): string {
  const mode = row.routeMode?.trim();
  if (mode) return mode;
  return classifyRouteReasonMode(row.routeReason?.trim() ?? "") ?? "";
}

/**
 * 安全解析网关重试与故障转移尝试链 (attempts_json)
 * 进行严格的逐项字段类型与有限数值校验，防止畸形对象或 [null] 导致抽屉页面崩溃
 */
export function parseAttempts(raw?: string | null): ProxyRequestAttempt[] {
  if (!raw?.trim()) return [];
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];

    return parsed.filter((item): item is ProxyRequestAttempt => {
      // 必须是有效非空对象
      if (!item || typeof item !== "object") return false;
      const att = item as Partial<ProxyRequestAttempt>;

      // 验证 attemptIndex: 必须为大于等于 0 的有效有限数字
      if (
        typeof att.attemptIndex !== "number" ||
        !Number.isFinite(att.attemptIndex) ||
        att.attemptIndex < 0
      ) {
        return false;
      }

      // 验证 model: 必须存在且为字符串
      if (typeof att.model !== "string") {
        return false;
      }

      // 验证 durationMs: 必须为有限数值
      if (typeof att.durationMs !== "number" || !Number.isFinite(att.durationMs)) {
        return false;
      }

      // 验证 success: 必须为布尔值
      if (typeof att.success !== "boolean") {
        return false;
      }

      // 可选字段的防御性类型校验
      if (
        att.statusCode != null &&
        (typeof att.statusCode !== "number" || !Number.isFinite(att.statusCode))
      ) {
        return false;
      }
      if (att.upstreamId != null && typeof att.upstreamId !== "string") {
        return false;
      }
      if (att.providerName != null && typeof att.providerName !== "string") {
        return false;
      }
      if (att.errorCategory != null && typeof att.errorCategory !== "string") {
        return false;
      }
      if (att.diagnostic != null && typeof att.diagnostic !== "string") {
        return false;
      }

      return true;
    });
  } catch {
    return [];
  }
}

function EllipsisText({ value }: { value?: string | null }) {
  const text = value?.trim() || "—";
  return (
    <Text ellipsis={{ tooltip: text }} style={{ maxWidth: "100%", margin: 0 }}>
      {text}
    </Text>
  );
}

function ModelIdText({ value }: { value?: string | null }) {
  const raw = value?.trim() || "";
  if (!raw) {
    return (
      <Text ellipsis style={{ maxWidth: "100%", margin: 0 }}>
        —
      </Text>
    );
  }
  const short = catalogModelView({ publicId: raw }).short;
  return (
    <Text ellipsis={{ tooltip: raw }} style={{ maxWidth: "100%", margin: 0 }}>
      {short}
    </Text>
  );
}

export function GatewayRouteLogsCard() {
  const { t } = useTranslation();
  const [page, setPage] = useState(0);
  const [targetFilter, setTargetFilter] = useState<ProviderTarget | null>(null);
  const [statusFilter, setStatusFilter] = useState<GatewayRouteLogStatusFilter>("all");
  const [modeFilter, setModeFilter] = useState<string>("all");
  const [searchKeyword, setSearchKeyword] = useState<string>("");
  const [debouncedKeyword, setDebouncedKeyword] = useState<string>("");
  const [autoRefresh, setAutoRefresh] = useState(false);
  const [detailRecord, setDetailRecord] = useState<GatewayRouteLog | null>(null);

  // 搜索关键字 300ms 防抖，变更时重置回第一页
  useEffect(() => {
    const timer = setTimeout(() => {
      setDebouncedKeyword(searchKeyword);
      setPage(0);
    }, 300);
    return () => clearTimeout(timer);
  }, [searchKeyword]);

  const visibleTargets = useMemo(
    () => filterUiAgents(PROVIDER_TARGET_OPTIONS),
    [],
  );

  const hasActiveFilter =
    targetFilter !== null ||
    statusFilter !== "all" ||
    modeFilter !== "all" ||
    Boolean(debouncedKeyword.trim());

  // 全局后端分页与条件过滤查询
  const logsQuery = useQuery({
    queryKey: [
      "gateway-route-logs",
      targetFilter,
      statusFilter,
      modeFilter,
      debouncedKeyword.trim(),
      page,
    ],
    queryFn: () =>
      listGatewayRouteLogs(
        {
          target: targetFilter,
          status: statusFilter === "all" ? null : statusFilter,
          mode: modeFilter === "all" ? null : modeFilter,
          keyword: debouncedKeyword.trim() || null,
        },
        ROUTE_LOG_PAGE_SIZE,
        page * ROUTE_LOG_PAGE_SIZE,
      ),
    placeholderData: keepPreviousData,
  });

  // 统一数据刷新：复用 usage-log-recorded 事件与 10 秒兜底轮询，保持当前分页与筛选
  useUsageLogRefresh({
    enabled: autoRefresh,
    pollIntervalMs: 10_000,
    onRefresh: () => refreshUsageQuery(logsQuery),
  });

  // 抽屉详情数据：优先当前页最新同 ID 记录，离页后保持静态快照并明确提示
  const liveRecord = useMemo(() => {
    if (!detailRecord) return null;
    return logsQuery.data?.data.find((item) => item.id === detailRecord.id) ?? null;
  }, [detailRecord, logsQuery.data?.data]);

  const activeDetailRecord = liveRecord ?? detailRecord;
  const isOffPageSnapshot = Boolean(detailRecord && !liveRecord);

  const totalCount = logsQuery.data?.total ?? 0;

  const handleResetFilters = () => {
    setTargetFilter(null);
    setStatusFilter("all");
    setModeFilter("all");
    setSearchKeyword("");
    setDebouncedKeyword("");
    setPage(0);
  };

  /**
   * 状态徽标渲染：精准区分断流、客户端取消、限流、错误及成功
   */
  const renderStatusBadge = (row: GatewayRouteLog) => {
    const code = row.statusCode;
    const isMidstreamError = row.streamOutcome === "midstream_error";
    const isCancelled = row.streamOutcome === "cancelled";
    const hasErrorCat = Boolean(row.errorCategory);

    // 1. 中途断流状态（流传输异常终止）
    if (isMidstreamError) {
      return (
        <Tooltip title={t("gateway.midstreamErrorDesc", { defaultValue: "流式传输过程中断连接" })}>
          <Tag color="magenta" icon={<CloseCircleOutlined />}>
            {code ? `${code} ` : ""}
            {t("gateway.midstreamError", { defaultValue: "中途断流" })}
          </Tag>
        </Tooltip>
      );
    }

    // 2. 客户端取消状态（即便 HTTP 为 200 也不展示为绿色成功）
    if (isCancelled) {
      return (
        <Tooltip title={t("gateway.cancelledDesc", { defaultValue: "客户端主动断开连接或取消请求" })}>
          <Tag color="default" icon={<StopOutlined />}>
            {code ? `${code} ` : ""}
            {t("gateway.cancelled", { defaultValue: "已取消" })}
          </Tag>
        </Tooltip>
      );
    }

    // 3. 限流状态 (429)
    if (code === 429) {
      return (
        <Tooltip title={row.errorCategory || t("gateway.rateLimited", { defaultValue: "触发上游限流" })}>
          <Tag color="warning" icon={<WarningOutlined />}>
            429 {t("gateway.rateLimit", { defaultValue: "限流" })}
          </Tag>
        </Tooltip>
      );
    }

    // 4. 鉴权失败状态 (401/403)
    if (code === 401 || code === 403) {
      return (
        <Tooltip title={row.errorCategory || t("gateway.authError", { defaultValue: "鉴权失败" })}>
          <Tag color="error" icon={<CloseCircleOutlined />}>
            {code} {t("gateway.auth", { defaultValue: "鉴权" })}
          </Tag>
        </Tooltip>
      );
    }

    // 5. 上游服务异常 (5xx)
    if (code != null && code >= 500) {
      return (
        <Tooltip title={row.errorCategory || t("gateway.serverError", { defaultValue: "上游服务异常" })}>
          <Tag color="error" icon={<CloseCircleOutlined />}>
            {code}
          </Tag>
        </Tooltip>
      );
    }

    // 6. 其它客户端或上游异常 (4xx)
    if (code != null && code >= 400) {
      return (
        <Tooltip title={row.errorCategory || ""}>
          <Tag color="error">{code}</Tag>
        </Tooltip>
      );
    }

    // 7. 存在错误分类标签
    if (hasErrorCat) {
      return (
        <Tag color="volcano" icon={<WarningOutlined />}>
          {row.errorCategory}
        </Tag>
      );
    }

    // 8. 成功状态（2xx 且排除断流、取消及错误分类）
    if (code != null && code >= 200 && code < 300) {
      return (
        <Tag color="success" icon={<CheckCircleOutlined />}>
          {code}
        </Tag>
      );
    }

    return <Tag color="default">—</Tag>;
  };

  return (
    <Card
      size="small"
      title={
        <Space size={8} wrap>
          <span>{t("proxy.recentRoutes", { defaultValue: "最近路由与请求日志" })}</span>
          {hasActiveFilter ? (
            <Tag color="processing">
              {t("gateway.filterActive", { defaultValue: "已启用筛选" })}
            </Tag>
          ) : (
            <Tag color="default">
              {t("gateway.totalLogsCount", {
                count: totalCount,
                defaultValue: `共 ${totalCount} 条记录`,
              })}
            </Tag>
          )}
        </Space>
      }
      extra={
        <Space size={12} wrap>
          <Space size={4}>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("gateway.autoRefresh", { defaultValue: "自动刷新" })}
            </Text>
            <Switch
              size="small"
              checked={autoRefresh}
              onChange={setAutoRefresh}
            />
          </Space>
          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={logsQuery.isFetching}
            onClick={() => void logsQuery.refetch()}
          >
            {t("common.refresh", { defaultValue: "刷新" })}
          </Button>
        </Space>
      }
      className="gateway-route-logs"
    >
      {/* 筛选工具栏 */}
      <Space wrap size={[8, 8]} style={{ marginBottom: 12, width: "100%" }}>
        <Select
          size="small"
          style={{ width: 140 }}
          value={targetFilter}
          onChange={(val) => {
            setTargetFilter(val);
            setPage(0);
          }}
          options={[
            { value: null, label: t("gateway.allAgents", { defaultValue: "全部应用" }) },
            ...visibleTargets.map((target) => ({
              value: target,
              label: t(LABEL_KEYS[target] ?? `workspace.${target}`),
            })),
          ]}
        />
        <Select
          size="small"
          style={{ width: 140 }}
          value={statusFilter}
          onChange={(val: GatewayRouteLogStatusFilter) => {
            setStatusFilter(val);
            setPage(0);
          }}
          options={[
            { value: "all", label: t("gateway.statusAll", { defaultValue: "全部状态" }) },
            { value: "success", label: t("gateway.statusSuccess", { defaultValue: "仅成功 (2xx)" }) },
            { value: "rate_limited", label: t("gateway.statusRateLimit", { defaultValue: "仅限流 (429)" }) },
            { value: "midstream_error", label: t("gateway.statusMidstream", { defaultValue: "仅中途断流" }) },
            { value: "error", label: t("gateway.statusError", { defaultValue: "仅异常 / 断流" }) },
          ]}
        />
        <Select
          size="small"
          style={{ width: 130 }}
          value={modeFilter}
          onChange={(val: string) => {
            setModeFilter(val);
            setPage(0);
          }}
          options={[
            { value: "all", label: t("gateway.modeAll", { defaultValue: "全部模式" }) },
            { value: "long_context", label: t("gateway.modes.long_context", { defaultValue: "长上下文" }) },
            { value: "think", label: t("gateway.modes.think", { defaultValue: "深度思考" }) },
            { value: "plan", label: t("gateway.modes.plan", { defaultValue: "代码规划" }) },
            { value: "edit", label: t("gateway.modes.edit", { defaultValue: "修改内容" }) },
            { value: "background", label: t("gateway.modes.background", { defaultValue: "后台任务" }) },
            { value: "web_search", label: t("gateway.modes.web_search", { defaultValue: "网络搜索" }) },
            { value: "vision", label: t("gateway.modes.vision", { defaultValue: "视觉理解" }) },
            { value: "image_gen", label: t("gateway.modes.image_gen", { defaultValue: "图像生成" }) },
            { value: "default", label: t("gateway.modes.default", { defaultValue: "默认模式" }) },
            { value: "rule", label: t("gateway.modes.rule", { defaultValue: "自定义规则" }) },
            { value: "explicit_model", label: t("gateway.modes.explicit_model", { defaultValue: "显式指定" }) },
          ]}
        />
        <Input.Search
          size="small"
          placeholder={t("gateway.searchLogPlaceholder", { defaultValue: "搜索模型 / 上游 / ID" })}
          style={{ width: 200 }}
          value={searchKeyword}
          onChange={(e) => setSearchKeyword(e.target.value)}
          onSearch={(val) => {
            setDebouncedKeyword(val);
            setPage(0);
          }}
          allowClear
        />
      </Space>

      {/* 激活筛选提示与快捷重置 */}
      {hasActiveFilter ? (
        <div style={{ marginBottom: 10, fontSize: 12 }}>
          <Space size={8} wrap align="center">
            <Tag color="blue" style={{ margin: 0 }}>
              {t("gateway.filterMatchedTotal", {
                count: totalCount,
                defaultValue: `筛选出 ${totalCount} 条记录`,
              })}
            </Tag>
            <Button
              type="link"
              size="small"
              style={{ padding: 0, height: "auto", fontSize: 12 }}
              onClick={handleResetFilters}
            >
              {t("gateway.filterReset", { defaultValue: "重置筛选" })}
            </Button>
          </Space>
        </div>
      ) : null}

      <Table
        size="small"
        rowKey="id"
        tableLayout="fixed"
        scroll={{ x: 1240 }}
        dataSource={logsQuery.data?.data ?? []}
        loading={logsQuery.isPending && !logsQuery.data}
        pagination={{
          current: (logsQuery.data?.page ?? page) + 1,
          pageSize: logsQuery.data?.pageSize ?? ROUTE_LOG_PAGE_SIZE,
          total: totalCount,
          showSizeChanger: false,
          onChange: (nextPage) => setPage(nextPage - 1),
        }}
        columns={[
          {
            title: t("gateway.time", { defaultValue: "时间" }),
            dataIndex: "createdAt",
            width: 90,
            render: (value: number) => (
              <Tooltip title={new Date(value).toLocaleString()}>
                <span>{new Date(value).toLocaleTimeString()}</span>
              </Tooltip>
            ),
          },
          {
            title: t("gateway.reason", { defaultValue: "依据" }),
            width: 124,
            ellipsis: true,
            render: (_: unknown, row: GatewayRouteLog) => {
              const modeKey = routeLogModeKey(row);
              const label = modeKey
                ? t(`gateway.modes.${modeKey}`, { defaultValue: modeKey })
                : (row.routeReason?.trim() || "—");
              const detail = row.routeReason?.trim() || label;
              return (
                <div className="gateway-route-logs__reason">
                  <Tooltip title={detail}>
                    <Tag
                      color={routeModeColor(row.routeMode || modeKey)}
                      className="gateway-route-logs__tag"
                    >
                      {label}
                    </Tag>
                  </Tooltip>
                </div>
              );
            },
          },
          {
            title: t("gateway.requested", { defaultValue: "请求" }),
            dataIndex: "requestedModel",
            ellipsis: true,
            width: 160,
            render: (value: string | null) => <ModelIdText value={value} />,
          },
          {
            title: t("gateway.model", { defaultValue: "模型" }),
            dataIndex: "model",
            ellipsis: true,
            width: 160,
            render: (value: string | null) => <ModelIdText value={value} />,
          },
          {
            title: t("gateway.upstream", { defaultValue: "上游" }),
            dataIndex: "providerName",
            ellipsis: true,
            width: 130,
            render: (value: string | null) => <EllipsisText value={value} />,
          },
          {
            title: t("gateway.duration", { defaultValue: "耗时" }),
            dataIndex: "durationMs",
            width: 80,
            render: (value: number) => `${value}ms`,
          },
          {
            title: (
              <Tooltip title={t("gateway.rateTooltip", { defaultValue: "平均输出速率（包含首 Token 等待，非纯解码速率）" })}>
                <span style={{ cursor: "help", borderBottom: "1px dotted var(--color-text-tertiary)" }}>
                  {t("gateway.rate", { defaultValue: "速率" })}
                </span>
              </Tooltip>
            ),
            width: 100,
            render: (_: unknown, row: GatewayRouteLog) => formatTokenRate(row),
          },
          {
            title: "Token",
            width: 110,
            render: (_: unknown, row: GatewayRouteLog) => {
              const input = row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens;
              return `${formatCompactNumber(input)} / ${formatCompactNumber(row.outputTokens)}`;
            },
          },
          {
            title: t("gateway.cost", { defaultValue: "费用" }),
            dataIndex: "estimatedCost",
            width: 80,
            render: (value: number) => `$${Number(value ?? 0).toFixed(4)}`,
          },
          {
            title: t("gateway.status", { defaultValue: "状态" }),
            width: 120,
            render: (_: unknown, row: GatewayRouteLog) => {
              const attempts = parseAttempts(row.attemptsJson);
              return (
                <Space size={4} wrap>
                  {renderStatusBadge(row)}
                  {attempts.length > 1 ? (
                    <Tooltip
                      title={t("gateway.attemptsCount", {
                        count: attempts.length,
                        defaultValue: `共 ${attempts.length} 次尝试`,
                      })}
                    >
                      <Tag color="gold">{attempts.length}次尝试</Tag>
                    </Tooltip>
                  ) : row.attemptIndex > 0 ? (
                    <Tooltip
                      title={t("gateway.retryAttemptHint", {
                        index: row.attemptIndex,
                        defaultValue: `第 ${row.attemptIndex} 次重试 / 切换尝试`,
                      })}
                    >
                      <Tag color="gold">#{row.attemptIndex}</Tag>
                    </Tooltip>
                  ) : null}
                </Space>
              );
            },
          },
          {
            title: t("gateway.actions", { defaultValue: "详情" }),
            width: 60,
            fixed: "right",
            render: (_: unknown, row: GatewayRouteLog) => (
              <Button
                type="link"
                size="small"
                icon={<InfoCircleOutlined />}
                onClick={() => setDetailRecord(row)}
              />
            ),
          },
        ]}
      />

      {/* 请求与路由详情抽屉 */}
      <Drawer
        title={
          <Space size={8}>
            <span>{t("gateway.logDetailTitle", { defaultValue: "请求与路由详情" })}</span>
            {activeDetailRecord ? (
              isOffPageSnapshot ? (
                <Tag color="default">{t("gateway.snapshotBadge", { defaultValue: "离页快照" })}</Tag>
              ) : (
                <Tag color="processing">{t("gateway.liveBadge", { defaultValue: "实时数据" })}</Tag>
              )
            ) : null}
          </Space>
        }
        open={Boolean(activeDetailRecord)}
        onClose={() => setDetailRecord(null)}
        extra={<Button loading={logsQuery.isFetching} onClick={() => void refreshUsageQuery(logsQuery)}>{t("common.refresh")}</Button>}
        width={500}
      >
        {activeDetailRecord ? (
          <>
            {isOffPageSnapshot && (
              <Alert
                type="info"
                showIcon
                style={{ marginBottom: 16 }}
                message={
                  <Space direction="vertical" size={2}>
                    <Text strong>{t("gateway.logDetailSnapshotTitle", { defaultValue: "离页快照" })}</Text>
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      {t("gateway.logDetailSnapshotDesc", {
                        defaultValue: "当前记录已不在当前页或当前筛选结果中，展示为打开详情时的静态快照。",
                      })}
                    </Text>
                  </Space>
                }
              />
            )}
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label={t("gateway.requestId", { defaultValue: "请求 ID" })}>
                <Space size={4}>
                  <Text code copyable>{activeDetailRecord.id}</Text>
                </Space>
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.time", { defaultValue: "时间" })}>
                {new Date(activeDetailRecord.createdAt).toLocaleString()}
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.reason", { defaultValue: "路由依据" })}>
                <Space size={6}>
                  <Tag color={routeModeColor(activeDetailRecord.routeMode || routeLogModeKey(activeDetailRecord))}>
                    {routeLogModeKey(activeDetailRecord)
                      ? t(`gateway.modes.${routeLogModeKey(activeDetailRecord)}`, { defaultValue: routeLogModeKey(activeDetailRecord) })
                      : activeDetailRecord.routeReason}
                  </Tag>
                  {activeDetailRecord.routeReason ? (
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      {activeDetailRecord.routeReason}
                    </Text>
                  ) : null}
                </Space>
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.requested", { defaultValue: "客户端请求模型" })}>
                <Text strong>{activeDetailRecord.requestedModel || "auto"}</Text>
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.model", { defaultValue: "网关调度模型" })}>
                <Text strong style={{ color: "var(--ant-color-primary, #1677ff)" }}>
                  {activeDetailRecord.model || "—"}
                </Text>
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.upstream", { defaultValue: "命中上游" })}>
                <Space direction="vertical" size={2}>
                  <Text strong>{activeDetailRecord.providerName || "—"}</Text>
                  {activeDetailRecord.upstreamId ? (
                    <Text type="secondary" code style={{ fontSize: 11 }}>
                      {activeDetailRecord.upstreamId}
                    </Text>
                  ) : null}
                </Space>
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.profileId", { defaultValue: "使用档案" })}>
                <Text code>{activeDetailRecord.profileId || "gprof_shared"}</Text>
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.attemptIndex", { defaultValue: "尝试轮次" })}>
                {parseAttempts(activeDetailRecord.attemptsJson).length > 1 ? (
                  <Tag color="gold">
                    {t("gateway.attemptsCount", {
                      count: parseAttempts(activeDetailRecord.attemptsJson).length,
                      defaultValue: `共 ${parseAttempts(activeDetailRecord.attemptsJson).length} 次尝试`,
                    })}
                  </Tag>
                ) : activeDetailRecord.attemptIndex > 0 ? (
                  <Tag color="gold">
                    {t("gateway.attemptIndexTag", {
                      index: activeDetailRecord.attemptIndex,
                      defaultValue: `重试 / 备用 #${activeDetailRecord.attemptIndex}`,
                    })}
                  </Tag>
                ) : (
                  t("gateway.firstAttempt", { defaultValue: "首选尝试 (0)" })
                )}
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.status", { defaultValue: "HTTP 状态" })}>
                {renderStatusBadge(activeDetailRecord)}
              </Descriptions.Item>
              {activeDetailRecord.errorCategory ? (
                <Descriptions.Item label={t("gateway.errorCategory", { defaultValue: "错误分类" })}>
                  <Tag color="volcano">{activeDetailRecord.errorCategory}</Tag>
                </Descriptions.Item>
              ) : null}
              {activeDetailRecord.streamOutcome ? (
                <Descriptions.Item label={t("gateway.streamOutcome", { defaultValue: "流式结果" })}>
                  {activeDetailRecord.streamOutcome === "complete" ? (
                    <Tag color="green">{activeDetailRecord.streamOutcome}</Tag>
                  ) : activeDetailRecord.streamOutcome === "cancelled" ? (
                    <Tag color="default" icon={<StopOutlined />}>
                      {t("gateway.cancelled", { defaultValue: "已取消" })}
                    </Tag>
                  ) : (
                    <Tag color="magenta">{activeDetailRecord.streamOutcome}</Tag>
                  )}
                </Descriptions.Item>
              ) : null}
              <Descriptions.Item label={t("gateway.duration", { defaultValue: "耗时" })}>
                {activeDetailRecord.durationMs} ms
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.rate", { defaultValue: "Token 速率" })}>
                {formatTokenRate(activeDetailRecord)}
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.tokensDetail", { defaultValue: "Token 消耗" })}>
                <Space direction="vertical" size={2} style={{ width: "100%" }}>
                  <div>{t("gateway.inputTokens", { defaultValue: "输入 Token" })}: {activeDetailRecord.inputTokens}</div>
                  <div>{t("gateway.cacheReadTokens", { defaultValue: "缓存读取 Token" })}: {activeDetailRecord.cacheReadInputTokens}</div>
                  <div>{t("gateway.cacheCreationTokens", { defaultValue: "缓存写入 Token" })}: {activeDetailRecord.cacheCreationInputTokens}</div>
                  <div>{t("gateway.outputTokens", { defaultValue: "输出 Token" })}: {activeDetailRecord.outputTokens}</div>
                </Space>
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.cost", { defaultValue: "预估费用" })}>
                ${Number(activeDetailRecord.estimatedCost ?? 0).toFixed(4)}
              </Descriptions.Item>
            </Descriptions>

            {/* 结构化尝试链渲染：展示故障转移与重试过程中的各个候选上游状态 */}
            {(() => {
              const attempts = parseAttempts(activeDetailRecord.attemptsJson);
              if (attempts.length === 0) return null;
              return (
                <div style={{ marginTop: 16 }}>
                  <Typography.Text strong style={{ display: "block", marginBottom: 8 }}>
                    {t("gateway.attemptsChain", { defaultValue: "切换与重试尝试链" })} ({attempts.length})
                  </Typography.Text>
                  <Space direction="vertical" size={8} style={{ width: "100%" }}>
                    {attempts.map((att, idx) => (
                      <Card
                        key={idx}
                        size="small"
                        style={{
                          borderColor: att.success
                            ? "var(--ant-color-success-border, #b7eb8f)"
                            : "var(--ant-color-error-border, #ffccc7)",
                          background: att.success
                            ? "var(--ant-color-success-bg, #f6ffed)"
                            : "var(--ant-color-error-bg, #fff2f0)",
                        }}
                      >
                        <Space direction="vertical" size={4} style={{ width: "100%" }}>
                          <Space size={6} wrap>
                            <Tag color={att.attemptIndex === 0 ? "blue" : "gold"}>
                              {att.attemptIndex === 0
                                ? t("gateway.attemptPrimary", { defaultValue: "初始尝试 (#0)" })
                                : t("gateway.attemptFallback", {
                                    index: att.attemptIndex,
                                    defaultValue: `重试 / 备用 #${att.attemptIndex}`,
                                  })}
                            </Tag>
                            <Text strong>{att.model}</Text>
                            {att.providerName ? (
                              <Text type="secondary">({att.providerName})</Text>
                            ) : null}
                            {att.statusCode != null ? (
                              <Tag color={att.success ? "success" : "error"}>{att.statusCode}</Tag>
                            ) : null}
                            <Text type="secondary" style={{ fontSize: 11 }}>
                              {att.durationMs}ms
                            </Text>
                            {typeof att.queueWaitMs === "number" && Number.isFinite(att.queueWaitMs) && att.queueWaitMs >= 0 ? (
                              <Text type="secondary" style={{ fontSize: 11 }}>
                                {t("gateway.queueWait", { ms: att.queueWaitMs })}
                              </Text>
                            ) : null}
                          </Space>
                          {att.errorCategory ? (
                            <Space size={4}>
                              <Text type="secondary" style={{ fontSize: 11 }}>
                                {t("gateway.errorCategory", { defaultValue: "错误分类" })}:
                              </Text>
                              <Tag color="volcano">{att.errorCategory}</Tag>
                            </Space>
                          ) : null}
                          {att.diagnostic ? (
                            <Text code style={{ fontSize: 11, wordBreak: "break-all" }}>
                              {att.diagnostic}
                            </Text>
                          ) : null}
                        </Space>
                      </Card>
                    ))}
                  </Space>
                </div>
              );
            })()}
          </>
        ) : null}
      </Drawer>
    </Card>
  );
}
