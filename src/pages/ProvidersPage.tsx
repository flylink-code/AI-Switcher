import { useState } from "react";
import { errMsg } from "@/lib/errMsg";
import {
  Alert,
  Badge,
  Button,
  Card,
  Collapse,
  Select,
  Space,
  Switch,
  Tag,
  Typography,
  message,
} from "antd";
import NodeIndexOutlined from "@ant-design/icons/es/icons/NodeIndexOutlined";
import SettingOutlined from "@ant-design/icons/es/icons/SettingOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { ClaudeCodeAgentSettings } from "@/types/backend";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { AgentConnectionCard } from "@/components/AgentConnectionCard";
import { GatewayUpstreamPanel } from "@/components/proxy/GatewayUpstreamPanel";
import { useNavigatePage } from "@/lib/navigation";
import {
  getAntigravityGatewayStatus,
  getKiroGatewayStatus,
  getSmartGatewayStatus,
  getClaudeCodeDefaultPermissionMode,
  getClaudeCodeAgentSettings,
  getOpenCodePermissionMode,
  getPiSettings,
  readLivePrompt,
  setClaudeCodeDefaultPermissionMode,
  setClaudeCodeAgentSettings,
  setOpenCodePermissionMode,
  updatePiSettings,
} from "@/services/api";

const { Text, Title } = Typography;

function livePromptBlocksAgentTeams(content: string | undefined | null): boolean {
  if (!content) return false;
  return /不创建\s*Agent Teams|不使用多级代理编排|do not create Agent Teams/i.test(content);
}


export default function ProvidersPage() {
  const { t } = useTranslation();
  const navigate = useNavigatePage();
  const setWorkspaceTarget = usePagePreferencesStore((state) => state.setWorkspaceTarget);

  const [piThinkingLevel, setPiThinkingLevel] = useState<string>("medium");

  const smartGatewayQuery = useQuery({
    queryKey: ["smart-gateway-status"],
    queryFn: getSmartGatewayStatus,
    refetchInterval: 10_000,
  });

  const antigravityQuery = useQuery({
    queryKey: ["antigravity-gateway"],
    queryFn: getAntigravityGatewayStatus,
    refetchInterval: 10_000,
  });

  const kiroQuery = useQuery({
    queryKey: ["kiro-gateway"],
    queryFn: getKiroGatewayStatus,
    refetchInterval: 10_000,
  });

  const defaultPermissionModeQuery = useQuery({
    queryKey: ["claude-code-default-permission-mode"],
    queryFn: getClaudeCodeDefaultPermissionMode,
  });

  const agentSettingsQuery = useQuery({
    queryKey: ["claude-code-agent-settings"],
    queryFn: getClaudeCodeAgentSettings,
  });

  const opencodePermissionQuery = useQuery({
    queryKey: ["opencode-permission-mode"],
    queryFn: getOpenCodePermissionMode,
  });

  const piSettingsQuery = useQuery({
    queryKey: ["pi-settings"],
    queryFn: async () => {
      const res = await getPiSettings();
      if (typeof res.defaultThinkingLevel === "string") {
        setPiThinkingLevel(res.defaultThinkingLevel);
      }
      return res;
    },
  });

  const livePromptQuery = useQuery({
    queryKey: ["claude-code-live-prompt"],
    queryFn: () => readLivePrompt("claude_code"),
  });

  const smartGateway = smartGatewayQuery.data;
  const antigravity = antigravityQuery.data;
  const kiro = kiroQuery.data;

  const handleDefaultPermissionModeChange = async (mode: string) => {
    try {
      await setClaudeCodeDefaultPermissionMode(mode);
      await defaultPermissionModeQuery.refetch();
      void message.success(t("providers.defaultPermissionModeSaved"));
    } catch (error) {
      void message.error(errMsg(error));
    }
  };

  const handleAgentSettingsChange = async (
    patch: Partial<ClaudeCodeAgentSettings>,
    savedKey = "providers.agentTeamsSaved",
  ) => {
    const current = agentSettingsQuery.data;
    if (!current) return;
    const next: ClaudeCodeAgentSettings = { ...current, ...patch };
    if (!next.tmuxSupported && next.teammateMode === "tmux") {
      next.teammateMode = "in-process";
    }
    try {
      await setClaudeCodeAgentSettings(next);
      await agentSettingsQuery.refetch();
      void message.success(t(savedKey));
    } catch (error) {
      void message.error(errMsg(error));
    }
  };

  const handleOpenCodePermissionChange = async (allow: boolean) => {
    try {
      await setOpenCodePermissionMode(allow ? "allow" : "ask");
      await opencodePermissionQuery.refetch();
      void message.success(t("providers.opencodePermissionSaved"));
    } catch (error) {
      void message.error(errMsg(error));
    }
  };

  const handleUpdatePiThinkingLevel = async (level: string) => {
    setPiThinkingLevel(level);
    try {
      await updatePiSettings(null, null, level);
      void message.success(
        t("providers.piThinkingLevelUpdated", {
          defaultValue: `已设置 Pi 思考强度为: ${level}`,
        }),
      );
      void piSettingsQuery.refetch();
    } catch (error) {
      void message.error(errMsg(error));
    }
  };

  const openWorkspacePrompts = () => {
    setWorkspaceTarget("claude_code");
    if (typeof localStorage !== "undefined") {
      localStorage.setItem("cs.workspaceTab", "prompts");
    }
    navigate("workspace");
  };

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%", minWidth: 0 }}>
      {/* Top Header: Title, Runtime status badges, Quick Navigation */}
      <div
        className="cc-workbench-header"
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          flexWrap: "wrap",
          gap: 12,
        }}
      >
        <Space align="center" size={10} wrap>
          <Title level={4} style={{ margin: 0 }}>
            {t("providers.pageTitle", { defaultValue: "统一供应商与接入" })}
          </Title>
          <Tag
            color={smartGateway?.running ? "blue" : undefined}
            style={{ cursor: "pointer", margin: 0 }}
            onClick={() => navigate("gateway")}
          >
            <Badge status={smartGateway?.running ? "processing" : "default"} style={{ marginRight: 6 }} />
            {smartGateway?.running
              ? t("providers.gatewayRunningTag", {
                  port: smartGateway.port ?? 15828,
                  defaultValue: `智能网关 :${smartGateway.port ?? 15828}`,
                })
              : t("providers.gatewayStoppedTag", { defaultValue: "智能网关未启动" })}
          </Tag>
          <Tag
            color={antigravity?.running ? "purple" : undefined}
            style={{ cursor: "pointer", margin: 0 }}
            onClick={() => navigate("gateway")}
          >
            {antigravity?.running
              ? t("workbench.antigravityRunning", {
                  port: antigravity.port,
                  defaultValue: `反代网关 :${antigravity.port}`,
                })
              : t("workbench.antigravityStopped", { defaultValue: "反代网关未运行" })}
          </Tag>
          <Tag
            color={kiro?.running ? "green" : undefined}
            style={{ cursor: "pointer", margin: 0 }}
            onClick={() => navigate("gateway")}
          >
            {kiro?.running
              ? t("gateway.kiroRunning", {
                  port: kiro.port,
                  defaultValue: `Kiro :${kiro.port}`,
                })
              : t("gateway.kiroStopped", { defaultValue: "Kiro 未运行" })}
          </Tag>
        </Space>
        <Space wrap>
          <Button icon={<NodeIndexOutlined />} onClick={() => navigate("gateway")}>
            {t("providers.openGateway", { defaultValue: "智能网关配置" })}
          </Button>
        </Space>
      </div>

      {/* 1. Unified Agent Connection Card (T1/T2, filterUiAgents, no Desktop/DSH) */}
      <AgentConnectionCard />

      {/* 2. Global Upstream Pool Management (CRUD, Presets, Models, Quota, OAuth, Import/Export) */}
      <GatewayUpstreamPanel allowlistTarget="claude_code" />

      {/* 3. Collapsible Agent Runtime & Permission Settings */}
      <Collapse
        ghost
        items={[
          {
            key: "agentSettings",
            label: (
              <Space size={6}>
                <SettingOutlined />
                <span>{t("providers.agentSpecificSettings", { defaultValue: "Agent 运行与权限设置" })}</span>
              </Space>
            ),
            children: (
              <Space direction="vertical" size={12} style={{ width: "100%" }}>
                <Card size="small" title="Claude Code" className="page-surface">
                  <Space direction="vertical" size={12} style={{ width: "100%" }}>
                    <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
                      <Space direction="vertical" size={0} style={{ minWidth: 0, flex: 1 }}>
                        <strong>{t("providers.defaultPermissionModeTitle")}</strong>
                        <Text type="secondary" style={{ fontSize: 12 }}>
                          {t("providers.defaultPermissionModeHint")}
                        </Text>
                      </Space>
                      <Select
                        value={defaultPermissionModeQuery.data ?? "default"}
                        loading={defaultPermissionModeQuery.isLoading}
                        disabled={defaultPermissionModeQuery.isFetching}
                        style={{ minWidth: 200 }}
                        onChange={(value) => void handleDefaultPermissionModeChange(String(value))}
                        options={[
                          { value: "default", label: t("providers.defaultPermissionModeDefault") },
                          { value: "plan", label: t("providers.defaultPermissionModePlan") },
                          { value: "acceptEdits", label: t("providers.defaultPermissionModeAcceptEdits") },
                          { value: "auto", label: t("providers.defaultPermissionModeAuto") },
                        ]}
                      />
                    </Space>
                    <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
                      <Space direction="vertical" size={0} style={{ minWidth: 0, flex: 1 }}>
                        <span>{t("providers.agentTeamsEnable")}</span>
                        <Text type="secondary" style={{ fontSize: 12 }}>
                          {t("providers.agentTeamsEnableHint")}
                        </Text>
                      </Space>
                      <Switch
                        checked={agentSettingsQuery.data?.teamsEnabled ?? false}
                        loading={agentSettingsQuery.isLoading || agentSettingsQuery.isFetching}
                        onChange={(checked) => void handleAgentSettingsChange({ teamsEnabled: checked })}
                      />
                    </Space>
                    {agentSettingsQuery.data?.teamsEnabled &&
                      livePromptBlocksAgentTeams(livePromptQuery.data?.content) && (
                        <Alert
                          type="warning"
                          showIcon
                          message={t("providers.agentTeamsPromptConflict")}
                          action={
                            <Button size="small" onClick={openWorkspacePrompts}>
                              {t("providers.agentTeamsOpenPrompts")}
                            </Button>
                          }
                        />
                      )}
                    <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
                      <Space direction="vertical" size={0} style={{ minWidth: 0, flex: 1 }}>
                        <span>{t("providers.autoModeServer")}</span>
                        <Text type="secondary" style={{ fontSize: 12 }}>
                          {t("providers.autoModeServerHint")}
                        </Text>
                      </Space>
                      <Switch
                        checked={agentSettingsQuery.data?.autoModeServer ?? true}
                        loading={agentSettingsQuery.isLoading || agentSettingsQuery.isFetching}
                        onChange={(checked) =>
                          void handleAgentSettingsChange(
                            { autoModeServer: checked },
                            "providers.autoModeServerSaved",
                          )
                        }
                      />
                    </Space>
                    <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
                      <span>{t("providers.agentTeamsMode")}</span>
                      <Select
                        value={
                          agentSettingsQuery.data?.teammateMode === "tmux" &&
                          !agentSettingsQuery.data.tmuxSupported
                            ? "in-process"
                            : (agentSettingsQuery.data?.teammateMode ?? "auto")
                        }
                        loading={agentSettingsQuery.isLoading}
                        disabled={!agentSettingsQuery.data?.teamsEnabled || agentSettingsQuery.isFetching}
                        style={{ minWidth: 280 }}
                        onChange={(value) => void handleAgentSettingsChange({ teammateMode: String(value) })}
                        options={[
                          { value: "auto", label: t("providers.agentTeamsModeAuto") },
                          { value: "in-process", label: t("providers.agentTeamsModeInProcess") },
                          {
                            value: "tmux",
                            label: t("providers.agentTeamsModeTmux"),
                            disabled: !agentSettingsQuery.data?.tmuxSupported,
                          },
                        ]}
                      />
                    </Space>
                    {!agentSettingsQuery.data?.tmuxSupported && (
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {t("providers.agentTeamsModeTmuxDisabled")}
                      </Text>
                    )}
                  </Space>
                </Card>

                <Card size="small" title="OpenCode" className="page-surface">
                  <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
                    <Space direction="vertical" size={0} style={{ minWidth: 0, flex: 1 }}>
                      <strong>{t("providers.opencodePermissionTitle")}</strong>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {t("providers.opencodePermissionHint")}
                      </Text>
                    </Space>
                    <Switch
                      checked={opencodePermissionQuery.data === "allow"}
                      loading={opencodePermissionQuery.isLoading || opencodePermissionQuery.isFetching}
                      onChange={(checked) => void handleOpenCodePermissionChange(checked)}
                    />
                  </Space>
                </Card>

                <Card size="small" title="Pi" className="page-surface">
                  <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
                    <Space direction="vertical" size={0} style={{ minWidth: 0, flex: 1 }}>
                      <strong>
                        {t("providers.piThinkingLevelTitle", {
                          defaultValue: "Pi 默认思考强度 (Thinking Level)",
                        })}
                      </strong>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {t("providers.piThinkingLevelHint", {
                          defaultValue: "控制 Pi 模型 Reasoning/Thinking 思考过程",
                        })}
                      </Text>
                    </Space>
                    <Select
                      style={{ minWidth: 160 }}
                      value={piThinkingLevel}
                      onChange={(val) => void handleUpdatePiThinkingLevel(String(val))}
                      options={[
                        { label: "关闭 (off)", value: "off" },
                        { label: "极低 (minimal)", value: "minimal" },
                        { label: "低 (low)", value: "low" },
                        { label: "中 (medium)", value: "medium" },
                        { label: "高 (high)", value: "high" },
                        { label: "超高 (xhigh)", value: "xhigh" },
                        { label: "最大 (max)", value: "max" },
                      ]}
                    />
                  </Space>
                </Card>
              </Space>
            ),
          },
        ]}
      />
    </Space>
  );
}
