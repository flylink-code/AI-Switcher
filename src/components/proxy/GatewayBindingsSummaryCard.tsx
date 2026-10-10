import { useMemo } from "react";
import { Button, Card, Space, Table, Tag, Tooltip, Typography } from "antd";
import ArrowRightOutlined from "@ant-design/icons/es/icons/ArrowRightOutlined";
import LinkOutlined from "@ant-design/icons/es/icons/LinkOutlined";
import LoadingOutlined from "@ant-design/icons/es/icons/LoadingOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useNavigatePage } from "@/lib/navigation";
import { usageSourceIcon } from "@/components/UsageSourceIcons";
import { LABEL_KEYS, PROVIDER_TARGET_OPTIONS } from "@/components/AgentTargetSwitcher";
import { OnboardingTip } from "@/components/OnboardingTip";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { filterUiAgents } from "@/lib/agentVisibility";
import { getAgentConnectionMode } from "@/services/providers";
import type { GatewayBinding, GatewayProfile, Provider, ProviderTarget } from "@/types/backend";

const { Text } = Typography;

const SHARED_PROFILE_ID = "gprof_shared";

export interface GatewayBindingsSummaryCardProps {
  bindings: GatewayBinding[];
  profiles: GatewayProfile[];
  upstreams?: Provider[];
  loading?: boolean;
}

interface SummaryRow {
  target: ProviderTarget;
  modeStatus: "loading" | "unknown" | "gateway" | "direct" | "external";
  profileId?: string;
  directUpstreamId?: string;
}

export function GatewayBindingsSummaryCard({
  bindings,
  profiles,
  upstreams = [],
  loading = false,
}: GatewayBindingsSummaryCardProps) {
  const { t } = useTranslation();
  const navigate = useNavigatePage();
  const visibleAgents = usePagePreferencesStore((state) => state.visibleAgents);

  const visibleTargets = useMemo(() => {
    const allowed = filterUiAgents(PROVIDER_TARGET_OPTIONS);
    return allowed.filter((target) => visibleAgents.includes(target));
  }, [visibleAgents]);

  const connectionModesQuery = useQuery({
    queryKey: ["agent-connection-modes", visibleTargets],
    queryFn: async () => {
      const entries = await Promise.all(
        visibleTargets.map(async (target) => {
          try {
            const mode = await getAgentConnectionMode(target);
            return [target, mode] as const;
          } catch {
            return [target, "error"] as const;
          }
        }),
      );
      const result: Partial<Record<ProviderTarget, string>> = {};
      for (const [target, mode] of entries) {
        if (mode) result[target] = mode;
      }
      return result;
    },
    staleTime: 5000,
  });

  const connectionModes = useMemo(
    () => connectionModesQuery.data ?? {},
    [connectionModesQuery.data],
  );

  const profileMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const p of profiles) {
      const label =
        p.id === SHARED_PROFILE_ID
          ? t("gateway.profileDefault", { defaultValue: p.name || "默认" })
          : p.name.trim() || p.id;
      map.set(p.id, label);
    }
    return map;
  }, [profiles, t]);

  const upstreamMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const u of upstreams) {
      map.set(u.id, u.name.trim() || u.baseUrl);
    }
    return map;
  }, [upstreams]);

  const rows: SummaryRow[] = useMemo(() => {
    return visibleTargets.map((target) => {
      const binding = bindings.find((item) => item.targetApp === target);
      const activeMode = connectionModes[target];

      if (connectionModesQuery.isLoading) {
        return {
          target,
          modeStatus: "loading",
        };
      }

      if (connectionModesQuery.isError || !activeMode || activeMode === "error") {
        return {
          target,
          modeStatus: "unknown",
        };
      }

      if (activeMode === "gateway") {
        return {
          target,
          modeStatus: "gateway",
          profileId: binding?.profileId || SHARED_PROFILE_ID,
        };
      }

      if (activeMode === "direct") {
        return {
          target,
          modeStatus: "direct",
          directUpstreamId: binding?.directUpstreamId,
        };
      }

      return {
        target,
        modeStatus: "external",
      };
    });
  }, [visibleTargets, bindings, connectionModes, connectionModesQuery.isLoading, connectionModesQuery.isError]);

  const gatewayCount = rows.filter((r) => r.modeStatus === "gateway").length;

  return (
    <Card
      size="small"
      title={
        <Space size={8}>
          <span>{t("gateway.bindAppsSummary", { defaultValue: "已接入应用摘要" })}</span>
          <Tag color="cyan">
            {t("gateway.gatewayBoundCount", {
              count: gatewayCount,
              defaultValue: `{{count}} 个网关接管`,
            })}
          </Tag>
        </Space>
      }
      extra={
        <Button
          type="link"
          size="small"
          icon={<LinkOutlined />}
          onClick={() => navigate("providers")}
        >
          <Space size={4}>
            <span>{t("gateway.manageAgentConnections", { defaultValue: "管理 Agent 连接" })}</span>
            <ArrowRightOutlined style={{ fontSize: 11 }} />
          </Space>
        </Button>
      }
    >
      <OnboardingTip
        tipKey="gateway_bind"
        message={t("gateway.bindAppsSummaryHint", {
          defaultValue:
            "Agent 连接模式（网关 / 直连 / 独立）由「供应商」页唯一管理，避免多入口配置冲突。网关运行中时，当前处于网关接管状态的应用会自动使用对应的路由档案。",
        })}
        style={{ marginBottom: 12 }}
      />
      <Table<SummaryRow>
        size="small"
        pagination={false}
        rowKey="target"
        loading={loading || connectionModesQuery.isLoading}
        dataSource={rows}
        columns={[
          {
            title: t("gateway.bindAgent", { defaultValue: "应用" }),
            dataIndex: "target",
            width: 180,
            render: (target: ProviderTarget) => (
              <Space size={8}>
                <span style={{ fontSize: 16, display: "inline-flex", alignItems: "center" }}>
                  {usageSourceIcon(target)}
                </span>
                <Text strong>{t(LABEL_KEYS[target] ?? `workspace.${target}`)}</Text>
              </Space>
            ),
          },
          {
            title: t("gateway.connectionMode", { defaultValue: "连接模式" }),
            dataIndex: "modeStatus",
            width: 160,
            render: (modeStatus: SummaryRow["modeStatus"]) => {
              switch (modeStatus) {
                case "loading":
                  return (
                    <Tag icon={<LoadingOutlined />}>
                      {t("gateway.detectingMode", { defaultValue: "检测中..." })}
                    </Tag>
                  );
                case "gateway":
                  return <Tag color="success">{t("gateway.modeGateway", { defaultValue: "智能网关" })}</Tag>;
                case "direct":
                  return <Tag color="blue">{t("gateway.modeDirect", { defaultValue: "直连上游" })}</Tag>;
                case "external":
                  return (
                    <Tag color="default">
                      {t("gateway.modeExternal", { defaultValue: "独立供应商 / 外部" })}
                    </Tag>
                  );
                default:
                  return (
                    <Tag color="default">
                      {t("gateway.unknownMode", { defaultValue: "未知连接" })}
                    </Tag>
                  );
              }
            },
          },
          {
            title: t("gateway.routeProfileOrUpstream", { defaultValue: "路由档案 / 直连目标" }),
            render: (_: unknown, row: SummaryRow) => {
              if (row.modeStatus === "gateway") {
                const profileLabel =
                  profileMap.get(row.profileId ?? SHARED_PROFILE_ID) ??
                  row.profileId ??
                  t("gateway.profileDefault", { defaultValue: "默认" });
                return (
                  <Space size={6}>
                    <Tag color="purple">{profileLabel}</Tag>
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      {row.profileId === SHARED_PROFILE_ID
                        ? t("gateway.sharedProfileHint", { defaultValue: "（默认档案）" })
                        : ""}
                    </Text>
                  </Space>
                );
              }
              if (row.modeStatus === "direct") {
                const upstreamName = row.directUpstreamId
                  ? upstreamMap.get(row.directUpstreamId) ?? row.directUpstreamId
                  : t("gateway.unspecifiedUpstream", { defaultValue: "未指定上游" });
                return (
                  <Tooltip title={row.directUpstreamId}>
                    <Text type="secondary">{upstreamName}</Text>
                  </Tooltip>
                );
              }
              if (row.modeStatus === "external") {
                return (
                  <Text type="secondary">
                    {t("gateway.notRoutedByGateway", { defaultValue: "未由智能网关接管" })}
                  </Text>
                );
              }
              return <Text type="secondary">—</Text>;
            },
          },
          {
            title: t("gateway.actions", { defaultValue: "操作" }),
            width: 100,
            align: "right",
            render: () => (
              <Button
                type="link"
                size="small"
                onClick={() => navigate("providers")}
              >
                {t("common.edit", { defaultValue: "配置" })}
              </Button>
            ),
          },
        ]}
      />
    </Card>
  );
}
