import React from "react";
import { Alert, Button, Typography } from "antd";
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
  /** Localized label of the page-local Agent target. */
  clientLabel: string;
  busy: boolean;
  refreshing: boolean;
  onStart: () => void;
  onStop: () => void;
  onRefresh: () => void;
  /** Page-local Agent switcher, rendered at the left of the header row. */
  headerExtra?: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
}

/**
 * Runtime hero: current proxy state + context line + dominant start/stop.
 * The Agent switcher lives in the header row (page-local target).
 */
export const ProxyRuntimeCard: React.FC<ProxyRuntimeCardProps> = ({
  status,
  target,
  clientLabel,
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

  if (isOpencode) {
    return (
      <Surface padding="md" className={className} style={style}>
        <Inline gap="sm" align="center" style={{ marginBottom: "var(--space-3)" }}>
          {headerExtra}
          <ApiOutlined style={{ fontSize: "20px", color: "var(--color-brand)" }} />
          <Text strong style={{ fontSize: "var(--font-size-lg)" }}>
            OpenCode Direct Connection
          </Text>
        </Inline>
        <Alert type="info" showIcon message={t("proxy.opencodeDirectHint")} />
      </Surface>
    );
  }

  return (
    <Surface
      variant={isRunning ? "elevated" : "default"}
      padding="md"
      className={className}
      style={{
        borderColor: isRunning ? "var(--color-brand)" : undefined,
        ...style,
      }}
    >
      <Stack gap="sm">
        <Inline justify="space-between" align="center" wrap gap="sm">
          <Inline gap="sm" align="center">
            {headerExtra}
            <ApiOutlined style={{ fontSize: "20px", color: isRunning ? "var(--color-brand)" : "var(--color-text-secondary)" }} />
            <Text strong style={{ fontSize: "var(--font-size-lg)", color: "var(--color-text-primary)" }}>
              {t("proxy.runtime", { defaultValue: "Proxy Runtime" })}
            </Text>
            {getStatusBadge()}
          </Inline>

          <Inline gap="sm" align="center">
            <Button
              size="small"
              icon={<ReloadOutlined spin={refreshing} />}
              loading={refreshing}
              onClick={onRefresh}
            >
              {t("proxy.refresh", { defaultValue: "刷新" })}
            </Button>
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
        </Inline>

        {/* Context line: Client → local endpoint → routed provider */}
        <Text style={{ fontSize: "var(--font-size-sm)", color: "var(--color-text-secondary)" }}>
          {clientLabel}
          {" → "}
          <Text code style={{ fontSize: "var(--font-size-xs)" }}>
            127.0.0.1:{status?.port ?? "—"}
          </Text>
          {" → "}
          {status?.targetProvider ? (
            <Text strong style={{ fontSize: "var(--font-size-sm)" }}>
              {status.targetProvider}
            </Text>
          ) : (
            t("proxy.noTarget", { defaultValue: "未指定" })
          )}
        </Text>

        {/* Error Alert */}
        {status?.lastError && (
          <Alert type="error" showIcon message={status.lastError} />
        )}
      </Stack>
    </Surface>
  );
};
