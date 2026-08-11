import React from "react";
import { Button, Dropdown, Empty, Modal, Tag, Typography, type MenuProps } from "antd";
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
import { Surface, Inline, Stack, StatusBadge } from "@/components/ui";
import { ProviderBrandIcon } from "@/components/ProviderBrandIcon";

const { Text } = Typography;

export interface ProviderDetailProps {
  provider: Provider | null;
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

function DetailRow({ label, children }: { label: React.ReactNode; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", gap: "var(--space-3)", alignItems: "baseline", minWidth: 0 }}>
      <span
        style={{
          width: 88,
          flexShrink: 0,
          fontSize: "var(--font-size-xs)",
          color: "var(--color-text-tertiary)",
        }}
      >
        {label}
      </span>
      <span style={{ flex: 1, minWidth: 0, fontSize: "var(--font-size-sm)", color: "var(--color-text-primary)" }}>
        {children}
      </span>
    </div>
  );
}

/** Detail pane for the selected provider (right side of the workspace). */
export const ProviderDetail: React.FC<ProviderDetailProps> = ({
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

  if (!provider) {
    return (
      <Surface padding="lg" className={className} style={{ display: "flex", alignItems: "center", justifyContent: "center", ...style }}>
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description={t("providers.selectProvider", { defaultValue: "选择左侧供应商查看详情" })}
        />
      </Surface>
    );
  }

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
    <Surface padding="md" className={className} style={style}>
      <Stack gap="md">
        {/* Header */}
        <Inline justify="space-between" align="center" wrap>
          <Inline gap="sm" align="center">
            <ProviderBrandIcon provider={provider} size={28} />
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

        {/* Fields */}
        <Stack gap="sm">
          <DetailRow label="Endpoint">
            <Text code copyable ellipsis={{ tooltip: provider.baseUrl }} style={{ fontSize: "var(--font-size-xs)" }}>
              {provider.baseUrl}
            </Text>
          </DetailRow>
          <DetailRow label="Model">
            <Text code style={{ fontSize: "var(--font-size-xs)" }}>
              {provider.model || t("providers.defaultModel", { defaultValue: "默认" })}
            </Text>
          </DetailRow>
          <DetailRow label="Protocol">{provider.protocolType}</DetailRow>
          {provider.failoverGroup > 0 && (
            <DetailRow label="Failover">Group {provider.failoverGroup}</DetailRow>
          )}
          {provider.notes && (
            <DetailRow label={t("providers.notes", { defaultValue: "备注" })}>{provider.notes}</DetailRow>
          )}
        </Stack>

        {/* Actions */}
        <Inline justify="space-between" align="center" style={{ paddingTop: "var(--space-3)", borderTop: "1px solid var(--color-border-subtle)" }}>
          <Inline gap="sm">
            <Button
              size="small"
              icon={<SafetyCertificateOutlined />}
              disabled={busy || !provider.apiKeySet}
              onClick={() => onTest(provider)}
            >
              {t("providers.testConnection", { defaultValue: "测试连接" })}
            </Button>
            <Button
              size="small"
              icon={<EditOutlined />}
              disabled={busy}
              onClick={() => onEdit(provider)}
            >
              {t("providers.edit")}
            </Button>
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
