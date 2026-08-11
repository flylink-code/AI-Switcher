import React from "react";
import { InputNumber, Tag, Typography } from "antd";
import { useTranslation } from "react-i18next";
import type { ProviderTarget, ProxyStatus } from "@/types/backend";
import { Surface, Stack } from "@/components/ui";

const { Text } = Typography;

export interface ProxyRoutePanelProps {
  status: ProxyStatus | null;
  target: ProviderTarget;
  port: number;
  onPortChange: (port: number) => void;
  busy: boolean;
  className?: string;
  style?: React.CSSProperties;
}

function RouteRow({ label, children }: { label: React.ReactNode; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", gap: "var(--space-3)", alignItems: "center", minWidth: 0 }}>
      <span
        style={{
          width: 96,
          flexShrink: 0,
          fontSize: "var(--font-size-xs)",
          color: "var(--color-text-tertiary)",
        }}
      >
        {label}
      </span>
      <span style={{ flex: 1, minWidth: 0 }}>{children}</span>
    </div>
  );
}

/** Route configuration panel: provider, listening port, endpoint. */
export const ProxyRoutePanel: React.FC<ProxyRoutePanelProps> = ({
  status,
  target,
  port,
  onPortChange,
  busy,
  className = "",
  style,
}) => {
  const { t } = useTranslation();
  const isRunning = status?.running ?? false;

  const endpointUrl = target === "codex"
    ? `http://127.0.0.1:${port}/v1/responses`
    : `http://127.0.0.1:${port}/v1/messages`;

  return (
    <Surface padding="md" className={className} style={style}>
      <Stack gap="sm">
        <Text strong style={{ fontSize: "var(--font-size-md)", color: "var(--color-text-primary)" }}>
          {t("proxy.route", { defaultValue: "Route" })}
        </Text>

        <RouteRow label={t("proxy.fieldTarget", { defaultValue: "目标供应商" })}>
          {status?.targetProvider ? (
            <Tag color="blue" style={{ margin: 0 }}>
              {status.targetProvider}
            </Tag>
          ) : (
            <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
              {t("proxy.noTarget", { defaultValue: "未指定" })}
            </Text>
          )}
        </RouteRow>

        <RouteRow label={t("proxy.port", { defaultValue: "端口号" })}>
          <InputNumber
            min={1024}
            max={65535}
            value={port}
            onChange={(v) => v != null && onPortChange(v)}
            disabled={busy || isRunning || status?.phase === "starting"}
            size="small"
            style={{ width: 100 }}
          />
        </RouteRow>

        <RouteRow label={t("proxy.fieldEndpoint", { defaultValue: "接入端点" })}>
          <Text copyable code style={{ fontSize: "var(--font-size-xs)", fontFamily: "var(--font-family-mono)" }}>
            {endpointUrl}
          </Text>
        </RouteRow>
      </Stack>
    </Surface>
  );
};
