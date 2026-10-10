import { useEffect, useMemo, useRef, useState } from "react";
import { Alert, Button, Card, Space, Tag, Typography } from "antd";
import { useQueries, useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { LABEL_KEYS } from "@/components/AgentTargetSwitcher";
import { filterUiAgents } from "@/lib/agentVisibility";
import { refreshUsageQuery, useUsageLogRefresh } from "@/lib/useUsageLogRefresh";
import {
  getAgentConnectionMode,
  getSmartGatewayStatus,
  listGatewayUpstreams,
  listSmartGatewayBindings,
} from "@/services/providers";
import { listProxyRequestLogs } from "@/services/usage";
import type { ProviderTarget, ProxyRequestLog } from "@/types/backend";
import "./GatewayTopology.css";

interface GatewayTopologyResultProps {
  log?: ProxyRequestLog;
  isRecentSuccess: boolean;
}

function GatewayTopologyResult({ log, isRecentSuccess }: GatewayTopologyResultProps) {
  const { t } = useTranslation();
  const isInitialRef = useRef(true);
  const prevFingerprintRef = useRef<string | null>(null);
  const [animating, setAnimating] = useState(false);

  // 终态指纹：监控日志 ID、状态码、流状态及错误分类
  const currentFingerprint = log
    ? `${log.id}:${log.statusCode ?? ""}:${log.streamOutcome ?? ""}:${log.errorCategory ?? ""}`
    : isRecentSuccess
    ? "empty"
    : null;

  useEffect(() => {
    // 拓扑初载不动画：首次渲染或首批查询完成时只记录基准指纹，不触发动画
    if (isInitialRef.current) {
      if (currentFingerprint !== null) {
        isInitialRef.current = false;
        prevFingerprintRef.current = currentFingerprint;
      }
      return;
    }

    // 仅新日志或终态变化时触发 350ms 出现动画
    if (currentFingerprint && currentFingerprint !== prevFingerprintRef.current) {
      prevFingerprintRef.current = currentFingerprint;
      setAnimating(true);
      const timer = window.setTimeout(() => {
        setAnimating(false);
      }, 350);
      return () => window.clearTimeout(timer);
    }
  }, [currentFingerprint]);

  const isCancelled = log?.streamOutcome === "cancelled";
  const isMidstream = log?.streamOutcome === "midstream_error";
  // 取消不显示成功：即使 HTTP 状态码为 200，若客户端主动取消也不视为成功
  const success =
    !!log &&
    log.statusCode !== null &&
    log.statusCode >= 200 &&
    log.statusCode < 300 &&
    !log.errorCategory &&
    !isMidstream &&
    !isCancelled;

  const tagColor = !log
    ? "default"
    : isCancelled
    ? "default"
    : success
    ? "success"
    : "error";

  const tagLabel = !log
    ? isRecentSuccess
      ? t("topology.noRequest")
      : t("topology.unknown")
    : isCancelled
    ? t("gateway.cancelled", { defaultValue: "已取消" })
    : success
    ? t("topology.success")
    : t("topology.failure");

  return (
    <div
      className={`gateway-topology-result${
        animating ? " gateway-topology-result--animated" : ""
      }`}
    >
      <Space wrap size={4}>
        <Tag color={tagColor}>{tagLabel}</Tag>
        {log && (
          <Typography.Text type="secondary">
            {new Date(log.createdAt).toLocaleString()} · {log.providerName ?? "—"} ·{" "}
            {log.model ?? "—"} · {log.statusCode ?? "—"}
          </Typography.Text>
        )}
      </Space>
    </div>
  );
}

export function GatewayTopology({ targets }: { targets: ProviderTarget[] }) {
  const { t } = useTranslation();
  const visible = useMemo(() => filterUiAgents(targets), [targets]);

  const gateway = useQuery({
    queryKey: ["smart-gateway-status"],
    queryFn: getSmartGatewayStatus,
    refetchInterval: 10_000,
  });
  const bindings = useQuery({
    queryKey: ["smart-gateway-bindings"],
    queryFn: listSmartGatewayBindings,
  });
  const upstreams = useQuery({
    queryKey: ["gateway-upstreams"],
    queryFn: listGatewayUpstreams,
  });
  const modes = useQueries({
    queries: visible.map((target) => ({
      queryKey: ["agent-connection-mode", target],
      queryFn: () => getAgentConnectionMode(target),
      refetchInterval: 10_000,
    })),
  });
  const recent = useQueries({
    queries: visible.map((target) => ({
      queryKey: ["agent-latest-request", target],
      queryFn: () => listProxyRequestLogs({ targetApp: target, page: 0, pageSize: 1 }),
    })),
  });

  // 复用 usage-log-recorded 事件，最新请求到达时及时刷新拓扑最新状态（不主动取消查询）
  useUsageLogRefresh({
    enabled: true,
    pollIntervalMs: 10_000,
    onRefresh: () => Promise.all(recent.map((q) => refreshUsageQuery(q))),
  });

  const failed =
    gateway.isError ||
    bindings.isError ||
    upstreams.isError ||
    modes.some((q) => q.isError) ||
    recent.some((q) => q.isError);

  const refresh = () => {
    void gateway.refetch({ cancelRefetch: false });
    void bindings.refetch({ cancelRefetch: false });
    void upstreams.refetch({ cancelRefetch: false });
    modes.forEach((q) => void q.refetch({ cancelRefetch: false }));
    recent.forEach((q) => void q.refetch({ cancelRefetch: false }));
  };

  return (
    <Card
      size="small"
      className="page-surface"
      title={t("topology.title")}
      extra={
        <Button size="small" onClick={refresh}>
          {t("common.refresh")}
        </Button>
      }
    >
      <Space direction="vertical" style={{ width: "100%" }}>
        <Typography.Text type="secondary">{t("topology.hint")}</Typography.Text>
        {failed && <Alert type="warning" showIcon message={t("topology.loadFailed")} />}
        {visible.map((target, index) => {
          const mode = modes[index].isSuccess ? modes[index].data : null;
          const binding = bindings.data?.find((item) => item.targetApp === target);
          const log = recent[index].isSuccess ? recent[index].data?.data[0] : undefined;
          const upstream = upstreams.data?.find((item) => item.id === binding?.directUpstreamId);
          const running = gateway.isSuccess && gateway.data.running;

          return (
            <div className="gateway-topology-row" key={target}>
              <Typography.Text strong>{t(LABEL_KEYS[target])}</Typography.Text>
              <span className="gateway-topology-arrow" aria-hidden="true">
                →
              </span>
              <Space wrap size={4}>
                <Tag color={mode === "gateway" ? (running ? "blue" : "warning") : "default"}>
                  {mode === "gateway"
                    ? `${t(
                        gateway.isError
                          ? "topology.unknown"
                          : running
                          ? "topology.gatewayRunning"
                          : gateway.isPending
                          ? "topology.unknown"
                          : "topology.gatewayStopped",
                      )} ${gateway.isSuccess ? `:${gateway.data.port}` : ""}`
                    : t(
                        mode === "direct"
                          ? "topology.direct"
                          : mode === "official" || mode === "external"
                          ? "topology.official"
                          : "topology.unknown",
                      )}
                </Tag>
                <span className="gateway-topology-arrow" aria-hidden="true">
                  →
                </span>
                <Typography.Text>
                  {mode === "direct"
                    ? upstream?.name ?? t("topology.unknown")
                    : mode === "gateway"
                    ? t("topology.pool")
                    : "—"}
                </Typography.Text>
              </Space>
              <GatewayTopologyResult
                log={log}
                isRecentSuccess={recent[index].isSuccess}
              />
            </div>
          );
        })}
      </Space>
    </Card>
  );
}
