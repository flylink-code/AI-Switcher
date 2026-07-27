import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { Alert, App as AntApp, Button, ConfigProvider, theme as antdTheme } from "antd";
import zhCN from "antd/locale/zh_CN";
import enUS from "antd/locale/en_US";
import { useTranslation } from "react-i18next";
import { AppLayout } from "@/components/AppLayout";
import { useThemeStore } from "@/stores/themeStore";
import { useAppStore } from "@/stores/appStore";
import { StartupScreen } from "@/components/StartupScreen";
import { runStartupWarmup, type StartupProgress } from "@/lib/startupWarmup";
import { reportFrontendPerformance, reportFrontendStartup } from "@/services/api";
import {
  getLoadedPage,
  preloadPage,
  type PageKey,
} from "@/lib/pageRegistry";

export default function App() {
  const { i18n } = useTranslation();
  const resolved = useThemeStore((s) => s.resolved);
  const language = useAppStore((s) => s.language);

  // Default to the Providers page now that it's functional (P1).
  const [activeKey, setActiveKey] = useState<PageKey>("providers");
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
  const navigationStartedAt = useRef<{ key: PageKey; startedAt: number } | null>(null);
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
  const handleNavigate = useCallback((key: PageKey) => {
    if (key === activeKey) return;
    navigationStartedAt.current = { key, startedAt: performance.now() };
    setActiveKey(key);
  }, [activeKey]);
  const handlePagePaint = useCallback((key: PageKey) => {
    const navigation = navigationStartedAt.current;
    if (!navigation || navigation.key !== key) return;
    navigationStartedAt.current = null;
    const durationMs = performance.now() - navigation.startedAt;
    if (durationMs > 50) {
      void reportFrontendPerformance("navigation_slow", key, durationMs).catch(
        () => undefined,
      );
    }
  }, []);

  return (
    <ConfigProvider locale={antdLocale} theme={themeConfig}>
      <AntApp>
        {!startupReady ? (
          <StartupScreen progress={startupProgress} onSkip={() => finishStartup("skipped")} />
        ) : (
          <AppLayout activeKey={activeKey} onNavigate={handleNavigate}>
            <ActivePage pageKey={activeKey} onPaint={handlePagePaint} />
          </AppLayout>
        )}
      </AntApp>
    </ConfigProvider>
  );
}

function ActivePage({
  pageKey,
  onPaint,
}: {
  pageKey: PageKey;
  onPaint: (key: PageKey) => void;
}) {
  const { t } = useTranslation();
  const [, rerender] = useReducer((value: number) => value + 1, 0);
  const [loadAttempt, retryLoad] = useReducer((value: number) => value + 1, 0);
  const [loadError, setLoadError] = useState<string | null>(null);
  const Page = getLoadedPage(pageKey);
  const renderStartedAt = performance.now();

  useEffect(() => {
    if (Page) return;
    let active = true;
    void preloadPage(pageKey)
      .then(() => {
        if (active) {
          setLoadError(null);
          rerender();
        }
      })
      .catch((error: unknown) => {
        if (active) {
          setLoadError(error instanceof Error ? error.message : String(error));
        }
      });
    return () => {
      active = false;
    };
  }, [Page, loadAttempt, pageKey]);

  useEffect(() => {
    if (!Page) return;
    const frame = window.requestAnimationFrame(() => {
      void reportFrontendPerformance(
        "page_mount",
        pageKey,
        performance.now() - renderStartedAt,
      ).catch(() => undefined);
      onPaint(pageKey);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [Page, onPaint, pageKey]);

  if (!Page) {
    return (
      <div style={{ padding: 24 }}>
        {loadError ? (
          <Alert
            type="error"
            showIcon
            message={t("startup.pageLoadFailed")}
            description={loadError}
            action={<Button onClick={retryLoad}>{t("startup.retry")}</Button>}
          />
        ) : (
          t("startup.pageRecovering")
        )}
      </div>
    );
  }

  return <Page />;
}
