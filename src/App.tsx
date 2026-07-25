import { lazy, Suspense, useEffect, useMemo, useState } from "react";
import { App as AntApp, ConfigProvider, theme as antdTheme } from "antd";
import zhCN from "antd/locale/zh_CN";
import enUS from "antd/locale/en_US";
import { useTranslation } from "react-i18next";
import { AppLayout, NAV_ITEMS } from "@/components/AppLayout";
import { useThemeStore } from "@/stores/themeStore";
import { useAppStore } from "@/stores/appStore";
const ProvidersPage = lazy(() => import("@/pages/ProvidersPage"));
const ProxyPage = lazy(() => import("@/pages/ProxyPage"));
const McpPage = lazy(() => import("@/pages/McpPage"));
const PromptsPage = lazy(() => import("@/pages/PromptsPage"));
const SkillsPage = lazy(() => import("@/pages/SkillsPage"));
const UsagePage = lazy(() => import("@/pages/UsagePage"));
const EnvironmentPage = lazy(() => import("@/pages/EnvironmentPage"));

export default function App() {
  const { i18n } = useTranslation();
  const resolved = useThemeStore((s) => s.resolved);
  const language = useAppStore((s) => s.language);

  // Default to the Providers page now that it's functional (P1).
  const [activeKey, setActiveKey] = useState<string>("providers");

  // Keep i18next in sync with the persisted language.
  useEffect(() => {
    if (i18n.language !== language) void i18n.changeLanguage(language);
  }, [language, i18n]);

  const themeConfig = useMemo(
    () => ({
      zeroRuntime: true,
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
      case "proxy":
        return <ProxyPage />;
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
          <Suspense fallback={<div style={{ padding: 24 }}>Loading…</div>}>{renderPage()}</Suspense>
        </AppLayout>
      </AntApp>
    </ConfigProvider>
  );
}
