import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { App as AntApp, ConfigProvider, theme as antdTheme } from "antd";
import zhCN from "antd/locale/zh_CN";
import enUS from "antd/locale/en_US";
import { useTranslation } from "react-i18next";
import { AppLayout, NAV_ITEMS } from "@/components/AppLayout";
import { useThemeStore } from "@/stores/themeStore";
import { useAppStore } from "@/stores/appStore";
import { StartupScreen } from "@/components/StartupScreen";
import { runStartupWarmup, type StartupProgress } from "@/lib/startupWarmup";
import { reportFrontendStartup } from "@/services/api";
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
  const [startupReady, setStartupReady] = useState(false);
  const [startupProgress, setStartupProgress] = useState<StartupProgress>({
    completed: 0,
    total: 1,
    current: "starting",
    failures: [],
  });
  const startupStartedAt = useRef(performance.now());
  const startupFinished = useRef(false);
  const progressRef = useRef(startupProgress);
  const finishStartup = useCallback((reason: "completed" | "timeout" | "skipped") => {
    if (startupFinished.current) return;
    startupFinished.current = true;
    setStartupReady(true);
    const durationMs = Math.round(performance.now() - startupStartedAt.current);
    void reportFrontendStartup(durationMs, reason, progressRef.current.failures).catch(() => undefined);
  }, []);

  useEffect(() => {
    let active = true;
    const timeout = window.setTimeout(() => {
      if (active) finishStartup("timeout");
    }, 5_000);
    void runStartupWarmup((progress) => {
      if (active) {
        progressRef.current = progress;
        setStartupProgress(progress);
      }
    }).then(() => {
      if (!active) return;
      window.clearTimeout(timeout);
      finishStartup("completed");
    });
    return () => {
      active = false;
      window.clearTimeout(timeout);
    };
  }, [finishStartup]);

  useEffect(() => {
    setVisitedKeys((current) => {
      if (current.has(activeKey)) return current;
      const next = new Set(current);
      next.add(activeKey);
      return next;
    });
  }, [activeKey]);

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
        {!startupReady ? (
          <StartupScreen progress={startupProgress} onSkip={() => finishStartup("skipped")} />
        ) : (
          <AppLayout activeKey={validKey} onNavigate={setActiveKey}>
            {NAV_ITEMS.filter((item) => visitedKeys.has(item.key)).map((item) => (
                <div
                  key={item.key}
                  hidden={item.key !== validKey}
                  aria-hidden={item.key !== validKey}
                >
                  <Suspense fallback={<div style={{ padding: 24 }}>Loading…</div>}>
                    {renderPage(item.key)}
                  </Suspense>
                </div>
              ))}
          </AppLayout>
        )}
      </AntApp>
    </ConfigProvider>
  );
}
