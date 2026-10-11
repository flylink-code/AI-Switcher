import { useMemo, useState, type ReactNode } from "react";
import {
  Alert,
  Button,
  Card,
  Radio,
  Select,
  Space,
  Tag,
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
  listGatewayUpstreams,
  listSmartGatewayBindings,
  setAgentDirect,
  switchToOfficial,
} from "@/services/providers";
import type {
  GatewayBinding,
  Provider,
  ProviderTarget,
} from "@/types/backend";
import { errMsg } from "@/lib/errMsg";
import { ExpandableSection } from "@/components/common/ExpandableSection";

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

function OpenAiOnClaudeGuide() {
  const { t } = useTranslation();
  return (
    <ExpandableSection title={t("providers.openaiOnClaudeTitle", { defaultValue: "OpenAI 协议上游（如 sub2api）怎么用" })}>
      <Text type="secondary" style={{ display: "block", fontSize: 12, lineHeight: 1.6 }}>
        {t("providers.openaiOnClaudeSteps", {
          defaultValue:
            "1. 在供应商 → 智能网关里新增上游，协议选 OpenAI Chat 或 OpenAI Responses，地址带 /v1，并填写模型名。2. 在本页把 Claude Code 设为「智能网关」，不要选直连。3. 重启 Claude Code，用 /model 从聚合目录里选这个模型。若该站点同时提供 Anthropic /v1/messages，可以再加一条 Anthropic 协议的上游，那时才可以直连。",
        })}
      </Text>
    </ExpandableSection>
  );
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


export interface AgentConnectionCardProps {
  /** When false, the agent is not installed: hide connection controls. */
  installed?: Partial<Record<ProviderTarget, boolean>>;
  renderHeaderExtra?: (target: ProviderTarget) => ReactNode;
  renderAdvanced?: (target: ProviderTarget) => ReactNode;
  framed?: boolean;
}

export function AgentConnectionCard({
  installed,
  renderHeaderExtra,
  renderAdvanced,
  framed = true,
}: AgentConnectionCardProps = {}) {
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

  const smartGatewayQuery = useQuery({
    queryKey: ["smart-gateway-status"],
    queryFn: getSmartGatewayStatus,
    refetchInterval: 5_000,
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
        setPendingDirect((prev) => ({ ...prev, [target]: true }));
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

  const rows = (
    <Space direction="vertical" size={12} style={{ width: "100%" }}>
      {dataSource.map((row) => {
        const ready = installed?.[row.target] !== false;
        const advanced = ready ? renderAdvanced?.(row.target) : null;
        const isRowBusy = Boolean(loadingTargets[row.target]);
        const isDirect = row.displayMode === "direct";
        const optionsForTarget = upstreams.map((upstream) => {
          const compat = checkUpstreamDirectCompatibility(row.target, upstream, t);
          const name = `${upstream.name} (${upstream.model || upstream.protocolType})`;
          return {
            value: upstream.id,
            label: compat.compatible || !compat.reason ? name : `${name} — ${compat.reason}`,
            disabled: !compat.compatible,
          };
        });
        const directGroups = [
          {
            label: t("providers.directCompatibleGroup", { defaultValue: "可直连" }),
            options: optionsForTarget.filter((option) => !option.disabled),
          },
          {
            label: t("providers.directGatewayGroup", { defaultValue: "需走智能网关" }),
            options: optionsForTarget.filter((option) => option.disabled),
          },
        ].filter((group) => group.options.length > 0);
        const hasCompatibleDirect = optionsForTarget.some((option) => !option.disabled);
        const selectedUpstreamId =
          (row.binding?.mode === "direct" ? row.binding.directUpstreamId : undefined) ||
          rememberedDirect[row.target] ||
          undefined;
        return (
          <Card key={row.target} size="small" className="page-surface">
            <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
              {usageSourceIcon(row.target, { size: 18 })}
              <Text strong>{t(LABEL_KEYS[row.target] ?? `workspace.${row.target}`)}</Text>
              <Tag style={{ margin: 0 }}>
                {row.isT1
                  ? t("providers.tierT1", { defaultValue: "T1 全功能" })
                  : t("providers.tierT2", { defaultValue: "T2 兼容" })}
              </Tag>
              <span style={{ marginLeft: "auto" }}>{renderHeaderExtra?.(row.target)}</span>
            </div>
            {ready ? (
              <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 10 }}>
                <Radio.Group
                  size="small"
                  optionType="button"
                  buttonStyle="solid"
                  disabled={isRowBusy}
                  value={row.displayMode}
                  onChange={(event) => void handleModeChange(row.target, event.target.value as AgentConnectionMode)}
                  options={[
                    { label: t("providers.modeGateway", { defaultValue: "智能网关" }), value: "gateway" },
                    { label: t("providers.modeDirect", { defaultValue: "直连" }), value: "direct" },
                    { label: t("providers.modeOfficial", { defaultValue: "官方" }), value: "official" },
                  ]}
                />
                {isDirect ? (
                  hasCompatibleDirect ? (
                    <Select
                      size="small"
                      style={{ maxWidth: 420 }}
                      disabled={isRowBusy}
                      value={selectedUpstreamId}
                      placeholder={t("providers.selectDirectUpstream", { defaultValue: "请选择直连上游..." })}
                      options={directGroups}
                      onChange={(value) => void handleSelectDirectUpstream(row.target, String(value))}
                    />
                  ) : (
                    <Alert
                      type="info"
                      showIcon
                      message={t("providers.noCompatibleUpstreamForDirect", {
                        agent: t(LABEL_KEYS[row.target] ?? `workspace.${row.target}`),
                        defaultValue: "没有可直连的上游。协议不匹配时请改用智能网关。",
                      })}
                      action={
                        <Button size="small" type="primary" disabled={isRowBusy} onClick={() => void handleModeChange(row.target, "gateway")}>
                          {t("providers.useSmartGateway", { defaultValue: "改用智能网关" })}
                        </Button>
                      }
                    />
                  )
                ) : row.displayMode === "gateway" ? (
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {!smartGateway
                      ? t("providers.statusGatewayUnknown", { defaultValue: "网关状态未知" })
                      : smartGateway.running
                        ? t("providers.statusGateway", {
                            port: smartGateway.port,
                            defaultValue: `已连接网关 (:${smartGateway.port})`,
                          })
                        : t("providers.statusGatewayStopped", {
                            port: smartGateway.port,
                            defaultValue: `网关未启动 (:${smartGateway.port})`,
                          })}
                    {" · "}
                    {t("providers.gatewayCatalogHint", { defaultValue: "使用聚合目录中的模型" })}
                  </Text>
                ) : null}
                {row.target === "claude_code" ? <OpenAiOnClaudeGuide /> : null}
                {row.isT1 ? <AgentConfigDrift target={row.target} /> : null}
                {advanced ? (
                  <ExpandableSection title={t("agentHub.advanced", { defaultValue: "高级设置" })}>
                    {advanced}
                  </ExpandableSection>
                ) : null}
              </div>
            ) : null}
          </Card>
        );
      })}
    </Space>
  );

  if (!framed) return rows;

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
      {rows}
    </Card>
  );
}
