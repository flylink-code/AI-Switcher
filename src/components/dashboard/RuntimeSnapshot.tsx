import React from "react";
import { Button, Tag, Typography } from "antd";
import ApiOutlined from "@ant-design/icons/es/icons/ApiOutlined";
import ArrowRightOutlined from "@ant-design/icons/es/icons/ArrowRightOutlined";
import { useTranslation } from "react-i18next";
import type { ProviderTarget, ProxyStatus } from "@/types/backend";
import { Surface, Inline, Stack, StatusBadge } from "@/components/ui";
import { useNavigatePage } from "@/lib/navigation";

const { Text } = Typography;

export interface RuntimeSnapshotProps {
  proxyStatus: ProxyStatus | null;
  target: ProviderTarget;
  isAppRunning?: boolean;
  className?: string;
  style?: React.CSSProperties;
}

export const RuntimeSnapshot: React.FC<RuntimeSnapshotProps> = ({
  proxyStatus,
  target,
  isAppRunning,
  className = "",
  style,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigatePage();
  const isOpencode = target === "opencode";
  const proxyRunning = proxyStatus?.running ?? false;
  const port = proxyStatus?.port ?? (target === "codex" ? 15822 : isOpencode ? 15824 : 15821);

  return (
    <Surface padding="md" className={className} style={style}>
      <Stack gap="sm">
        {/* Title Row */}
        <Inline justify="space-between" align="center">
          <Inline gap="sm">
            <ApiOutlined style={{ fontSize: 18, color: "var(--color-brand)" }} />
            <Text strong style={{ fontSize: "var(--font-size-md)" }}>
              {t("dashboard.runtimeTitle", { defaultValue: "运行状态概览 (Runtime Overview)" })}
            </Text>
          </Inline>

          <Button
            type="link"
            size="small"
            icon={<ArrowRightOutlined />}
            onClick={() => navigate("proxy")}
            style={{ fontSize: "var(--font-size-xs)", padding: 0 }}
          >
            {t("dashboard.openProxy", { defaultValue: "管理代理" })}
          </Button>
        </Inline>

        {/* Content Row */}
        {isOpencode ? (
          <Inline gap="sm" align="center">
            <Tag color="blue">{t("workbench.proxyDirect", { defaultValue: "直连 · 无需本地代理" })}</Tag>
          </Inline>
        ) : (
          <Inline justify="space-between" align="center" wrap gap="md">
            <Inline gap="sm" align="center">
              <StatusBadge
                status={proxyRunning ? "running" : "stopped"}
                label={proxyRunning ? t("workbench.proxyRunning", { port }) : t("workbench.proxyStopped")}
              />

              {isAppRunning != null && (
                <Tag color={isAppRunning ? "success" : "default"} style={{ margin: 0 }}>
                  {isAppRunning ? t("workbench.running") : t("workbench.stopped")}
                </Tag>
              )}
            </Inline>

            {proxyStatus?.targetProvider && (
              <Inline gap="xs" align="center">
                <Text style={{ fontSize: "var(--font-size-xs)", color: "var(--color-text-secondary)" }}>
                  {t("proxy.fieldTarget", { defaultValue: "目标供应商" })}:
                </Text>
                <Tag color="blue" style={{ margin: 0 }}>
                  {proxyStatus.targetProvider}
                </Tag>
              </Inline>
            )}
          </Inline>
        )}
      </Stack>
    </Surface>
  );
};
