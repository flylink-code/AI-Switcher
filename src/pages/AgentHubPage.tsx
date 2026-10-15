import { useState, type ReactNode } from "react";
import { Alert, Button, Modal, Select, Space, Switch, Tag, Typography, message } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { AgentConnectionCard } from "@/components/AgentConnectionCard";
import { VscodeCopilotCard } from "@/components/agent/VscodeCopilotCard";
import { CodexAdvancedSettings } from "@/components/agent/CodexAdvancedSettings";
import { ClaudeCodeLocalizationSection } from "@/components/agent/localization/ClaudeCodeLocalizationSection";
import { ClaudeDesktopLocalizationSection } from "@/components/agent/localization/ClaudeDesktopLocalizationSection";
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
import {
  getOpenCodeAction,
  getToolUpdateState,
  getUpgradeButton,
  opencodePackageName,
} from "@/lib/cliUpdate";
import { errMsg } from "@/lib/errMsg";
import { useNavigatePage } from "@/lib/navigation";
import { useAppStore } from "@/stores/appStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { ClaudeCodeAgentSettings, ProviderTarget } from "@/types/backend";
import {
  ensureNodeRuntimeViaFnm,
  getClaudeCodeAgentSettings,
  getClaudeCodeDefaultPermissionMode,
  getClaudeDesktopAutoMode,
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
  setClaudeDesktopAutoMode,
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
  // 中文化仅 zh-CN 界面开放（沿用原汉化页的门槛）。
  const showLocalization = useAppStore((s) => s.language) === "zh-CN";
  const [piThinkingLevel, setPiThinkingLevel] = useState("medium");
  const [busyTarget, setBusyTarget] = useState<BusyTarget | null>(null);
  const [installingNode, setInstallingNode] = useState(false);
  const [opencodeSwitchOpen, setOpencodeSwitchOpen] = useState(false);

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
  const opencodeAction = getOpenCodeAction(opencodeInfo);

  const desktopAutoModeQuery = useQuery({
    queryKey: ["claude-desktop-auto-mode"],
    queryFn: getClaudeDesktopAutoMode,
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
      case "vscode_copilot":
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
              ? await runOpenCodeCliUpdate()
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

  const handleDesktopAutoModeChange = async (enabled: boolean) => {
    try {
      await setClaudeDesktopAutoMode(enabled);
      await desktopAutoModeQuery.refetch();
      void message.success(t("providers.desktopAutoModeSaved"));
    } catch (error) {
      void message.error(errMsg(error));
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
  // Codex 允许只装桌面版（无 CLI）：故意不进 installed，卡片始终可用。
  if (opencodeInfo) installed.opencode = opencodeInfo.installed;
  if (piInfo) installed.pi = piInfo.installed;
  if (desktopInfo) installed.claude_desktop = desktopInfo.installed;

  // 更新状态只对四个 CLI 类 Agent 有意义；Claude Desktop 等返回 null。
  const updateStateFor = (target: ProviderTarget) => {
    switch (target) {
      case "claude_code":
        return getToolUpdateState(claudeInfo);
      case "codex":
        return getToolUpdateState(codexInfo);
      case "opencode":
        return getToolUpdateState(opencodeInfo);
      case "pi":
        return getToolUpdateState(piInfo);
      default:
        return null;
    }
  };

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
      if (target === "cline" || target === "dsh" || target === "vscode_copilot") return null;
      if (target === "codex") {
        // Codex 可只装桌面版：CLI 可选，不抢眼的安装按钮。
        return (
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("agentHub.codexCliOptional", { defaultValue: "CLI 未安装（可选）" })}
          </Text>
        );
      }
      return (
        <Button size="small" type="primary" loading={busyTarget === target} onClick={() => void runToolUpdate(target)}>
          {t("agentHub.install", { defaultValue: "安装" })}
        </Button>
      );
    }
    const updateState = updateStateFor(target);
    return (
      <>
        {info.currentVersion ? (
          <Text type="secondary" style={{ fontSize: 12 }}>{info.currentVersion}</Text>
        ) : null}
        <Text type="secondary" style={{ fontSize: 12 }}>
          <span aria-hidden style={{ color: "#34c759", marginRight: 4 }}>●</span>
          {t("agentHub.installed", { defaultValue: "已安装" })}
        </Text>
        {updateState?.hasUpdate ? (
          <Tag color="warning" style={{ marginInlineEnd: 0 }}>
            {t("agentHub.updateAvailable", {
              version: updateState.latestVersion ?? "",
              defaultValue: "有新版本 {{version}}",
            })}
          </Tag>
        ) : null}
      </>
    );
  };

  const renderAdvanced = (target: ProviderTarget) => {
    if (target === "claude_desktop") {
      return (
        <Space direction="vertical" size={12} style={{ width: "100%" }}>
          <SettingRow label={t("providers.desktopAutoMode")}>
            <Switch
              checked={desktopAutoModeQuery.data ?? true}
              onChange={(checked) => void handleDesktopAutoModeChange(checked)}
            />
          </SettingRow>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("providers.desktopAutoModeHint")}
          </Text>
          {showLocalization ? <ClaudeDesktopLocalizationSection /> : null}
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
        </Space>
      );
    }
    const info = toolInfo(target);
    const updateState = updateStateFor(target);
    const replacingV1 = target === "opencode" && opencodeAction.kind === "switch";
    const upgradeButton = updateState ? getUpgradeButton(updateState) : null;
    // Codex 未安装 CLI（桌面版用户）：把“升级”换成“安装 CLI”。
    const codexCliMissing = target === "codex" && codexInfo != null && !codexInfo.installed;
    const upgradeLabel = replacingV1 || upgradeButton?.labelKey !== "upToDate"
      ? t("agentHub.upgrade", { defaultValue: "升级" })
      : t("agentHub.upToDate", { defaultValue: "已是最新" });
    const maintenance = info && target !== "cline" && target !== "dsh" && target !== "vscode_copilot" ? (
      <Space>
        <Button size="small" onClick={() => void refreshTool(target)}>
          {t("agentHub.detect", { defaultValue: "检测" })}
        </Button>
        {codexCliMissing ? (
          <Button size="small" type="primary" loading={busyTarget === "codex"} onClick={() => void runToolUpdate("codex")}>
            {t("agentHub.installCodexCli", { defaultValue: "安装 CLI" })}
          </Button>
        ) : (
          <Button
            size="small"
            type={replacingV1 ? "primary" : (upgradeButton?.type ?? "default")}
            disabled={replacingV1 ? false : (upgradeButton?.disabled ?? false)}
            loading={busyTarget === target}
            onClick={() => {
              if (replacingV1) {
                setOpencodeSwitchOpen(true);
                return;
              }
              void runToolUpdate(target);
            }}
          >
            {upgradeLabel}
          </Button>
        )}
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
          {showLocalization ? <ClaudeCodeLocalizationSection /> : null}
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
    if (target === "codex") {
      return (
        <Space direction="vertical" size={12} style={{ width: "100%" }}>
          <CodexAdvancedSettings />
          {maintenance}
        </Space>
      );
    }
    return null;
  };

  const node = nodeQuery.data;

  return (
    <Space direction="vertical" size={12} style={{ width: "100%" }}>
      <AgentConnectionCard
        framed={false}
        installed={installed}
        renderHeaderExtra={renderHeaderExtra}
        renderAdvanced={renderAdvanced}
      />
      <VscodeCopilotCard />
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
      {/* 已装 v1 时升级会卸载 opencode-ai：App 级 Modal 确认（禁用 Modal.confirm）。 */}
      <Modal
        open={opencodeSwitchOpen}
        title={t("agentHub.opencodeSwitchConfirmTitle", {
          channel: "v2",
          defaultValue: "升级到 OpenCode v2？",
        })}
        okText={t("agentHub.upgrade", { defaultValue: "升级" })}
        cancelText={t("common.cancel", { defaultValue: "取消" })}
        confirmLoading={busyTarget === "opencode"}
        onCancel={() => setOpencodeSwitchOpen(false)}
        onOk={async () => {
          // 先执行再关闭，confirmLoading 才能显示安装进度。
          try {
            await runToolUpdate("opencode");
          } finally {
            setOpencodeSwitchOpen(false);
          }
        }}
      >
        <Text>
          {t("agentHub.opencodeSwitchConfirmBody", {
            from: "v1",
            to: "v2",
            fromPkg: opencodePackageName("v1"),
            toPkg: opencodePackageName("v2"),
            defaultValue:
              "将卸载 {{fromPkg}}（{{from}}）并安装 {{toPkg}}（{{to}}）。两者共用 opencode 命令；配置仍在 ~/.config/opencode，不会被改动。",
          })}
        </Text>
      </Modal>
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
