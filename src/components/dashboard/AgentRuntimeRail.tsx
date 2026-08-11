import React, { useState } from "react";
import { Button, Tag, Typography, message } from "antd";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { ManagedAppRuntimeStatus, ProviderTarget } from "@/types/backend";
import { Surface, Inline, StatusBadge } from "@/components/ui";
import { LABEL_KEYS, TARGET_OPTIONS } from "@/components/AgentTargetSwitcher";
import { usageSourceIcon } from "@/components/UsageSourceIcons";
import { proxyStatusOptions } from "@/lib/appQueries";
import { errMsg } from "@/lib/useProviderActions";
import { setProxyPort, startProxy, stopProxy } from "@/services/api";

const { Text } = Typography;

export interface AgentRuntimeItemProps {
  target: ProviderTarget;
  appRunning?: boolean;
}

export const AgentRuntimeItem: React.FC<AgentRuntimeItemProps> = ({ target, appRunning }) => {
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
      return <StatusBadge status="stopped" label={t("proxy.statusUnavailable", { defaultValue: "不可用" })} />;
    if (status.phase === "starting")
      return <StatusBadge status="warning" label={t("proxy.starting", { defaultValue: "启动中" })} />;
    if (isRunning)
      return <StatusBadge status="running" label={t("proxy.running", { defaultValue: "运行中" })} />;
    if (status.phase === "error")
      return <StatusBadge status="error" label={t("proxy.failed", { defaultValue: "异常" })} />;
    return <StatusBadge status="stopped" label={t("proxy.stopped", { defaultValue: "已停止" })} />;
  };

  return (
    <div
      style={{
        flex: 1,
        minWidth: 180,
        padding: "8px 14px",
        display: "flex",
        flexDirection: "column",
        justifyContent: "space-between",
        gap: "6px",
      }}
    >
      {/* Header */}
      <Inline justify="space-between" align="center">
        <Inline gap="xs" align="center">
          {usageSourceIcon(target, { size: 16 })}
          <Text strong style={{ fontSize: "13px" }}>
            {t(LABEL_KEYS[target])}
          </Text>
          {appRunning != null && (
            <span
              style={{
                width: 6,
                height: 6,
                borderRadius: "50%",
                backgroundColor: appRunning ? "var(--color-success, #52c41a)" : "var(--color-text-tertiary, #bfbfbf)",
                display: "inline-block",
              }}
              title={appRunning ? "App Running" : "App Stopped"}
            />
          )}
        </Inline>
        {statusBadge()}
      </Inline>

      {/* Endpoint & Route */}
      <div style={{ fontSize: "12px", color: "var(--color-text-secondary)" }}>
        {!isOpencode ? (
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: "6px" }}>
            <span style={{ fontFamily: "monospace", fontSize: "11px", color: "var(--color-text-secondary)" }}>
              :{status?.port ?? "—"}
            </span>
            <Text
              ellipsis
              style={{
                maxWidth: 110,
                fontSize: "11px",
                fontWeight: 500,
                color: status?.targetProvider ? "var(--color-text-primary)" : "var(--color-text-tertiary)",
              }}
            >
              {status?.targetProvider ?? t("proxy.noTarget", { defaultValue: "未指定" })}
            </Text>
          </div>
        ) : (
          <span style={{ fontSize: "11px", color: "var(--color-text-tertiary)" }}>
            {t("workbench.noLocalProxyNeeded", { defaultValue: "无需本地代理" })}
          </span>
        )}
      </div>

      {/* Action Button */}
      {!isOpencode && (
        <div style={{ marginTop: "2px" }}>
          {isRunning ? (
            <Button
              size="small"
              danger
              type="text"
              icon={<StopOutlined />}
              loading={busy}
              onClick={() => void handleStop()}
              style={{
                fontSize: "11px",
                height: "24px",
                padding: "0 8px",
                backgroundColor: "rgba(255, 77, 79, 0.06)",
              }}
            >
              {t("proxy.stopShort", { defaultValue: "停止" })}
            </Button>
          ) : (
            <Button
              size="small"
              type="primary"
              ghost
              icon={<PlayCircleOutlined />}
              loading={busy}
              disabled={status?.phase === "starting"}
              onClick={() => void handleStart()}
              style={{ fontSize: "11px", height: "24px", padding: "0 8px" }}
            >
              {t("proxy.startShort", { defaultValue: "启动" })}
            </Button>
          )}
        </div>
      )}
    </div>
  );
};

export interface AgentRuntimeRailProps {
  appRunningStatus?: ManagedAppRuntimeStatus | Record<string, boolean>;
  className?: string;
  style?: React.CSSProperties;
}

/**
 * Unified Agent Runtime Rail for Overview:
 * Displays Claude Code, Claude Desktop, Codex, and OpenCode in a single Surface rail with vertical dividers.
 */
export const AgentRuntimeRail: React.FC<AgentRuntimeRailProps> = ({
  appRunningStatus,
  className = "",
  style,
}) => {
  const { t } = useTranslation();

  const appRunningKeyMap: Record<ProviderTarget, string> = {
    claude_code: "claudeCode",
    claude_desktop: "claudeDesktop",
    codex: "codex",
    opencode: "opencode",
  };

  return (
    <Surface padding="none" className={className} style={{ overflow: "hidden", ...style }}>
      <div
        style={{
          padding: "6px 14px 4px 14px",
          borderBottom: "1px solid var(--color-border-subtle, rgba(0,0,0,0.06))",
          fontSize: "11px",
          fontWeight: 600,
          color: "var(--color-text-secondary)",
          letterSpacing: "0.2px",
        }}
      >
        {t("workbench.runtimeRailTitle", { defaultValue: "运行环境" })}
      </div>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          alignItems: "stretch",
        }}
      >
        {TARGET_OPTIONS.map((target, idx) => (
          <React.Fragment key={target}>
            {idx > 0 && (
              <div
                style={{
                  width: 1,
                  backgroundColor: "var(--color-border-subtle, rgba(0,0,0,0.06))",
                  alignSelf: "stretch",
                }}
              />
            )}
            <AgentRuntimeItem
              target={target}
              appRunning={appRunningStatus ? Boolean(appRunningStatus[appRunningKeyMap[target] as keyof ManagedAppRuntimeStatus]) : undefined}
            />
          </React.Fragment>
        ))}
      </div>
    </Surface>
  );
};
