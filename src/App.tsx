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
const DesktopLocalizationPage = lazy(() => import("@/pages/DesktopLocalizationPage"));
const AboutPage = lazy(() => import("@/pages/AboutPage"));

export default function App() {
  const { i18n } = useTranslation();
  const resolved = useThemeStore((s) => s.resolved);
  const language = useAppStore((s) => s.language);

  // Default to the Providers page now that it's functional (P1).
  const [activeKey, setActiveKey] = useState<string>("providers");
  const [visitedKeys, setVisitedKeys] = useState<Set<string>>(() => new Set(["providers"]));
  const [pageReady, setPageReady] = useState(false);

  // Paint the lightweight application shell before requesting the default
  // page's Ant Design Table chunk and provider data.
  useEffect(() => {
    const frame = window.requestAnimationFrame(() => setPageReady(true));
    return () => window.cancelAnimationFrame(frame);
  }, []);

  useEffect(() => {
    setVisitedKeys((current) => {
      if (current.has(activeKey)) return current;
      const next = new Set(current);
      next.add(activeKey);
      return next;
    });
  }, [activeKey]);

  useEffect(() => {
    const preload = () => {
      void Promise.allSettled([
        import("@/pages/ProxyPage"),
        import("@/pages/McpPage"),
        import("@/pages/PromptsPage"),
        import("@/pages/SkillsPage"),
        import("@/pages/UsagePage"),
        import("@/pages/EnvironmentPage"),
        import("@/pages/DesktopLocalizationPage"),
        import("@/pages/AboutPage"),
      ]).then(() => {
        setVisitedKeys(new Set(NAV_ITEMS.map((item) => item.key)));
      });
    };
    const idleWindow = window as Window & {
      requestIdleCallback?: (callback: () => void) => number;
      cancelIdleCallback?: (id: number) => void;
    };
    if (idleWindow.requestIdleCallback) {
      const idle = idleWindow.requestIdleCallback(preload);
      return () => idleWindow.cancelIdleCallback?.(idle);
    }
    const timer = window.setTimeout(preload, 500);
    return () => window.clearTimeout(timer);
  }, []);

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

  const renderPage = (key: string) => {
    switch (key) {
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
      case "localization":
        return <DesktopLocalizationPage />;
      case "about":
        return <AboutPage />;
      default:
        return <AboutPage />;
    }
  };

  // Validate active key against nav items.
  const validKey = NAV_ITEMS.some((n) => n.key === activeKey) ? activeKey : "environment";

  return (
    <ConfigProvider locale={antdLocale} theme={themeConfig}>
      <AntApp>
        <AppLayout activeKey={validKey} onNavigate={setActiveKey}>
          {pageReady
            ? NAV_ITEMS.filter((item) => visitedKeys.has(item.key)).map((item) => (
                <div
                  key={item.key}
                  hidden={item.key !== validKey}
                  aria-hidden={item.key !== validKey}
                >
                  <Suspense fallback={<div style={{ padding: 24 }}>Loading…</div>}>
                    {renderPage(item.key)}
                  </Suspense>
                </div>
              ))
            : null}
        </AppLayout>
      </AntApp>
    </ConfigProvider>
  );
}
