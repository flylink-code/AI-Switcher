import React from "react";
import { Button, Tag, Typography } from "antd";
import ClusterOutlined from "@ant-design/icons/es/icons/ClusterOutlined";
import ArrowRightOutlined from "@ant-design/icons/es/icons/ArrowRightOutlined";
import { useTranslation } from "react-i18next";
import type { Provider, ProviderTarget } from "@/types/backend";
import { Surface, Inline, Stack, StatusBadge } from "@/components/ui";
import { ProviderBrandIcon } from "@/components/ProviderBrandIcon";
import { useNavigatePage } from "@/lib/navigation";

const { Text } = Typography;

export interface ProviderSnapshotProps {
  currentProvider?: Provider | null;
  officialCurrent?: boolean;
  target: ProviderTarget;
  className?: string;
  style?: React.CSSProperties;
}

export const ProviderSnapshot: React.FC<ProviderSnapshotProps> = ({
  currentProvider,
  officialCurrent = false,
  target,
  className = "",
  style,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigatePage();
  const isOpencode = target === "opencode";

  return (
    <Surface padding="md" className={className} style={style}>
      <Stack gap="sm">
        {/* Title Row */}
        <Inline justify="space-between" align="center">
          <Inline gap="sm">
            <ClusterOutlined style={{ fontSize: 18, color: "var(--color-brand)" }} />
            <Text strong style={{ fontSize: "var(--font-size-md)" }}>
              {t("dashboard.providerTitle", { defaultValue: "当前供应商 (Current Provider)" })}
            </Text>
          </Inline>

          <Button
            type="link"
            size="small"
            icon={<ArrowRightOutlined />}
            onClick={() => navigate("providers")}
            style={{ fontSize: "var(--font-size-xs)", padding: 0 }}
          >
            {t("dashboard.manageProviders", { defaultValue: "供应商列表" })}
          </Button>
        </Inline>

        {/* Provider Details */}
        {!isOpencode && officialCurrent ? (
          <Inline justify="space-between" align="center">
            <Inline gap="sm">
              <StatusBadge status="current" label={t("providers.officialMode", { defaultValue: "官方模式" })} />
              <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
                {t("providers.officialModeHint", { defaultValue: "使用官方原生 API Endpoint / 账号凭据" })}
              </Text>
            </Inline>
          </Inline>
        ) : currentProvider ? (
          <Stack gap="xs">
            <Inline justify="space-between" align="center">
              <Inline gap="sm">
                <ProviderBrandIcon provider={currentProvider} size={20} />
                <Text strong style={{ fontSize: "var(--font-size-md)", color: "var(--color-text-primary)" }}>
                  {currentProvider.name}
                </Text>
                {!isOpencode && <StatusBadge status="current" label={t("providers.current")} />}
              </Inline>

              <Tag color={currentProvider.protocolType === "anthropic" ? "blue" : "orange"} style={{ margin: 0 }}>
                {currentProvider.protocolType}
              </Tag>
            </Inline>

            <Inline justify="space-between" align="center" style={{ fontSize: "var(--font-size-xs)", color: "var(--color-text-secondary)" }}>
              <Inline gap="xs">
                <span>Model:</span>
                <Text code style={{ fontSize: "var(--font-size-xs)" }}>
                  {currentProvider.model || "Default"}
                </Text>
              </Inline>

              {currentProvider.healthLatencyMs != null && (
                <StatusBadge
                  status={currentProvider.healthStatus === "healthy" ? "healthy" : "error"}
                  label={`${currentProvider.healthLatencyMs}ms`}
                />
              )}
            </Inline>
          </Stack>
        ) : (
          <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
            {t("providers.empty", { defaultValue: "暂无当前可用的供应商" })}
          </Text>
        )}
      </Stack>
    </Surface>
  );
};
