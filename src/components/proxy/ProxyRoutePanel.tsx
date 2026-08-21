import React from "react";
import { Alert, Card, Descriptions, InputNumber, Tag, Typography } from "antd";
import { useTranslation } from "react-i18next";
import type { ProviderTarget, ProxyStatus } from "@/types/backend";

const { Text } = Typography;

export interface ProxyRoutePanelProps {
  status: ProxyStatus | null;
  target: ProviderTarget;
  port: number;
  onPortChange: (port: number) => void;
  busy: boolean;
  /** Localized label of the page-local Agent target. */
  clientLabel: string;
  className?: string;
  style?: React.CSSProperties;
}

/** Route configuration panel: provider, listening port, endpoint. */
export const ProxyRoutePanel: React.FC<ProxyRoutePanelProps> = ({
  status,
  target,
  port,
  onPortChange,
  busy,
  clientLabel,
  className = "",
  style,
}) => {
  const { t } = useTranslation();
  const isRunning = status?.running ?? false;
  const isOpencode = target === "opencode";
  const portLocked = busy || isRunning || status?.phase === "starting";

  const endpointUrl = target === "codex"
    ? `http://127.0.0.1:${port}/v1/responses`
    : `http://127.0.0.1:${port}/v1/messages`;

  const summary = isOpencode
    ? null
    : (
      <Text type="secondary" className="proxy-context-strip">
        {clientLabel}
        {" · "}
        <Text code style={{ fontSize: "var(--font-size-xs)" }}>
          127.0.0.1:{status?.port ?? port}
        </Text>
        {" · "}
        {status?.targetProvider ?? t("proxy.noTarget")}
      </Text>
    );

  return (
    <Card
      size="small"
      className={`page-surface ${className}`.trim()}
      title={t("proxy.route")}
      extra={summary}
      style={style}
    >
      {isOpencode && (
        <Alert type="info" showIcon message={t("proxy.opencodeDirectHint")} style={{ marginBottom: 12 }} />
      )}
      {status?.lastError && (
        <Alert type="error" showIcon message={status.lastError} style={{ marginBottom: 12 }} />
      )}
      <Descriptions column={1} size="small" bordered>
        <Descriptions.Item label={t("proxy.fieldTarget")}>
          {status?.targetProvider ? (
            <Tag color="blue" style={{ margin: 0 }}>
              {status.targetProvider}
            </Tag>
          ) : (
            <Text type="secondary">{t("proxy.noTarget")}</Text>
          )}
        </Descriptions.Item>
        <Descriptions.Item label={t("proxy.port")}>
          <InputNumber
            min={1024}
            max={65535}
            value={port}
            onChange={(value) => value != null && onPortChange(value)}
            disabled={portLocked}
            size="small"
            style={{ width: 120 }}
          />
          {portLocked && !isOpencode && (
            <Text type="secondary" style={{ marginLeft: 8, fontSize: 12 }}>
              {t("proxy.portRunningHint")}
            </Text>
          )}
        </Descriptions.Item>
        {!isOpencode && (
          <Descriptions.Item label={t("proxy.fieldEndpoint")}>
            <Text copyable code style={{ wordBreak: "break-all" }}>
              {endpointUrl}
            </Text>
          </Descriptions.Item>
        )}
      </Descriptions>
    </Card>
  );
};
