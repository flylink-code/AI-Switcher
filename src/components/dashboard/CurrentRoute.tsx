import React, { useEffect, useState } from "react";
import { Button, Select, Tag, Typography } from "antd";
import ClusterOutlined from "@ant-design/icons/es/icons/ClusterOutlined";
import ArrowRightOutlined from "@ant-design/icons/es/icons/ArrowRightOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { ProviderTarget } from "@/types/backend";
import { Surface, Inline, Stack, StatusBadge } from "@/components/ui";
import { ProviderBrandIcon } from "@/components/ProviderBrandIcon";
import { LABEL_KEYS, TARGET_OPTIONS } from "@/components/AgentTargetSwitcher";
import { proxyStatusOptions } from "@/lib/appQueries";
import { useNavigatePage } from "@/lib/navigation";
import { useProvidersStore } from "@/stores/providersStore";

const { Text } = Typography;

export interface CurrentRouteProps {
  className?: string;
  style?: React.CSSProperties;
}

/**
 * Overview component to inspect current request routing for a selected Agent.
 * Answers: "How are requests flowing for this Agent?"
 * Layout: Agent → Local Port → Provider → Model
 */
export const CurrentRoute: React.FC<CurrentRouteProps> = ({ className = "", style }) => {
  const { t } = useTranslation();
  const navigate = useNavigatePage();
  const [selectedTarget, setSelectedTarget] = useState<ProviderTarget>("claude_code");

  const providersStore = useProvidersStore();

  useEffect(() => {
    void providersStore.load(selectedTarget);
  }, [selectedTarget]);

  const statusQuery = useQuery(proxyStatusOptions(selectedTarget));
  const proxyStatus = statusQuery.data ?? null;

  const currentProvider = providersStore.providers.find((p) => p.isCurrent) || null;
  const officialCurrent = !providersStore.providers.some((p) => p.isCurrent);
  const isOpencode = selectedTarget === "opencode";

  return (
    <Surface padding="md" className={className} style={{ height: "100%", boxSizing: "border-box", ...style }}>
      <Stack gap="md" style={{ height: "100%", justifyContent: "space-between" }}>
        {/* Header: Title + Agent Selector + Link */}
        <Inline justify="space-between" align="center" wrap gap="sm">
          <Inline gap="sm" align="center">
            <ClusterOutlined style={{ fontSize: 16, color: "var(--color-brand)" }} />
            <Text strong style={{ fontSize: "var(--font-size-md)" }}>
              {t("workbench.currentRouteTitle", { defaultValue: "当前路由" })}
            </Text>
          </Inline>

          <Inline gap="sm" align="center">
            <Select<ProviderTarget>
              size="small"
              value={selectedTarget}
              onChange={setSelectedTarget}
              style={{ width: 124 }}
              options={TARGET_OPTIONS.map((tgt) => ({
                value: tgt,
                label: t(LABEL_KEYS[tgt]),
              }))}
            />
            <Button
              type="link"
              size="small"
              icon={<ArrowRightOutlined />}
              onClick={() => navigate("providers")}
              style={{ fontSize: "var(--font-size-xs)", padding: 0 }}
            >
              {t("dashboard.manageProviders", { defaultValue: "配置" })}
            </Button>
          </Inline>
        </Inline>

        {/* Route Flow Canvas */}
        {isOpencode ? (
          <div
            style={{
              padding: "16px",
              backgroundColor: "var(--color-bg-subtle, rgba(0,0,0,0.02))",
              borderRadius: "8px",
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              justifyContent: "center",
              gap: "6px",
              textAlign: "center",
              flex: 1,
            }}
          >
            <Text strong style={{ fontSize: "13px" }}>
              OpenCode Direct
            </Text>
            <Text type="secondary" style={{ fontSize: "12px" }}>
              {t("workbench.noLocalProxyNeeded", { defaultValue: "无需本地代理" })}
            </Text>
          </div>
        ) : (
          <div
            style={{
              padding: "12px 14px",
              backgroundColor: "var(--color-bg-subtle, rgba(0,0,0,0.02))",
              borderRadius: "8px",
              display: "flex",
              flexDirection: "column",
              gap: "10px",
              flex: 1,
              justifyContent: "center",
            }}
          >
            {/* Route Pipeline visual */}
            <div style={{ display: "flex", alignItems: "center", gap: "8px", flexWrap: "wrap" }}>
              <Tag color="blue" style={{ margin: 0, fontWeight: 500 }}>
                {t(LABEL_KEYS[selectedTarget])}
              </Tag>
              <span style={{ color: "var(--color-text-tertiary)" }}>→</span>
              <span style={{ fontFamily: "monospace", fontSize: "12px", color: "var(--color-text-secondary)" }}>
                :{proxyStatus?.port ?? "—"}
              </span>
              <span style={{ color: "var(--color-text-tertiary)" }}>→</span>

              {officialCurrent ? (
                <StatusBadge status="current" label={t("providers.officialMode", { defaultValue: "官方模式" })} />
              ) : currentProvider ? (
                <Inline gap="xs" align="center">
                  <ProviderBrandIcon provider={currentProvider} size={16} />
                  <Text strong style={{ fontSize: "13px" }}>
                    {currentProvider.name}
                  </Text>
                </Inline>
              ) : (
                <Text type="secondary" style={{ fontSize: "12px" }}>
                  {t("proxy.noTarget", { defaultValue: "未指定" })}
                </Text>
              )}

              {currentProvider?.model && (
                <>
                  <span style={{ color: "var(--color-text-tertiary)" }}>→</span>
                  <Text code style={{ fontSize: "11px" }}>
                    {currentProvider.model}
                  </Text>
                </>
              )}
            </div>

            {/* Status & Latency Footer */}
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                paddingTop: "6px",
                borderTop: "1px dashed var(--color-border-subtle, rgba(0,0,0,0.08))",
                fontSize: "12px",
              }}
            >
              <Inline gap="xs" align="center">
                {currentProvider?.healthLatencyMs != null && (
                  <StatusBadge
                    status={currentProvider.healthStatus === "healthy" ? "healthy" : "error"}
                    label={`${currentProvider.healthLatencyMs}ms`}
                  />
                )}
                {currentProvider?.protocolType && (
                  <Text type="secondary" style={{ fontSize: "11px" }}>
                    {currentProvider.protocolType}
                  </Text>
                )}
              </Inline>

              <Text type="secondary" style={{ fontSize: "11px" }}>
                {proxyStatus?.running
                  ? t("proxy.running", { defaultValue: "代理运行中" })
                  : t("proxy.stopped", { defaultValue: "代理未启动" })}
              </Text>
            </div>
          </div>
        )}
      </Stack>
    </Surface>
  );
};
