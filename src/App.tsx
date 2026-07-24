import { useEffect, useMemo, useState } from "react";
import { App as AntApp, ConfigProvider, theme as antdTheme } from "antd";
import zhCN from "antd/locale/zh_CN";
import enUS from "antd/locale/en_US";
import { useTranslation } from "react-i18next";
import { AppLayout, NAV_ITEMS } from "@/components/AppLayout";
import { useThemeStore } from "@/stores/themeStore";
import { useAppStore } from "@/stores/appStore";
import ProvidersPage from "@/pages/ProvidersPage";
import McpPage from "@/pages/McpPage";
import PromptsPage from "@/pages/PromptsPage";
import SkillsPage from "@/pages/SkillsPage";
import UsagePage from "@/pages/UsagePage";
import EnvironmentPage from "@/pages/EnvironmentPage";

export default function App() {
  const { i18n } = useTranslation();
  const resolved = useThemeStore((s) => s.resolved);
  const language = useAppStore((s) => s.language);

  // Default to the Environment page so P0 detection is visible on first launch.
  const [activeKey, setActiveKey] = useState<string>("environment");

  // Keep i18next in sync with the persisted language.
  useEffect(() => {
    if (i18n.language !== language) void i18n.changeLanguage(language);
  }, [language, i18n]);

  const themeConfig = useMemo(
    () => ({
      algorithm:
        resolved === "dark" ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm,
      token: { colorPrimary: "#5865f2", borderRadius: 8 },
    }),
    [resolved],
  );

  const antdLocale = language === "en-US" ? enUS : zhCN;

  const renderPage = () => {
    switch (activeKey) {
      case "providers":
        return <ProvidersPage />;
      case "mcp":
        return <McpPage />;
      case "prompts":
        return <PromptsPage />;
      case "skills":
        return <SkillsPage />;
      case "usage":
        return <UsagePage />;
      case "environment":
        return <EnvironmentPage />;
      default:
        return <EnvironmentPage />;
    }
  };

  // Validate active key against nav items.
  const validKey = NAV_ITEMS.some((n) => n.key === activeKey) ? activeKey : "environment";

  return (
    <ConfigProvider locale={antdLocale} theme={themeConfig}>
      <AntApp>
        <AppLayout activeKey={validKey} onNavigate={setActiveKey}>
          {renderPage()}
        </AppLayout>
      </AntApp>
    </ConfigProvider>
  );
}
