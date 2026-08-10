import React from "react";
import { Alert, Button, InputNumber, Tag, Typography } from "antd";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import ApiOutlined from "@ant-design/icons/es/icons/ApiOutlined";
import { useTranslation } from "react-i18next";
import type { ProviderTarget, ProxyStatus } from "@/types/backend";
import { Surface, Inline, Stack, StatusBadge } from "@/components/ui";

const { Text } = Typography;

export interface ProxyRuntimeCardProps {
  status: ProxyStatus | null;
  target: ProviderTarget;
  port: number;
  onPortChange: (port: number) => void;
  busy: boolean;
  refreshing: boolean;
  onStart: () => void;
  onStop: () => void;
  onRefresh: () => void;
  /** 头部右侧额外内容（如目标切换器）。 */
  headerExtra?: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
}

export const ProxyRuntimeCard: React.FC<ProxyRuntimeCardProps> = ({
  status,
  target,
  port,
  onPortChange,
  busy,
  refreshing,
  onStart,
  onStop,
  onRefresh,
  headerExtra,
  className = "",
  style,
}) => {
  const { t } = useTranslation();
  const isOpencode = target === "opencode";
  const isRunning = status?.running ?? false;

  const getStatusBadge = () => {
    if (!status) return <StatusBadge status="stopped" label={t("proxy.statusUnavailable", { defaultValue: "状态不可用" })} />;
    if (status.phase === "starting") return <StatusBadge status="warning" label={t("proxy.starting", { defaultValue: "启动中..." })} />;
    if (isRunning) return <StatusBadge status="running" label={t("proxy.running", { defaultValue: "运行中" })} />;
    if (status.phase === "error") return <StatusBadge status="error" label={t("proxy.failed", { defaultValue: "异常" })} />;
    return <StatusBadge status="stopped" label={t("proxy.stopped", { defaultValue: "已停止" })} />;
  };

  const endpointUrl = target === "codex"
    ? `http://127.0.0.1:${port}/v1/responses`
    : `http://127.0.0.1:${port}/v1/messages`;

  if (isOpencode) {
    return (
      <Surface padding="md" className={className} style={style}>
        <Inline justify="space-between" align="center" style={{ marginBottom: "var(--space-3)" }}>
          <Inline gap="sm" align="center">
            <ApiOutlined style={{ fontSize: "20px", color: "var(--color-brand)" }} />
            <Text strong style={{ fontSize: "var(--font-size-lg)" }}>
              OpenCode Direct Connection
            </Text>
          </Inline>
          {headerExtra}
        </Inline>
        <Alert type="info" showIcon message={t("proxy.opencodeDirectHint")} />
      </Surface>
    );
  }

  return (
    <Surface
      variant={isRunning ? "elevated" : "default"}
      padding="lg"
      className={className}
      style={{
        borderColor: isRunning ? "var(--color-brand)" : undefined,
        ...style,
      }}
    >
      <Stack gap="md">
        {/* Header Row */}
        <Inline justify="space-between" align="center" wrap gap="sm">
          <Inline gap="sm">
            <ApiOutlined style={{ fontSize: "20px", color: isRunning ? "var(--color-brand)" : "var(--color-text-secondary)" }} />
            <Text strong style={{ fontSize: "var(--font-size-xl)", color: "var(--color-text-primary)" }}>
              {t("proxy.status", { defaultValue: "代理运行状态" })}
            </Text>
            {getStatusBadge()}
          </Inline>

          <Inline gap="sm" align="center">
            {headerExtra}
            <Button
              size="small"
              icon={<ReloadOutlined spin={refreshing} />}
              loading={refreshing}
              onClick={onRefresh}
            >
              {t("proxy.refresh", { defaultValue: "刷新" })}
            </Button>
          </Inline>
        </Inline>

        {/* Runtime Details Surface */}
        <Surface variant="subtle" padding="md" style={{ borderRadius: "var(--radius-md)" }}>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(auto-fit, minmax(220px, 1fr))",
              gap: "var(--space-3) var(--space-5)",
              alignItems: "center",
            }}
          >
            <Inline gap="sm">
              <Text style={{ fontSize: "var(--font-size-xs)", color: "var(--color-text-secondary)", flexShrink: 0 }}>
                {t("proxy.fieldTarget", { defaultValue: "目标供应商" })}
              </Text>
              {status?.targetProvider ? (
                <Tag color="blue" style={{ margin: 0 }}>
                  {status.targetProvider}
                </Tag>
              ) : (
                <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
                  {t("proxy.noTarget", { defaultValue: "未指定" })}
                </Text>
              )}
            </Inline>

            <Inline gap="xs" align="center">
              <Text style={{ fontSize: "var(--font-size-xs)", color: "var(--color-text-secondary)", flexShrink: 0 }}>
                {t("proxy.port", { defaultValue: "端口号" })}
              </Text>
              <InputNumber
                min={1024}
                max={65535}
                value={port}
                onChange={(v) => v != null && onPortChange(v)}
                disabled={busy || isRunning || status?.phase === "starting"}
                size="small"
                style={{ width: 90 }}
              />
            </Inline>

            <Inline gap="sm" align="center" style={{ gridColumn: "1 / -1" }}>
              <Text style={{ fontSize: "var(--font-size-xs)", color: "var(--color-text-secondary)", flexShrink: 0 }}>
                {t("proxy.fieldEndpoint", { defaultValue: "接入端点" })}
              </Text>
              <Text copyable code style={{ fontSize: "var(--font-size-xs)", fontFamily: "var(--font-family-mono)" }}>
                {endpointUrl}
              </Text>
            </Inline>
          </div>
        </Surface>

        {/* Error Alert */}
        {status?.lastError && (
          <Alert type="error" showIcon message={status.lastError} />
        )}

        {/* Actions Footer */}
        <Inline justify="flex-end" align="center" style={{ paddingTop: "var(--space-2)" }}>
          {isRunning ? (
            <Button
              type="primary"
              danger
              icon={<StopOutlined />}
              loading={busy}
              onClick={onStop}
            >
              {t("proxy.stop", { defaultValue: "停止代理" })}
            </Button>
          ) : (
            <Button
              type="primary"
              icon={<PlayCircleOutlined />}
              loading={busy}
              disabled={status?.phase === "starting"}
              onClick={onStart}
            >
              {t("proxy.start", { defaultValue: "启动代理" })}
            </Button>
          )}
        </Inline>
      </Stack>
    </Surface>
  );
};
