import React from "react";
import { Button, Card, Divider, Input, InputNumber, Space, Switch, Typography } from "antd";
import { useTranslation } from "react-i18next";
import { SettingsRow } from "@/components/settings";

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
    <Card
      size="small"
      className={`page-surface ${className}`.trim()}
      title={t("proxy.failoverTitle")}
      extra={
        <Switch
          checked={failoverEnabled}
          loading={failoverSaving}
          disabled={failoverSaving}
          checkedChildren={t("common.enabled")}
          unCheckedChildren={t("common.disabled")}
          onChange={onFailoverChange}
        />
      }
      style={style}
    >
      <Space direction="vertical" size="small" style={{ width: "100%" }}>
        <Text type="secondary">{t("proxy.failoverDescription")}</Text>
        <Text type="secondary">{t("proxy.failoverGroupHint")}</Text>
      </Space>
      <Divider style={{ margin: "12px 0 0" }} />
      <SettingsRow
        title={t("proxy.retryCodesTitle")}
        description={t("proxy.retryCodesHint")}
        control={
          <Space.Compact style={{ width: 360, maxWidth: "100%" }}>
            <Input
              value={retryCodes}
              onChange={(event) => onRetryCodesChange(event.target.value)}
              placeholder="400-404,408,429,500-599"
              size="small"
            />
            <Button size="small" loading={retrySaving} onClick={onRetryCodesSave}>
              {t("common.save")}
            </Button>
          </Space.Compact>
        }
      />
      <SettingsRow
        title={t("proxy.idleTimeoutTitle")}
        description={t("proxy.idleTimeoutHint")}
        control={
          <Space.Compact>
            <InputNumber
              min={5}
              max={3600}
              value={idleTimeout}
              onChange={(value) => value != null && onIdleTimeoutChange(value)}
              size="small"
              style={{ width: 120 }}
            />
            <Button size="small" loading={idleSaving} onClick={onIdleTimeoutSave}>
              {t("common.save")}
            </Button>
          </Space.Compact>
        }
        style={{ borderBottom: "none" }}
      />
    </Card>
  );
};
