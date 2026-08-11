import React from "react";
import { Button, Divider, Input, InputNumber, Space, Switch, Typography } from "antd";
import SafetyCertificateOutlined from "@ant-design/icons/es/icons/SafetyCertificateOutlined";
import FieldTimeOutlined from "@ant-design/icons/es/icons/FieldTimeOutlined";
import { useTranslation } from "react-i18next";
import { Surface, Inline, Stack } from "@/components/ui";

const { Text } = Typography;

export interface ResilienceSettingsProps {
  failoverEnabled: boolean;
  failoverSaving: boolean;
  onFailoverChange: (enabled: boolean) => void;
  retryCodes: string;
  onRetryCodesChange: (codes: string) => void;
  retrySaving: boolean;
  onRetryCodesSave: () => void;
  idleTimeout: number;
  onIdleTimeoutChange: (timeout: number) => void;
  idleSaving: boolean;
  onIdleTimeoutSave: () => void;
  className?: string;
  style?: React.CSSProperties;
}

export const ResilienceSettings: React.FC<ResilienceSettingsProps> = ({
  failoverEnabled,
  failoverSaving,
  onFailoverChange,
  retryCodes,
  onRetryCodesChange,
  retrySaving,
  onRetryCodesSave,
  idleTimeout,
  onIdleTimeoutChange,
  idleSaving,
  onIdleTimeoutSave,
  className = "",
  style,
}) => {
  const { t } = useTranslation();

  return (
    <Surface padding="md" className={className} style={style}>
      <Stack gap="md">
        {/* Failover Header */}
        <Inline justify="space-between" align="center">
          <Inline gap="sm">
            <SafetyCertificateOutlined style={{ fontSize: 16, color: "var(--color-brand)" }} />
            <Text strong style={{ fontSize: "var(--font-size-md)", color: "var(--color-text-primary)" }}>
              {t("proxy.failoverTitle", { defaultValue: "自动故障切换 (Failover)" })}
            </Text>
          </Inline>

          <Switch
            checked={failoverEnabled}
            loading={failoverSaving}
            disabled={failoverSaving}
            checkedChildren={t("common.enabled", { defaultValue: "开启" })}
            unCheckedChildren={t("common.disabled", { defaultValue: "关闭" })}
            onChange={onFailoverChange}
          />
        </Inline>

        <Stack gap="xs">
          <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
            {t("proxy.failoverDescription")}
          </Text>
          <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
            {t("proxy.failoverGroupHint")}
          </Text>
        </Stack>

        <Divider style={{ margin: 0 }} />

        {/* Resilience Options: two-column grid on wide screens */}
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fit, minmax(300px, 1fr))",
            gap: "var(--space-4) var(--space-6)",
          }}
        >
          {/* Retry Status Codes */}
          <Stack gap="xs">
            <Text strong style={{ fontSize: "var(--font-size-md)" }}>
              {t("proxy.retryCodesTitle", { defaultValue: "触发重试的 HTTP 状态码" })}
            </Text>
            <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
              {t("proxy.retryCodesHint")}
            </Text>
            <Space.Compact style={{ width: "100%", maxWidth: 460 }}>
              <Input
                value={retryCodes}
                onChange={(e) => onRetryCodesChange(e.target.value)}
                placeholder="400-404,408,429,500-599"
                size="small"
              />
              <Button size="small" loading={retrySaving} onClick={onRetryCodesSave}>
                {t("common.save", { defaultValue: "保存" })}
              </Button>
            </Space.Compact>
          </Stack>

          {/* Streaming Idle Timeout */}
          <Stack gap="xs">
            <Inline gap="sm" align="center">
              <FieldTimeOutlined style={{ color: "var(--color-brand)" }} />
              <Text strong style={{ fontSize: "var(--font-size-md)" }}>
                {t("proxy.idleTimeoutTitle", { defaultValue: "流式断连超时时间 (秒)" })}
              </Text>
            </Inline>
            <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
              {t("proxy.idleTimeoutHint")}
            </Text>
            <Inline gap="sm">
              <InputNumber
                min={5}
                max={3600}
                value={idleTimeout}
                onChange={(v) => v != null && onIdleTimeoutChange(v)}
                size="small"
                style={{ width: 120 }}
              />
              <Button size="small" type="text" loading={idleSaving} onClick={onIdleTimeoutSave}>
                {t("common.save", { defaultValue: "保存" })}
              </Button>
            </Inline>
          </Stack>
        </div>
      </Stack>
    </Surface>
  );
};
