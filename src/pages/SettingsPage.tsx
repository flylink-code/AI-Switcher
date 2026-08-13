import { Checkbox, Select } from "antd";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/appStore";
import { useThemeStore, type ThemeMode } from "@/stores/themeStore";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { TARGET_OPTIONS, LABEL_KEYS } from "@/components/AgentTargetSwitcher";
import { languages } from "@/i18n";
import { useNavigatePage } from "@/lib/navigation";
import { SettingsSection, SettingsRow } from "@/components/settings";
import type { ProviderTarget } from "@/types/backend";

/**
 * System settings only. Workspace resources (Projects / MCP / Prompts /
 * Skills / Agents / Codex Plugins) live in the Workspace page, not here.
 */
export default function SettingsPage() {
  const { t } = useTranslation();
  const navigate = useNavigatePage();
  const language = useAppStore((s) => s.language);
  const setLanguage = useAppStore((s) => s.setLanguage);
  const themeMode = useThemeStore((s) => s.mode);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const visibleAgents = usePagePreferencesStore((s) => s.visibleAgents);
  const setVisibleAgents = usePagePreferencesStore((s) => s.setVisibleAgents);

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
          title={t("settings.visibleAgents", { defaultValue: "显示 Agent" })}
          description={t("settings.visibleAgentsHint", {
            defaultValue: "勾选在全局和各页面切换器中显示的 Agent 工具",
          })}
          control={
            <Checkbox.Group
              options={TARGET_OPTIONS.map((target) => ({
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
          title={t("navigation.proxy", { defaultValue: "本地代理" })}
          description={t("settings.proxyHint", {
            defaultValue: "改端口、强制重启或调整故障切换；日常切换供应商会自动配套",
          })}
          onClick={() => navigate("proxy")}
        />
        <SettingsRow
          title={t("nav.environment", { defaultValue: "环境信息" })}
          description={t("settings.environmentHint", { defaultValue: "本机路径与环境诊断" })}
          onClick={() => navigate("environment")}
        />
        <SettingsRow
          title={t("nav.agentTools", { defaultValue: "Agent 工具" })}
          description={t("settings.agentToolsHint", {
            defaultValue: "检测并安装 / 更新 Node.js、Claude Code、Codex、OpenCode、Pi CLI 等 Agent 工具",
          })}
          onClick={() => navigate("agentTools")}
        />
      </SettingsSection>

      {/* Localization (zh-CN only feature) */}
      {language === "zh-CN" && (
        <SettingsSection title={t("settings.sectionLocalization", { defaultValue: "本地化" })}>
          <SettingsRow
            title={t("nav.localization", { defaultValue: "汉化与本地化" })}
            description={t("settings.localizationHint", { defaultValue: "客户端汉化与翻译配置" })}
            onClick={() => navigate("localization")}
          />
        </SettingsSection>
      )}

      {/* About */}
      <SettingsSection title={t("settings.sectionAbout", { defaultValue: "关于" })}>
        <SettingsRow
          title="AI-Switcher"
          description={t("settings.aboutHint", { defaultValue: "应用版本与更新信息" })}
          onClick={() => navigate("about")}
        />
      </SettingsSection>
    </div>
  );
}
