import { useMemo, useState } from "react";
import {
  Card,
  Radio,
  Select,
  Space,
  Table,
  Tag,
  Tooltip,
  Typography,
  message,
} from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { AgentConfigDrift } from "@/components/AgentConfigDrift";
import { usageSourceIcon } from "@/components/UsageSourceIcons";
import { LABEL_KEYS, PROVIDER_TARGET_OPTIONS } from "@/components/AgentTargetSwitcher";
import { filterUiAgents } from "@/lib/agentVisibility";
import {
  bindSmartGateway,
  getAgentConnectionMode,
  getSmartGatewayStatus,
  listGatewayProfiles,
  listGatewayUpstreams,
  listSmartGatewayBindings,
  setAgentDirect,
  setGatewayBindingProfile,
  switchToOfficial,
} from "@/services/providers";
import type {
  GatewayBinding,
  Provider,
  ProviderTarget,
} from "@/types/backend";

const { Text } = Typography;

export type AgentConnectionMode = "gateway" | "direct" | "official";

interface AgentRow {
  target: ProviderTarget;
  isT1: boolean;
  binding?: GatewayBinding;
  currentMode: AgentConnectionMode;
  displayMode: AgentConnectionMode;
  isPendingDirect: boolean;
}

export function checkUpstreamDirectCompatibility(
  target: ProviderTarget,
  upstream: Provider,
  t?: (key: string, opts?: Record<string, unknown>) => string,
): { compatible: boolean; reason?: string } {
  // Codex OAuth 任何 target 必须走智能网关，不支持直连
  if (upstream.providerKind === "codex_oauth") {
    return {
      compatible: false,
      reason: t
        ? t("providers.codexOauthRequiresGateway", {
            defaultValue: "Codex OAuth 账号必须经由智能网关，不支持直连",
          })
        : "Codex OAuth 账号必须经由智能网关，不支持直连",
    };
  }

  // Claude Code 直连仅支持原生 Anthropic 协议
  if (target === "claude_code") {
    if (upstream.protocolType !== "anthropic") {
      return {
        compatible: false,
        reason: t
          ? t("providers.directAnthropicOnly", {
              defaultValue: "Claude Code 直连仅支持 Anthropic 协议；需协议转换请走智能网关",
            })
          : "Claude Code 直连仅支持 Anthropic 协议；需协议转换请走智能网关",
      };
    }
  }

  // Codex 直连仅支持 OpenAI Chat / Responses 协议
  if (target === "codex") {
    if (
      upstream.protocolType !== "openai_chat" &&
      upstream.protocolType !== "openai_responses"
    ) {
      return {
        compatible: false,
        reason: t
          ? t("providers.directOpenAiOnly", {
              defaultValue: "Codex 直连仅支持 OpenAI 协议；需协议转换请走智能网关",
            })
          : "Codex 直连仅支持 OpenAI 协议；需协议转换请走智能网关",
      };
    }
  }

  return { compatible: true };
}

function errMsg(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message;
  return String(error ?? "未知错误");
}

export function AgentConnectionCard() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [loadingTargets, setLoadingTargets] = useState<Partial<Record<ProviderTarget, boolean>>>({});
  const [rememberedDirect, setRememberedDirect] = useState<Partial<Record<ProviderTarget, string>>>({});
  const [pendingDirect, setPendingDirect] = useState<Partial<Record<ProviderTarget, boolean>>>({});

  const visibleTargets = useMemo(
    () => filterUiAgents(PROVIDER_TARGET_OPTIONS),
    [],
  );

  const bindingsQuery = useQuery({
    queryKey: ["smart-gateway-bindings"],
    queryFn: listSmartGatewayBindings,
  });

  const upstreamsQuery = useQuery({
    queryKey: ["gateway-upstreams"],
    queryFn: listGatewayUpstreams,
  });

  const profilesQuery = useQuery({
    queryKey: ["gateway-profiles"],
    queryFn: listGatewayProfiles,
  });

  const smartGatewayQuery = useQuery({
    queryKey: ["smart-gateway-status"],
    queryFn: getSmartGatewayStatus,
    refetchInterval: 10_000,
  });
  const smartGateway = smartGatewayQuery.data;

  const connectionModesQuery = useQuery({
    queryKey: ["agent-connection-modes"],
    queryFn: async () => {
      const entries = await Promise.all(
        visibleTargets.map(async (target) => {
          const mode = await getAgentConnectionMode(target);
          return [target, mode] as const;
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

  const bindings = useMemo(
    () => bindingsQuery.data ?? [],
    [bindingsQuery.data],
  );
  const upstreams = useMemo(() => upstreamsQuery.data ?? [], [upstreamsQuery.data]);
  const profiles = useMemo(() => profilesQuery.data ?? [], [profilesQuery.data]);

  const profileOptions = useMemo(() => {
    return profiles.map((p) => ({
      value: p.id,
      label:
        p.id === "gprof_shared"
          ? t("gateway.profileDefault", { defaultValue: p.name || "默认档案" })
          : p.name.trim() || p.id,
    }));
  }, [profiles, t]);

  const invalidateAgentQueries = async () => {
    await queryClient.invalidateQueries({ queryKey: ["agent-config-drift"] });
    await queryClient.invalidateQueries({ queryKey: ["smart-gateway-bindings"] });
    await queryClient.invalidateQueries({ queryKey: ["smart-gateway-status"] });
    await queryClient.invalidateQueries({ queryKey: ["agent-connection-modes"] });
    await queryClient.invalidateQueries({ queryKey: ["providers"] });
  };

  const handleModeChange = async (target: ProviderTarget, nextMode: AgentConnectionMode) => {
    const agentLabel = t(LABEL_KEYS[target] ?? `workspace.${target}`);

    if (nextMode === "direct") {
      const binding = bindings.find((b) => b.targetApp === target);
      const existingDirectId =
        (binding?.mode === "direct" ? binding.directUpstreamId : "") || rememberedDirect[target];

      const validUpstreams = upstreams.filter(
        (u) => checkUpstreamDirectCompatibility(target, u, t).compatible,
      );

      if (validUpstreams.length === 0) {
        void message.warning(
          t("providers.noCompatibleUpstreamForDirect", {
            agent: agentLabel,
            defaultValue: `没有适用于 ${agentLabel} 的兼容直连上游，如需协议转换请切换至智能网关`,
          }),
        );
        return;
      }

      if (existingDirectId && validUpstreams.some((u) => u.id === existingDirectId)) {
        // 已有 direct 可记住：直接恢复该上游
        setLoadingTargets((prev) => ({ ...prev, [target]: true }));
        try {
          await setAgentDirect(target, existingDirectId);
          setRememberedDirect((prev) => ({ ...prev, [target]: existingDirectId }));
          setPendingDirect((prev) => ({ ...prev, [target]: false }));
          await invalidateAgentQueries();
          void message.success(
            t("providers.modeSwitchedDirect", {
              agent: agentLabel,
              defaultValue: `${agentLabel} 已切换为直连上游`,
            }),
          );
        } catch (error) {
          void message.error(errMsg(error));
        } finally {
          setLoadingTargets((prev) => ({ ...prev, [target]: false }));
        }
      } else {
        // direct 单击不悄悄选第一条上游：显示可选 Select 待用户选完才 setAgentDirect
        setPendingDirect((prev) => ({ ...prev, [target]: true }));
        void message.info(
          t("providers.selectDirectUpstreamPrompt", {
            agent: agentLabel,
            defaultValue: `请为 ${agentLabel} 选择直连上游`,
          }),
        );
      }
      return;
    }

    // 切换到 gateway 或 official 时清理待选状态
    setPendingDirect((prev) => ({ ...prev, [target]: false }));
    setLoadingTargets((prev) => ({ ...prev, [target]: true }));
    try {
      if (nextMode === "gateway") {
        await bindSmartGateway(target);
        await invalidateAgentQueries();
        void message.success(
          t("providers.modeSwitchedGateway", {
            agent: agentLabel,
            defaultValue: `${agentLabel} 已连接至智能网关`,
          }),
        );
      } else if (nextMode === "official") {
        // 官方切换用 switchToOfficial，不要隐式绑定；后端已删除绑定并清 T2 托管目录
        await switchToOfficial(target);
        await invalidateAgentQueries();
        void message.success(
          t("providers.modeSwitchedOfficial", {
            agent: agentLabel,
            defaultValue: `${agentLabel} 已切换为官方原生凭据`,
          }),
        );
      }
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setLoadingTargets((prev) => ({ ...prev, [target]: false }));
    }
  };

  const handleSelectDirectUpstream = async (target: ProviderTarget, upstreamId: string) => {
    setLoadingTargets((prev) => ({ ...prev, [target]: true }));
    try {
      await setAgentDirect(target, upstreamId);
      setRememberedDirect((prev) => ({ ...prev, [target]: upstreamId }));
      setPendingDirect((prev) => ({ ...prev, [target]: false }));
      await invalidateAgentQueries();
      void message.success(t("providers.directUpstreamSaved", { defaultValue: "直连上游已更新" }));
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setLoadingTargets((prev) => ({ ...prev, [target]: false }));
    }
  };

  const handleSelectProfile = async (target: ProviderTarget, profileId: string) => {
    setLoadingTargets((prev) => ({ ...prev, [target]: true }));
    try {
      // 选档案不得隐式绑定：set_binding_profile 只更新已有 gateway_bindings 行
      await setGatewayBindingProfile(target, profileId);
      await invalidateAgentQueries();
      void message.success(
        t("providers.gatewayProfileSaved", { defaultValue: "已切换智能网关档案" }),
      );
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setLoadingTargets((prev) => ({ ...prev, [target]: false }));
    }
  };

  const dataSource: AgentRow[] = useMemo(() => {
    return visibleTargets.map((target) => {
      const binding = bindings.find((b) => b.targetApp === target);
      const isT1 = target === "claude_code" || target === "codex";

      // getAgentConnectionMode 返回 external 而非 official，external 明确映射 official
      let currentMode: AgentConnectionMode = "official";
      const serverReportedMode = connectionModesQuery.data?.[target];
      if (serverReportedMode === "gateway") {
        currentMode = "gateway";
      } else if (serverReportedMode === "direct") {
        currentMode = "direct";
      } else if (serverReportedMode === "external") {
        currentMode = "official";
      } else if (binding) {
        currentMode = binding.mode === "direct" ? "direct" : "gateway";
      }

      const isPendingDirect = Boolean(pendingDirect[target]);
      const displayMode: AgentConnectionMode = isPendingDirect ? "direct" : currentMode;

      return {
        target,
        isT1,
        binding,
        currentMode,
        displayMode,
        isPendingDirect,
      };
    });
  }, [visibleTargets, bindings, connectionModesQuery.data, pendingDirect]);

  return (
    <Card
      size="small"
      className="page-surface"
      title={t("providers.agentConnectionsTitle", { defaultValue: "Agent 连接与路由" })}
    >
      <Text type="secondary" style={{ display: "block", marginBottom: 12, fontSize: 12 }}>
        {t("providers.agentConnectionsHint", {
          defaultValue:
            "统一配置各 Agent 的连接方式。推荐默认连接智能网关以享受多模型与动态路由，也可指定单个全局上游进行直连，或使用官方原生凭据。",
        })}
      </Text>

      <Table<AgentRow>
        size="small"
        rowKey="target"
        pagination={false}
        loading={bindingsQuery.isLoading}
        dataSource={dataSource}
        columns={[
          {
            title: t("providers.agentName", { defaultValue: "Agent" }),
            dataIndex: "target",
            width: 180,
            render: (target: ProviderTarget, row: AgentRow) => (
              <Space align="center" size={8}>
                <div style={{ width: 24, height: 24, display: "flex", alignItems: "center", justifyContent: "center" }}>
                  {usageSourceIcon(target, { size: 20 })}
                </div>
                <Space direction="vertical" size={0}>
                  <Text strong style={{ fontSize: 13 }}>
                    {t(LABEL_KEYS[target] ?? `workspace.${target}`)}
                  </Text>
                  <Tag
                    color={row.isT1 ? "blue" : "default"}
                    style={{ fontSize: 10, lineHeight: "16px", paddingInline: 4, margin: 0 }}
                  >
                    {row.isT1
                      ? t("providers.tierT1", { defaultValue: "T1 全功能" })
                      : t("providers.tierT2", { defaultValue: "T2 兼容" })}
                  </Tag>
                </Space>
              </Space>
            ),
          },
          {
            title: t("providers.connectionMode", { defaultValue: "连接模式" }),
            dataIndex: "displayMode",
            width: 250,
            render: (mode: AgentConnectionMode, row: AgentRow) => {
              const isRowBusy = Boolean(loadingTargets[row.target]);
              return (
                <Radio.Group
                  size="small"
                  optionType="button"
                  buttonStyle="solid"
                  disabled={isRowBusy}
                  value={mode}
                  onChange={(e) => void handleModeChange(row.target, e.target.value as AgentConnectionMode)}
                  options={[
                    {
                      label: t("providers.modeGateway", { defaultValue: "智能网关" }),
                      value: "gateway",
                    },
                    {
                      label: t("providers.modeDirect", { defaultValue: "直连" }),
                      value: "direct",
                    },
                    {
                      label: t("providers.modeOfficial", { defaultValue: "官方" }),
                      value: "official",
                    },
                  ]}
                />
              );
            },
          },
          {
            title: t("providers.directUpstream", { defaultValue: "直连上游" }),
            dataIndex: "binding",
            width: 260,
            render: (_: unknown, row: AgentRow) => {
              const isDirect = row.displayMode === "direct";
              const isRowBusy = Boolean(loadingTargets[row.target]);
              const optionsForTarget = upstreams.map((u) => {
                const compat = checkUpstreamDirectCompatibility(row.target, u, t);
                return {
                  value: u.id,
                  label: `${u.name} (${u.model || u.protocolType})`,
                  disabled: !compat.compatible,
                  reason: compat.reason,
                };
              });

              const selectedUpstreamId =
                (row.binding?.mode === "direct" ? row.binding.directUpstreamId : undefined) ||
                rememberedDirect[row.target] ||
                undefined;

              return (
                <Tooltip
                  title={
                    !isDirect
                      ? t("providers.directDisabledHint", { defaultValue: "仅在直连模式下可选择上游" })
                      : undefined
                  }
                >
                  <Select
                    size="small"
                    style={{ width: "100%" }}
                    disabled={!isDirect || isRowBusy}
                    value={isDirect ? selectedUpstreamId : undefined}
                    placeholder={
                      isDirect
                        ? t("providers.selectDirectUpstream", { defaultValue: "请选择直连上游..." })
                        : "—"
                    }
                    options={optionsForTarget}
                    optionRender={(option) => (
                      <Space style={{ width: "100%", justifyContent: "space-between" }}>
                        <span>{option.data.label}</span>
                        {option.data.reason ? (
                          <Text type="secondary" style={{ fontSize: 11 }}>
                            ({option.data.reason})
                          </Text>
                        ) : null}
                      </Space>
                    )}
                    onChange={(val) => void handleSelectDirectUpstream(row.target, String(val))}
                  />
                </Tooltip>
              );
            },
          },
          {
            title: t("providers.gatewayProfile", { defaultValue: "网关档案" }),
            dataIndex: "binding",
            width: 200,
            render: (binding: GatewayBinding | undefined, row: AgentRow) => {
              // 档案Select仅gateway已有绑定可选，直连/未绑disabled
              const isGatewayBound = row.displayMode === "gateway" && Boolean(binding);
              const isRowBusy = Boolean(loadingTargets[row.target]);
              return (
                <Tooltip
                  title={
                    !isGatewayBound
                      ? t("providers.profileDisabledHint", {
                          defaultValue: "仅在连接智能网关时可选档案",
                        })
                      : undefined
                  }
                >
                  <Select
                    size="small"
                    style={{ width: "100%" }}
                    disabled={!isGatewayBound || isRowBusy}
                    value={isGatewayBound ? (binding?.profileId || "gprof_shared") : undefined}
                    placeholder={
                      isGatewayBound
                        ? t("gateway.profileDefault", { defaultValue: "默认档案" })
                        : "—"
                    }
                    options={profileOptions}
                    onChange={(val) => void handleSelectProfile(row.target, String(val))}
                  />
                </Tooltip>
              );
            },
          },
          {
            title: t("configDrift.column"),
            width: 190,
            render: (_: unknown, row: AgentRow) => row.isT1 ? <AgentConfigDrift target={row.target} /> : "—",
          },
          {
            title: t("providers.status", { defaultValue: "当前状态" }),
            width: 180,
            render: (_: unknown, row: AgentRow) => {
              if (row.isPendingDirect) {
                return (
                  <Tag color="warning" style={{ margin: 0 }}>
                    {t("providers.statusPendingDirect", { defaultValue: "待选择上游" })}
                  </Tag>
                );
              }
              if (row.displayMode === "gateway") {
                if (!smartGatewayQuery.isSuccess || !smartGateway) {
                  return <Tag>{t("providers.statusGatewayUnknown", { defaultValue: "网关状态未知" })}</Tag>;
                }
                const port = smartGateway.port;
                const isGatewayRunning = smartGateway.running;
                if (!isGatewayRunning && smartGatewayQuery.isSuccess) {
                  return (
                    <Tooltip
                      title={t("providers.gatewayStoppedHint", {
                        defaultValue: "智能网关未运行，请求将无法处理，请前往网关页启动",
                      })}
                    >
                      <Tag color="warning" style={{ margin: 0 }}>
                        {t("providers.statusGatewayStopped", {
                          port,
                          defaultValue: `网关未启动 (:${port})`,
                        })}
                      </Tag>
                    </Tooltip>
                  );
                }
                return (
                  <Tag color="blue" style={{ margin: 0 }}>
                    {t("providers.statusGateway", {
                      port,
                      defaultValue: `已连接网关 (:${port})`,
                    })}
                  </Tag>
                );
              }
              if (row.displayMode === "direct") {
                const directId =
                  (row.binding?.mode === "direct" ? row.binding.directUpstreamId : undefined) ||
                  rememberedDirect[row.target];
                const matchedUpstream = upstreams.find((u) => u.id === directId);
                return (
                  <Tag color="cyan" style={{ margin: 0 }}>
                    {t("providers.statusDirect", { defaultValue: "直连: " })}
                    {matchedUpstream ? matchedUpstream.name : (directId || "—")}
                  </Tag>
                );
              }
              return (
                <Tag color="default" style={{ margin: 0 }}>
                  {t("providers.statusOfficial", { defaultValue: "官方原生" })}
                </Tag>
              );
            },
          },
        ]}
      />
    </Card>
  );
}
