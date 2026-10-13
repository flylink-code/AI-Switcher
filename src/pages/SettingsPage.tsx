import { useCallback, useState } from "react";
import { Button, Checkbox, Select, Tag, message } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { useAppStore, type LayoutMode } from "@/stores/appStore";
import { useThemeStore, type ThemeMode } from "@/stores/themeStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { PROVIDER_TARGET_OPTIONS, LABEL_KEYS } from "@/components/AgentTargetSwitcher";
import { filterUiAgents } from "@/lib/agentVisibility";
import { languages } from "@/i18n";
import { useNavigatePage } from "@/lib/navigation";
import {
  autostartOptions,
  closeBehaviorOptions,
  gatewayRuntimeModeOptions,
  gatewaySidecarStatusOptions,
} from "@/lib/appQueries";
import { useAppVersion } from "@/lib/useAppVersion";
import {
  restartApp,
  setAutostartConfig,
  setCloseBehavior,
  setGatewayRuntimeMode,
  type GatewayRuntimeMode,
  type GatewaySidecarStatus,
} from "@/services/api";
import { SettingsSection, SettingsRow } from "@/components/settings";
import type { AutostartMode, CloseBehavior, ProviderTarget } from "@/types/backend";

/**
 * System settings only. Workspace resources (Projects / MCP / Prompts /
 * Skills / Agents / Codex Plugins) live in the Workspace page, not here.
 */
export default function SettingsPage() {
  const { t } = useTranslation();
  const navigate = useNavigatePage();
  const language = useAppStore((s) => s.language);
  const layoutMode = useAppStore((s) => s.layoutMode);
  const setLayoutMode = useAppStore((s) => s.setLayoutMode);
  const appVersion = useAppVersion();
  const setLanguage = useAppStore((s) => s.setLanguage);
  const themeMode = useThemeStore((s) => s.mode);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const visibleAgents = usePagePreferencesStore((s) => s.visibleAgents);
  const setVisibleAgents = usePagePreferencesStore((s) => s.setVisibleAgents);
  const queryClient = useQueryClient();
  const autostartQuery = useQuery(autostartOptions);
  const closeBehaviorQuery = useQuery(closeBehaviorOptions);
  const [autostartChanging, setAutostartChanging] = useState(false);
  const [closeBehaviorChanging, setCloseBehaviorChanging] = useState(false);
  const gatewayModeQuery = useQuery(gatewayRuntimeModeOptions);
  const sidecarStatusQuery = useQuery({
    ...gatewaySidecarStatusOptions,
    refetchInterval: (query) => (query.state.data?.supervised ? 3_000 : false),
  });
  const [gatewayModeChanging, setGatewayModeChanging] = useState(false);
  const [gatewayModePendingRestart, setGatewayModePendingRestart] = useState(false);

  const onGatewayModeChange = useCallback(async (mode: GatewayRuntimeMode) => {
    setGatewayModeChanging(true);
    try {
      const saved = await setGatewayRuntimeMode(mode);
      queryClient.setQueryData(gatewayRuntimeModeOptions.queryKey, saved);
      setGatewayModePendingRestart(true);
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setGatewayModeChanging(false);
    }
  }, [queryClient]);

  const onAutostartChange = useCallback(async (mode: AutostartMode) => {
    setAutostartChanging(true);
    try {
      await setAutostartConfig(mode);
      await queryClient.invalidateQueries({ queryKey: autostartOptions.queryKey });
      const next = await queryClient.fetchQuery(autostartOptions);
      if (mode !== "off" && !next.enabled) {
        void message.error(
          next.taskManagerDisabled
            ? t("env.autostartTaskManagerDisabled")
            : t("env.autostartNotRegistered"),
        );
        return;
      }
      void message.success(t("env.autostartUpdated"));
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
      await queryClient.invalidateQueries({ queryKey: autostartOptions.queryKey });
    } finally {
      setAutostartChanging(false);
    }
  }, [queryClient, t]);

  const onCloseBehaviorChange = useCallback(async (behavior: CloseBehavior) => {
    setCloseBehaviorChanging(true);
    try {
      await setCloseBehavior(behavior);
      queryClient.setQueryData(closeBehaviorOptions.queryKey, behavior);
      void message.success(t("env.closeBehaviorUpdated"));
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setCloseBehaviorChanging(false);
    }
  }, [queryClient, t]);

  return (
    <div
      className="settings-desktop-view"
      style={{
        width: "calc(100% - 64px)",
        maxWidth: 1040,
        marginInline: "auto",
        display: "flex",
        flexDirection: "column",
        gap: "28px",
        paddingBottom: "40px",
      }}
    >
      {/* General */}
      <SettingsSection title={t("settings.sectionGeneral", { defaultValue: "通用" })}>
        <SettingsRow
          title={t("common.language", { defaultValue: "语言" })}
          description={t("settings.languageHint", { defaultValue: "界面显示语言" })}
          control={
            <Select
              value={language}
              style={{ width: 190 }}
              options={languages.map((lang) => ({ value: lang.value, label: lang.label }))}
              onChange={setLanguage}
            />
          }
        />
        <SettingsRow
          title={t("common.theme", { defaultValue: "主题" })}
          description={t("settings.themeHint", { defaultValue: "外观与明暗模式" })}
          control={
            <Select<ThemeMode>
              value={themeMode}
              style={{ width: 190 }}
              options={[
                { value: "light", label: t("common.themeLight") },
                { value: "dark", label: t("common.themeDark") },
                { value: "system", label: t("common.themeSystem") },
              ]}
              onChange={setThemeMode}
            />
          }
        />
        <SettingsRow
          title={t("env.fields.autostart")}
          description={
            autostartQuery.data?.taskManagerDisabled
              ? t("env.autostartTaskManagerDisabled")
              : t("settings.autostartHint")
          }
          control={
            <Select<AutostartMode>
              value={autostartQuery.data?.mode ?? "off"}
              loading={autostartQuery.isPending || autostartChanging}
              disabled={autostartChanging}
              style={{ width: 190 }}
              options={[
                { value: "off", label: t("env.autostartModes.off") },
                { value: "silent", label: t("env.autostartModes.silent") },
                { value: "window", label: t("env.autostartModes.window") },
              ]}
              onChange={(mode) => void onAutostartChange(mode)}
            />
          }
        />
        <SettingsRow
          title={t("env.fields.closeBehavior")}
          description={t("settings.closeBehaviorHint")}
          control={
            <Select<CloseBehavior>
              value={closeBehaviorQuery.data ?? "ask"}
              loading={closeBehaviorQuery.isPending || closeBehaviorChanging}
              disabled={closeBehaviorChanging}
              style={{ width: 190 }}
              options={[
                { value: "ask", label: t("env.closeBehaviors.ask") },
                { value: "tray", label: t("env.closeBehaviors.tray") },
                { value: "quit", label: t("env.closeBehaviors.quit") },
              ]}
              onChange={(behavior) => void onCloseBehaviorChange(behavior)}
            />
          }
        />
        <SettingsRow
          title={t("settings.gatewayRuntimeMode")}
          description={
            gatewayModePendingRestart
              ? t("settings.gatewayRuntimeModeRestart")
              : t("settings.gatewayRuntimeModeHint")
          }
          control={
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              {gatewayModePendingRestart ? (
                <Button size="small" type="primary" onClick={() => void restartApp()}>
                  {t("settings.gatewayRuntimeModeRestartNow")}
                </Button>
              ) : null}
              <Select<GatewayRuntimeMode>
                value={gatewayModeQuery.data ?? "sidecar"}
                loading={gatewayModeQuery.isPending || gatewayModeChanging}
                disabled={gatewayModeChanging}
                style={{ width: 190 }}
                options={[
                  { value: "inproc", label: t("settings.gatewayRuntimeModes.inproc") },
                  { value: "sidecar", label: t("settings.gatewayRuntimeModes.sidecar") },
                ]}
                onChange={(mode) => void onGatewayModeChange(mode)}
              />
            </div>
          }
        />
        {sidecarStatusQuery.data?.supervised ? (
          <SettingsRow
            title={t("settings.gatewaySidecarStatusTitle")}
            description={sidecarStatusDescription(t, sidecarStatusQuery.data)}
          />
        ) : null}
        <SettingsRow
          title={t("settings.layout", { defaultValue: "布局" })}
          description={t("settings.layoutHint", { defaultValue: "顶栏或侧栏导航" })}
          control={
            <Select<LayoutMode>
              value={layoutMode}
              style={{ width: 190 }}
              options={[
                { value: "top", label: t("settings.layoutTop", { defaultValue: "顶栏" }) },
                { value: "sidebar", label: t("settings.layoutSidebar", { defaultValue: "侧栏" }) },
              ]}
              onChange={setLayoutMode}
            />
          }
        />
        <SettingsRow
          title={t("settings.visibleAgents", { defaultValue: "显示 Agent" })}
          description={t("settings.visibleAgentsHint", {
            defaultValue: "勾选在全局和各页面切换器中显示的 Agent 工具",
          })}
          control={
            <Checkbox.Group
              options={filterUiAgents(PROVIDER_TARGET_OPTIONS).map((target) => ({
                label: t(LABEL_KEYS[target]),
                value: target,
              }))}
              value={visibleAgents}
              onChange={(checkedValues) => {
                if (checkedValues.length > 0) {
                  setVisibleAgents(checkedValues as ProviderTarget[]);
                }
              }}
            />
          }
        />
      </SettingsSection>

      {/* Tools & environment */}
      <SettingsSection title={t("settings.sectionToolsEnv", { defaultValue: "工具与环境" })}>
        <SettingsRow
          title={t("nav.localProxy", { defaultValue: "本地代理" })}
          description={t("settings.localProxyHint", {
            defaultValue: "按 Agent 做协议转换与故障切换",
          })}
          onClick={() => navigate("localProxy")}
        />
        <SettingsRow
          title={t("nav.environment", { defaultValue: "环境信息" })}
          description={t("settings.environmentHint", { defaultValue: "本机路径与环境诊断" })}
          onClick={() => navigate("environment")}
        />
      </SettingsSection>

      {/* About */}
      <SettingsSection title={t("settings.sectionAbout", { defaultValue: "关于" })}>
        <SettingsRow
          title="AI-Switcher"
          description={t("settings.aboutHint", { defaultValue: "应用版本与更新信息" })}
          control={appVersion ? <Tag style={{ margin: 0 }}>v{appVersion}</Tag> : null}
          onClick={() => navigate("about")}
        />
      </SettingsSection>
    </div>
  );
}

function sidecarStatusDescription(t: TFunction, status: GatewaySidecarStatus): string {
  const base = status.running
    ? t("settings.gatewaySidecarRunning", { pid: status.pid, restarts: status.restarts })
    : t("settings.gatewaySidecarDown", { restarts: status.restarts });
  const health = t("settings.gatewaySidecarHealth", {
    ag: status.antigravityUp
      ? t("settings.gatewaySidecarHealthUp")
      : t("settings.gatewaySidecarHealthDown"),
    kiro: status.kiroUp
      ? t("settings.gatewaySidecarHealthUp")
      : t("settings.gatewaySidecarHealthDown"),
  });
  const extra = [
    status.lastExitCode != null
      ? t("settings.gatewaySidecarExit", { code: status.lastExitCode })
      : "",
    !status.running && status.backoffMs > 0
      ? t("settings.gatewaySidecarBackoff", {
          seconds: Math.max(1, Math.round(status.backoffMs / 1000)),
        })
      : "",
  ].filter((part) => part.length > 0);
  return [base, health, ...extra].join(" · ");
}
