import React, { useState } from "react";
import { Button, Tag, Typography, message } from "antd";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { ProviderTarget } from "@/types/backend";
import { Surface, Inline, Stack, StatusBadge } from "@/components/ui";
import { LABEL_KEYS } from "@/components/AgentTargetSwitcher";
import { usageSourceIcon } from "@/components/UsageSourceIcons";
import { proxyStatusOptions } from "@/lib/appQueries";
import { errMsg } from "@/lib/useProviderActions";
import { setProxyPort, startProxy, stopProxy } from "@/services/api";

const { Text } = Typography;

export interface AgentRuntimeCellProps {
  target: ProviderTarget;
  /** Whether the client app process itself is running. */
  appRunning?: boolean;
  className?: string;
  style?: React.CSSProperties;
}

/**
 * Compact per-agent runtime cell for the Overview grid: proxy status,
 * route (endpoint → provider) and start/stop. Uses the exact same business
 * path as ProxyPage (setProxyPort + startProxy/stopProxy + setQueryData).
 * proxyStatusOptions has no polling — updates arrive via backend events.
 */
export const AgentRuntimeCell: React.FC<AgentRuntimeCellProps> = ({
  target,
  appRunning,
  className = "",
  style,
}) => {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);

  const statusQuery = useQuery(proxyStatusOptions(target));
  const status = statusQuery.data ?? null;
  const isOpencode = target === "opencode";
  const isRunning = status?.running ?? false;

  const handleStart = async () => {
    setBusy(true);
    try {
      const port = status?.port ?? (target === "codex" ? 15822 : 15821);
      await setProxyPort(port, target);
      const s = await startProxy(port, target);
      queryClient.setQueryData(["proxy-status", target], s);
      void message.success(t("proxy.started", { port: s.port }));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleStop = async () => {
    setBusy(true);
    try {
      const s = await stopProxy(target);
      queryClient.setQueryData(["proxy-status", target], s);
      void message.success(t("proxy.stopped"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const statusBadge = () => {
    if (isOpencode)
      return <Tag color="blue" style={{ margin: 0 }}>{t("workbench.proxyDirect", { defaultValue: "直连" })}</Tag>;
    if (!status)
      return <StatusBadge status="stopped" label={t("proxy.statusUnavailable", { defaultValue: "状态不可用" })} />;
    if (status.phase === "starting")
      return <StatusBadge status="warning" label={t("proxy.starting", { defaultValue: "启动中..." })} />;
    if (isRunning)
      return <StatusBadge status="running" label={t("proxy.running", { defaultValue: "运行中" })} />;
    if (status.phase === "error")
      return <StatusBadge status="error" label={t("proxy.failed", { defaultValue: "异常" })} />;
    return <StatusBadge status="stopped" label={t("proxy.stopped", { defaultValue: "已停止" })} />;
  };

  return (
    <Surface padding="md" className={className} style={style}>
      <Stack gap="sm">
        {/* Header: agent identity + proxy status */}
        <Inline justify="space-between" align="center">
          <Inline gap="sm" align="center">
            {usageSourceIcon(target, { size: 16 })}
            <Text strong style={{ fontSize: "var(--font-size-sm)" }}>
              {t(LABEL_KEYS[target])}
            </Text>
            {appRunning != null && (
              <Tag color={appRunning ? "success" : "default"} style={{ margin: 0 }}>
                {appRunning ? t("workbench.running") : t("workbench.stopped")}
              </Tag>
            )}
          </Inline>
          {statusBadge()}
        </Inline>

        {/* Route line: local endpoint → routed provider */}
        {!isOpencode && (
          <Inline justify="space-between" align="center" wrap gap="sm">
            <Text style={{ fontSize: "var(--font-size-xs)", color: "var(--color-text-secondary)" }}>
              <Text code style={{ fontSize: "var(--font-size-xs)" }}>
                127.0.0.1:{status?.port ?? "—"}
              </Text>
              {" → "}
              {status?.targetProvider ? (
                <Text strong style={{ fontSize: "var(--font-size-xs)" }}>
                  {status.targetProvider}
                </Text>
              ) : (
                t("proxy.noTarget", { defaultValue: "未指定" })
              )}
            </Text>

            {isRunning ? (
              <Button
                size="small"
                danger
                icon={<StopOutlined />}
                loading={busy}
                onClick={() => void handleStop()}
              >
                {t("proxy.stop", { defaultValue: "停止代理" })}
              </Button>
            ) : (
              <Button
                size="small"
                type="primary"
                icon={<PlayCircleOutlined />}
                loading={busy}
                disabled={status?.phase === "starting"}
                onClick={() => void handleStart()}
              >
                {t("proxy.start", { defaultValue: "启动代理" })}
              </Button>
            )}
          </Inline>
        )}
      </Stack>
    </Surface>
  );
};
