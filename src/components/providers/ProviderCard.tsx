import React from "react";
import { Button, Dropdown, Modal, Popconfirm, Tag, Tooltip, Typography, type MenuProps } from "antd";
import { useTranslation } from "react-i18next";
import ArrowUpOutlined from "@ant-design/icons/es/icons/ArrowUpOutlined";
import ArrowDownOutlined from "@ant-design/icons/es/icons/ArrowDownOutlined";
import SafetyCertificateOutlined from "@ant-design/icons/es/icons/SafetyCertificateOutlined";
import FieldTimeOutlined from "@ant-design/icons/es/icons/FieldTimeOutlined";
import GlobalOutlined from "@ant-design/icons/es/icons/GlobalOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import EditOutlined from "@ant-design/icons/es/icons/EditOutlined";
import EllipsisOutlined from "@ant-design/icons/es/icons/EllipsisOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import type { Provider } from "@/types/backend";
import { Surface, Inline, Stack, StatusBadge, IconButton } from "@/components/ui";
import { ProviderBrandIcon } from "@/components/ProviderBrandIcon";

const { Text } = Typography;

export interface ProviderCardProps {
  provider: Provider;
  index: number;
  totalCount: number;
  busy: boolean;
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onTest: (provider: Provider) => void;
  onSpeedtest: (provider: Provider) => void;
  onShareLink: (provider: Provider) => void;
  onMove: (id: string, delta: number) => void;
  className?: string;
  style?: React.CSSProperties;
}

export const ProviderCard: React.FC<ProviderCardProps> = ({
  provider,
  index,
  totalCount,
  busy,
  onSwitch,
  onEdit,
  onDelete,
  onTest,
  onSpeedtest,
  onShareLink,
  onMove,
  className = "",
  style,
}) => {
  const { t } = useTranslation();
  const isOpencode = provider.targetApp === "opencode";
  const isCurrent = provider.isCurrent;

  const moreItems: MenuProps["items"] = [
    {
      key: "up",
      icon: <ArrowUpOutlined />,
      label: t("providers.moveUp", { defaultValue: "上移" }),
      disabled: index === 0 || busy,
      onClick: () => onMove(provider.id, -1),
    },
    {
      key: "down",
      icon: <ArrowDownOutlined />,
      label: t("providers.moveDown", { defaultValue: "下移" }),
      disabled: index === totalCount - 1 || busy,
      onClick: () => onMove(provider.id, 1),
    },
    {
      key: "test",
      icon: <SafetyCertificateOutlined />,
      label: t("providers.testConnection", { defaultValue: "测试连接" }),
      disabled: busy || !provider.apiKeySet,
      onClick: () => onTest(provider),
    },
    {
      key: "speed",
      icon: <FieldTimeOutlined />,
      label: t("providers.speedtest", { defaultValue: "测速" }),
      disabled: busy || !provider.baseUrl,
      onClick: () => onSpeedtest(provider),
    },
    {
      key: "share",
      icon: <GlobalOutlined />,
      label: t("deeplink.shareLink", { defaultValue: "分享链接" }),
      disabled: busy,
      onClick: () => onShareLink(provider),
    },
    { type: "divider" },
    {
      key: "delete",
      icon: <DeleteOutlined />,
      danger: true,
      label: t("providers.delete", { defaultValue: "删除" }),
      disabled: busy,
      onClick: () => {
        Modal.confirm({
          title: t("providers.confirmDelete"),
          okText: t("providers.delete"),
          cancelText: t("providers.cancel"),
          okButtonProps: { danger: true },
          onOk: () => onDelete(provider),
        });
      },
    },
  ];

  return (
    <Surface
      variant={isCurrent ? "elevated" : "default"}
      padding="md"
      className={className}
      style={{
        borderColor: isCurrent ? "var(--color-brand)" : undefined,
        backgroundColor: isCurrent ? "var(--color-brand-subtle)" : undefined,
        ...style,
      }}
    >
      <Stack gap="sm">
        {/* Header Row */}
        <Inline justify="space-between" align="center">
          <Inline gap="sm">
            <ProviderBrandIcon provider={provider} size={24} />
            <Text strong style={{ fontSize: "var(--font-size-lg)", color: "var(--color-text-primary)" }}>
              {provider.name}
            </Text>
            {!isOpencode && isCurrent && (
              <StatusBadge status="current" label={t("providers.current")} />
            )}
            {provider.healthStatus && (
              <StatusBadge
                status={provider.healthStatus === "healthy" ? "healthy" : "error"}
                label={`${provider.healthStatus === "healthy" ? t("providers.healthy") : t("providers.unhealthy")}${provider.healthLatencyMs != null ? ` · ${provider.healthLatencyMs}ms` : ""}`}
              />
            )}
          </Inline>

          <Tag color={provider.protocolType === "anthropic" ? "blue" : "orange"} style={{ margin: 0 }}>
            {provider.protocolType}
          </Tag>
        </Inline>

        {/* Base URL */}
        <div style={{ fontSize: "var(--font-size-xs)", fontFamily: "var(--font-family-mono)", wordBreak: "break-all" }}>
          <Text code copyable ellipsis={{ tooltip: provider.baseUrl }}>
            {provider.baseUrl}
          </Text>
        </div>

        {/* Model & Extra Info */}
        <Inline justify="space-between" align="center" style={{ fontSize: "var(--font-size-xs)", color: "var(--color-text-secondary)" }}>
          <Inline gap="xs">
            <span>Model:</span>
            <Text code style={{ fontSize: "var(--font-size-xs)" }}>
              {provider.model || t("providers.defaultModel", { defaultValue: "默认" })}
            </Text>
          </Inline>

          {provider.failoverGroup > 0 && (
            <Tag style={{ margin: 0 }}>
              Group {provider.failoverGroup}
            </Tag>
          )}
        </Inline>

        {/* Actions Footer */}
        <Inline justify="space-between" align="center" style={{ paddingTop: "var(--space-2)", borderTop: "1px solid var(--color-border-subtle)", marginTop: "var(--space-1)" }}>
          <Inline gap="xs">
            <Button
              size="small"
              icon={<SafetyCertificateOutlined />}
              disabled={busy || !provider.apiKeySet}
              onClick={() => onTest(provider)}
            >
              {t("providers.testConnection", { defaultValue: "测试" })}
            </Button>
            <IconButton
              icon={<EditOutlined />}
              title={t("providers.edit")}
              disabled={busy}
              onClick={() => onEdit(provider)}
            />
            <Dropdown menu={{ items: moreItems }} trigger={["click"]}>
              <Button size="small" icon={<EllipsisOutlined />} disabled={busy} aria-label={t("providers.moreActions")} />
            </Dropdown>
          </Inline>

          {!isOpencode && (
            <Button
              size="small"
              type={isCurrent ? "default" : "primary"}
              disabled={isCurrent || busy}
              icon={<ThunderboltOutlined />}
              onClick={() => onSwitch(provider)}
            >
              {isCurrent ? t("providers.current") : t("providers.switch")}
            </Button>
          )}
        </Inline>
      </Stack>
    </Surface>
  );
};
