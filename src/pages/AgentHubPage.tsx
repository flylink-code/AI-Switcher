import { useState, type ReactNode } from "react";
import { Alert, Button, Select, Space, Switch, Typography, message } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { AgentConnectionCard } from "@/components/AgentConnectionCard";
import {
  claudeDesktopAppOptions,
  claudeVersionOptions,
  codexCliVersionOptions,
  localClaudeVersionOptions,
  localCodexCliVersionOptions,
  localOpenCodeCliVersionOptions,
  localPiCliVersionOptions,
  nodeRuntimeStatusOptions,
  opencodeCliVersionOptions,
  piCliVersionOptions,
} from "@/lib/appQueries";
import { openUrl } from "@tauri-apps/plugin-opener";
import { errMsg } from "@/lib/errMsg";
import { useNavigatePage } from "@/lib/navigation";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { ClaudeCodeAgentSettings, ProviderTarget } from "@/types/backend";
import {
  ensureNodeRuntimeViaFnm,
  getClaudeCodeAgentSettings,
  getClaudeCodeDefaultPermissionMode,
  getOpenCodePermissionMode,
  getPiSettings,
  readLivePrompt,
  runClaudeCodeUpdate,
  runClaudeDesktopAppUpdate,
  runCodexCliUpdate,
  runOpenCodeCliUpdate,
  runPiCliUpdate,
  setClaudeCodeAgentSettings,
  setClaudeCodeDefaultPermissionMode,
  setOpenCodePermissionMode,
  updatePiSettings,
} from "@/services/api";

const { Text } = Typography;

function livePromptBlocksAgentTeams(content: string | undefined | null): boolean {
  if (!content) return false;
  return /不创建\s*Agent Teams|不使用多级代理编排|do not create Agent Teams/i.test(content);
}

type ToolTarget = "claude_code" | "codex" | "opencode" | "pi";
type BusyTarget = ToolTarget | "claude_desktop";

export default function AgentHubPage() {
  const { t } = useTranslation();
  const navigate = useNavigatePage();
  const queryClient = useQueryClient();
  const setWorkspaceTarget = usePagePreferencesStore((state) => state.setWorkspaceTarget);
  const [piThinkingLevel, setPiThinkingLevel] = useState("medium");
  const [busyTarget, setBusyTarget] = useState<BusyTarget | null>(null);
  const [installingNode, setInstallingNode] = useState(false);

  const nodeQuery = useQuery(nodeRuntimeStatusOptions);
  const claudeQuery = useQuery({ ...claudeVersionOptions, placeholderData: () => queryClient.getQueryData(localClaudeVersionOptions.queryKey) });
  const localClaudeQuery = useQuery(localClaudeVersionOptions);
  const codexQuery = useQuery({ ...codexCliVersionOptions, placeholderData: () => queryClient.getQueryData(localCodexCliVersionOptions.queryKey) });
  const localCodexQuery = useQuery(localCodexCliVersionOptions);
  const opencodeQuery = useQuery({ ...opencodeCliVersionOptions, placeholderData: () => queryClient.getQueryData(localOpenCodeCliVersionOptions.queryKey) });
  const localOpenCodeQuery = useQuery(localOpenCodeCliVersionOptions);
  const piQuery = useQuery({ ...piCliVersionOptions, placeholderData: () => queryClient.getQueryData(localPiCliVersionOptions.queryKey) });
  const localPiQuery = useQuery(localPiCliVersionOptions);
  const desktopQuery = useQuery(claudeDesktopAppOptions);

  const claudeInfo = claudeQuery.data ?? localClaudeQuery.data;
  const codexInfo = codexQuery.data ?? localCodexQuery.data;
  const opencodeInfo = opencodeQuery.data ?? localOpenCodeQuery.data;
  const piInfo = piQuery.data ?? localPiQuery.data;
  const desktopInfo = desktopQuery.data;

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
      const result = await getPiSettings();
      if (typeof result.defaultThinkingLevel === "string") {
        setPiThinkingLevel(result.defaultThinkingLevel);
      }
      return result;
    },
  });
  const livePromptQuery = useQuery({
    queryKey: ["claude-code-live-prompt"],
    queryFn: () => readLivePrompt("claude_code"),
  });

  const toolInfo = (target: ProviderTarget) => {
    switch (target) {
      case "claude_code":
        return claudeInfo;
      case "codex":
        return codexInfo;
      case "opencode":
        return opencodeInfo;
      case "pi":
        return piInfo;
      case "claude_desktop":
        return desktopInfo;
      case "cline":
      case "dsh":
        return undefined;
      default: {
        const _exhaustive: never = target;
        return _exhaustive;
      }
    }
  };

  const refreshTool = async (target: ToolTarget) => {
    const key =
      target === "claude_code"
        ? "claude-code-version"
        : target === "codex"
          ? "codex-cli-version"
          : target === "opencode"
            ? "opencode-cli-version"
            : "pi-cli-version";
    await queryClient.invalidateQueries({ queryKey: [key] });
  };

  const runToolUpdate = async (target: ToolTarget) => {
    if (!nodeQuery.data?.meetsMinimum) {
      void message.warning(t("about.cliInstallNeedNode", { defaultValue: "需要 Node.js 22 或更高版本" }));
      return;
    }
    setBusyTarget(target);
    try {
      const result =
        target === "claude_code"
          ? await runClaudeCodeUpdate()
          : target === "codex"
            ? await runCodexCliUpdate()
            : target === "opencode"
              ? await runOpenCodeCliUpdate("v2")
              : await runPiCliUpdate();
      void message.success(result);
      await refreshTool(target);
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setBusyTarget(null);
    }
  };

  const runDesktopUpdate = async () => {
    if (desktopInfo && !desktopInfo.canInstallInApp) {
      try {
        await openUrl("https://claude.ai/download");
      } catch (error) {
        void message.error(errMsg(error));
      }
      return;
    }
    setBusyTarget("claude_desktop");
    try {
      const result = await runClaudeDesktopAppUpdate();
      void message.success(result);
      await queryClient.invalidateQueries({ queryKey: ["claude-desktop-app"] });
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setBusyTarget(null);
    }
  };

  const installNode = async () => {
    setInstallingNode(true);
    try {
      const status = await ensureNodeRuntimeViaFnm();
      queryClient.setQueryData(["node-runtime-status"], status);
      if (status.meetsMinimum) {
        void message.success(t("about.nodeRuntimeInstallSuccess", { version: status.version ?? "22+" }));
      } else {
        void message.error(status.installHint);
      }
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setInstallingNode(false);
    }
  };

  const handleDefaultPermissionModeChange = async (mode: string) => {
    try {
      await setClaudeCodeDefaultPermissionMode(mode);
      await defaultPermissionModeQuery.refetch();
      void message.success(t("providers.defaultPermissionModeSaved"));
    } catch (error) {
      void message.error(errMsg(error));
    }
  };

  const handleAgentSettingsChange = async (patch: Partial<ClaudeCodeAgentSettings>, savedKey = "providers.agentTeamsSaved") => {
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

  const installed: Partial<Record<ProviderTarget, boolean>> = {};
  if (claudeInfo) installed.claude_code = claudeInfo.installed;
  if (codexInfo) installed.codex = codexInfo.installed;
  if (opencodeInfo) installed.opencode = opencodeInfo.installed;
  if (piInfo) installed.pi = piInfo.installed;
  if (desktopInfo) installed.claude_desktop = desktopInfo.installed;

  const renderHeaderExtra = (target: ProviderTarget) => {
    const info = toolInfo(target);
    if (!info) return null;
    if (!info.installed) {
      if (target === "claude_desktop") {
        return (
          <Button size="small" type="primary" loading={busyTarget === "claude_desktop"} onClick={() => void runDesktopUpdate()}>
            {t("agentHub.install", { defaultValue: "安装" })}
          </Button>
        );
      }
      if (target === "cline" || target === "dsh") return null;
      return (
        <Button size="small" type="primary" loading={busyTarget === target} onClick={() => void runToolUpdate(target)}>
          {t("agentHub.install", { defaultValue: "安装" })}
        </Button>
      );
    }
    return (
      <>
        {info.currentVersion ? (
          <Text type="secondary" style={{ fontSize: 12 }}>{info.currentVersion}</Text>
        ) : null}
        <Text type="secondary" style={{ fontSize: 12 }}>
          <span aria-hidden style={{ color: "#34c759", marginRight: 4 }}>●</span>
          {t("agentHub.installed", { defaultValue: "已安装" })}
        </Text>
      </>
    );
  };

  const renderAdvanced = (target: ProviderTarget) => {
    if (target === "claude_desktop") {
      return (
        <Space>
          <Button size="small" onClick={() => void queryClient.invalidateQueries({ queryKey: ["claude-desktop-app"] })}>
            {t("agentHub.detect", { defaultValue: "检测" })}
          </Button>
          <Button size="small" loading={busyTarget === "claude_desktop"} onClick={() => void runDesktopUpdate()}>
            {t("agentHub.upgrade", { defaultValue: "升级" })}
          </Button>
          <Button size="small" type="link" onClick={() => navigate("localProxy")}>
            {t("agentHub.viewLocalProxy", { defaultValue: "查看本地代理" })}
          </Button>
        </Space>
      );
    }
    const info = toolInfo(target);
    const maintenance = info && target !== "cline" && target !== "dsh" ? (
      <Space>
        <Button size="small" onClick={() => void refreshTool(target)}>
          {t("agentHub.detect", { defaultValue: "检测" })}
        </Button>
        <Button size="small" loading={busyTarget === target} onClick={() => void runToolUpdate(target)}>
          {t("agentHub.upgrade", { defaultValue: "升级" })}
        </Button>
        <Button size="small" type="link" onClick={() => navigate("localProxy")}>
          {t("agentHub.viewLocalProxy", { defaultValue: "查看本地代理" })}
        </Button>
      </Space>
    ) : null;

    if (target === "claude_code") {
      return (
        <Space direction="vertical" size={12} style={{ width: "100%" }}>
          <SettingRow label={t("providers.defaultPermissionModeTitle")}>
            <Select
              size="small"
              value={defaultPermissionModeQuery.data ?? "default"}
              style={{ minWidth: 180 }}
              onChange={(value) => void handleDefaultPermissionModeChange(String(value))}
              options={[
                { value: "default", label: t("providers.defaultPermissionModeDefault") },
                { value: "plan", label: t("providers.defaultPermissionModePlan") },
                { value: "acceptEdits", label: t("providers.defaultPermissionModeAcceptEdits") },
                { value: "auto", label: t("providers.defaultPermissionModeAuto") },
              ]}
            />
          </SettingRow>
          <SettingRow label={t("providers.agentTeamsEnable")}>
            <Switch
              checked={agentSettingsQuery.data?.teamsEnabled ?? false}
              onChange={(checked) => void handleAgentSettingsChange({ teamsEnabled: checked })}
            />
          </SettingRow>
          {agentSettingsQuery.data?.teamsEnabled && livePromptBlocksAgentTeams(livePromptQuery.data?.content) ? (
            <Alert
              type="warning"
              showIcon
              message={t("providers.agentTeamsPromptConflict")}
              action={
                <Button
                  size="small"
                  onClick={() => {
                    setWorkspaceTarget("claude_code");
                    navigate("workspace");
                  }}
                >
                  {t("providers.agentTeamsOpenPrompts")}
                </Button>
              }
            />
          ) : null}
          <SettingRow label={t("providers.autoModeServer")}>
            <Switch
              checked={agentSettingsQuery.data?.autoModeServer ?? true}
              onChange={(checked) => void handleAgentSettingsChange({ autoModeServer: checked }, "providers.autoModeServerSaved")}
            />
          </SettingRow>
          <SettingRow label={t("providers.agentTeamsMode")}>
            <Select
              size="small"
              style={{ minWidth: 180 }}
              disabled={!agentSettingsQuery.data?.teamsEnabled}
              value={
                agentSettingsQuery.data?.teammateMode === "tmux" && !agentSettingsQuery.data.tmuxSupported
                  ? "in-process"
                  : (agentSettingsQuery.data?.teammateMode ?? "auto")
              }
              onChange={(value) => void handleAgentSettingsChange({ teammateMode: String(value) })}
              options={[
                { value: "auto", label: t("providers.agentTeamsModeAuto") },
                { value: "in-process", label: t("providers.agentTeamsModeInProcess") },
                { value: "tmux", label: t("providers.agentTeamsModeTmux"), disabled: !agentSettingsQuery.data?.tmuxSupported },
              ]}
            />
          </SettingRow>
          {maintenance}
        </Space>
      );
    }
    if (target === "opencode") {
      return (
        <Space direction="vertical" size={12}>
          <SettingRow label={t("providers.opencodePermissionTitle")}>
            <Switch
              checked={opencodePermissionQuery.data === "allow"}
              onChange={(checked) => {
                void setOpenCodePermissionMode(checked ? "allow" : "ask")
                  .then(() => opencodePermissionQuery.refetch())
                  .then(() => message.success(t("providers.opencodePermissionSaved")))
                  .catch((error: unknown) => message.error(errMsg(error)));
              }}
            />
          </SettingRow>
          {maintenance}
        </Space>
      );
    }
    if (target === "pi") {
      return (
        <Space direction="vertical" size={12}>
          <SettingRow label={t("providers.piThinkingLevelTitle", { defaultValue: "默认思考强度" })}>
            <Select
              size="small"
              style={{ minWidth: 160 }}
              value={piThinkingLevel}
              loading={piSettingsQuery.isLoading}
              onChange={(value) => {
                const level = String(value);
                setPiThinkingLevel(level);
                void updatePiSettings(null, null, level)
                  .then(() => message.success(t("providers.piThinkingLevelUpdated", { defaultValue: "已保存" })))
                  .catch((error: unknown) => message.error(errMsg(error)));
              }}
              options={["off", "minimal", "low", "medium", "high", "xhigh", "max"].map((value) => ({ value, label: value }))}
            />
          </SettingRow>
          {maintenance}
        </Space>
      );
    }
    if (target === "codex") return maintenance;
    return null;
  };

  const node = nodeQuery.data;

  return (
    <Space direction="vertical" size={16} style={{ width: "100%" }}>
      <AgentConnectionCard
        framed={false}
        installed={installed}
        renderHeaderExtra={renderHeaderExtra}
        renderAdvanced={renderAdvanced}
      />
      <Text type="secondary">
        Node.js {node?.version ?? t("agentHub.nodeUnknown", { defaultValue: "未检测" })}
        {node && !node.meetsMinimum ? ` · ${t("agentHub.nodeTooOld", { defaultValue: "版本过低" })}` : ""}
        {" "}
        <Button
          size="small"
          type="link"
          loading={installingNode}
          onClick={() => {
            if (node?.meetsMinimum) {
              void queryClient.invalidateQueries({ queryKey: ["node-runtime-status"] });
              return;
            }
            void installNode();
          }}
        >
          {node?.meetsMinimum
            ? t("common.refresh", { defaultValue: "刷新" })
            : t("about.installNodeViaFnm", { defaultValue: "安装 Node.js" })}
        </Button>
      </Text>
    </Space>
  );
}

function SettingRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12 }}>
      <span>{label}</span>
      {children}
    </div>
  );
}
