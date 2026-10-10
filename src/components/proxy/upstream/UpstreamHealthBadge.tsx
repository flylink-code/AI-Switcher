import React from "react";
import { Space, Tag, Tooltip, Typography } from "antd";
import CheckCircleOutlined from "@ant-design/icons/es/icons/CheckCircleOutlined";
import CloseCircleOutlined from "@ant-design/icons/es/icons/CloseCircleOutlined";
import WarningOutlined from "@ant-design/icons/es/icons/WarningOutlined";
import QuestionCircleOutlined from "@ant-design/icons/es/icons/QuestionCircleOutlined";
import type { GatewayUpstreamHealth } from "@/types/backend";

const { Text } = Typography;

export interface UpstreamHealthBadgeProps {
  health?: GatewayUpstreamHealth;
  t: (key: string, opts?: Record<string, unknown>) => string;
}

export function UpstreamHealthBadge({ health, t }: UpstreamHealthBadgeProps) {
  if (!health || health.status === "unknown") {
    return (
      <Tag icon={<QuestionCircleOutlined />}>
        {t("proxy.healthUnknown", { defaultValue: "未检测" })}
      </Tag>
    );
  }

  const lastError = health.lastError;
  const timeStr = health.lastCheckedAt > 0 ? new Date(health.lastCheckedAt).toLocaleString() : null;

  const tooltipContent =
    timeStr || lastError || health.consecutiveFailures > 0 ? (
      <div style={{ maxWidth: 320, wordBreak: "break-all" }}>
        {timeStr ? (
          <div>
            {t("proxy.healthLastChecked", {
              time: timeStr,
              defaultValue: `检测时间：${timeStr}`,
            })}
          </div>
        ) : null}
        {health.consecutiveFailures > 0 ? (
          <div style={{ marginTop: 2, color: "#faad14" }}>
            {t("proxy.healthConsecutiveFailures", {
              count: health.consecutiveFailures,
              defaultValue: `连续失败次数：${health.consecutiveFailures}`,
            })}
          </div>
        ) : null}
        {lastError ? (
          <div style={{ marginTop: 4, color: "#ff7875" }}>
            {t("proxy.healthLastError", {
              error: lastError,
              defaultValue: `错误信息：${lastError}`,
            })}
          </div>
        ) : null}
      </div>
    ) : null;

  const wrapWithTooltip = (node: React.ReactNode) => {
    if (!tooltipContent) return node;
    return <Tooltip title={tooltipContent}>{node}</Tooltip>;
  };

  if (health.status === "ok") {
    const latency =
      health.lastLatencyMs != null
        ? t("proxy.healthLatency", { ms: health.lastLatencyMs, defaultValue: "{{ms}}ms" })
        : "";
    return wrapWithTooltip(
      <Space size={4}>
        <Tag color="success" icon={<CheckCircleOutlined />}>
          {t("proxy.healthOk", { defaultValue: "正常" })}
        </Tag>
        {latency ? (
          <Text type="secondary" style={{ fontSize: 12 }}>
            {latency}
          </Text>
        ) : null}
      </Space>,
    );
  }

  if (health.status === "cooling" || health.status === "rate_limited") {
    const secs = Math.max(1, Math.ceil(health.cooldownRemainingMs / 1000));
    return wrapWithTooltip(
      <Space size={4}>
        <Tag color="warning" icon={<WarningOutlined />}>
          {t("proxy.healthCooling", { secs, defaultValue: `冷却中 ${secs}s` })}
        </Tag>
        <Text type="secondary" style={{ fontSize: 12 }}>
          ×{health.consecutiveFailures}
        </Text>
      </Space>,
    );
  }

  if (health.status === "auth_failed") {
    return wrapWithTooltip(
      <Space size={4}>
        <Tag color="error" icon={<CloseCircleOutlined />}>
          {t("proxy.healthAuthFailed", { defaultValue: "鉴权失败" })}
        </Tag>
        <Text type="secondary" style={{ fontSize: 12 }}>
          ×{health.consecutiveFailures}
        </Text>
        {health.lastLatencyMs != null ? (
          <Text type="secondary" style={{ fontSize: 12 }}>
            {health.lastLatencyMs}ms
          </Text>
        ) : null}
      </Space>,
    );
  }

  return wrapWithTooltip(
    <Space size={4}>
      <Tag color="error" icon={<CloseCircleOutlined />}>
        {t("proxy.healthFailing", { defaultValue: "连续失败" })}
      </Tag>
      <Text type="secondary" style={{ fontSize: 12 }}>
        ×{health.consecutiveFailures}
      </Text>
      {health.lastLatencyMs != null ? (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {health.lastLatencyMs}ms
        </Text>
      ) : null}
    </Space>,
  );
}
