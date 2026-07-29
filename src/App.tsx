import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import {
  Alert,
  App as AntApp,
  Button,
  Checkbox,
  ConfigProvider,
  Modal,
  Typography,
  theme as antdTheme,
} from "antd";
import zhCN from "antd/locale/zh_CN";
import enUS from "antd/locale/en_US";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { AppLayout } from "@/components/AppLayout";
import { useThemeStore } from "@/stores/themeStore";
import { useAppStore } from "@/stores/appStore";
import { StartupScreen } from "@/components/StartupScreen";
import { runStartupWarmup, type StartupProgress } from "@/lib/startupWarmup";
import {
  reportFrontendPerformance,
  reportFrontendStartup,
  restartApp,
  resolveCloseRequest,
} from "@/services/api";
import { checkForAppUpdate, installAvailableAppUpdate, type AppUpdate } from "@/lib/appUpdater";
import {
  getLoadedPage,
  preloadPage,
  type PageKey,
} from "@/lib/pageRegistry";

export default function App() {
  const { t, i18n } = useTranslation();
  const resolved = useThemeStore((s) => s.resolved);
  const language = useAppStore((s) => s.language);

  // Default to the Providers page now that it's functional (P1).
  const [activeKey, setActiveKey] = useState<PageKey>("providers");
  const [startupReady, setStartupReady] = useState(false);
  const [closeDialogOpen, setCloseDialogOpen] = useState(false);
  const [rememberCloseChoice, setRememberCloseChoice] = useState(false);
  const [resolvingClose, setResolvingClose] = useState(false);
  const [closeError, setCloseError] = useState<string | null>(null);
  const [availableUpdate, setAvailableUpdate] = useState<AppUpdate | null>(null);
  const [updatePromptOpen, setUpdatePromptOpen] = useState(false);
  const [installingUpdate, setInstallingUpdate] = useState(false);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [startupProgress, setStartupProgress] = useState<StartupProgress>({
    completed: 0,
    total: 1,
    current: "starting",
    failures: [],
  });
  const startupStartedAt = useRef(performance.now());
  const startupFinished = useRef(false);
  const startupLanguage = useRef(language);
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
    void runStartupWarmup(startupLanguage.current, (progress) => {
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
    if (language !== "zh-CN" && activeKey === "localization") {
      setActiveKey("providers");
    }
  }, [activeKey, language]);

  useEffect(() => {
    if (!startupReady) return;
    let active = true;
    const checkForUpdate = async () => {
      try {
        const update = await checkForAppUpdate(t("about.appUpdateTimedOut"));
        if (active && update) setAvailableUpdate(update);
      } catch (error) {
        console.warn("Automatic application update check failed", error);
      }
    };
    void checkForUpdate();
    const interval = window.setInterval(() => void checkForUpdate(), 6 * 60 * 60 * 1000);
    return () => {
      active = false;
      window.clearInterval(interval);
    };
  }, [startupReady, t]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    const hasTauri =
      typeof window !== "undefined" &&
      Boolean((window as unknown as Record<string, unknown>).__TAURI_INTERNALS__);
    if (!hasTauri) return;
    void listen("close-choice-requested", () => {
      setCloseError(null);
      setCloseDialogOpen(true);
    }).then((dispose) => {
      if (active) unlisten = dispose;
      else dispose();
    }).catch((error: unknown) => {
      console.error("Failed to register close-choice listener", error);
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  // Keep i18next in sync with the persisted language.
  useEffect(() => {
    if (i18n.language !== language) void i18n.changeLanguage(language);
  }, [language, i18n]);

  useEffect(() => {
    document.documentElement.dataset.theme = resolved;
    document.documentElement.style.colorScheme = resolved;
  }, [resolved]);

  const themeConfig = useMemo(
    () => ({
      algorithm:
        resolved === "dark" ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm,
      token: resolved === "dark" ? {
        colorPrimary: "#58a6ff",
        colorBgBase: "#010409",
        colorBgLayout: "#0d1117",
        colorBgContainer: "#161b22",
        colorBgElevated: "#161b22",
        colorFillSecondary: "#21262d",
        colorBorder: "#30363d",
        colorBorderSecondary: "#21262d",
        colorText: "#e6edf3",
        colorTextSecondary: "#8b949e",
        borderRadius: 8,
      } : { colorPrimary: "#5865f2", borderRadius: 8 },
      components: resolved === "dark" ? {
        Layout: {
          bodyBg: "#0d1117",
          headerBg: "#161b22",
          headerColor: "#e6edf3",
          siderBg: "#0d1117",
        },
        Menu: {
          darkItemBg: "#0d1117",
          darkSubMenuItemBg: "#0d1117",
          darkItemColor: "#8b949e",
          darkItemHoverColor: "#e6edf3",
          darkItemHoverBg: "#21262d",
          darkItemSelectedColor: "#e6edf3",
          darkItemSelectedBg: "#1f6feb",
          darkGroupTitleColor: "#8b949e",
        },
      } : undefined,
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
  const handleCloseChoice = useCallback(
    async (action: "tray" | "quit") => {
      setResolvingClose(true);
      try {
        await resolveCloseRequest(action, rememberCloseChoice);
        setCloseDialogOpen(false);
        setRememberCloseChoice(false);
        setCloseError(null);
      } catch (error) {
        setCloseError(error instanceof Error ? error.message : String(error));
      } finally {
        setResolvingClose(false);
      }
    },
    [rememberCloseChoice],
  );

  const installAvailableUpdate = useCallback(async () => {
    if (!availableUpdate) return;
    setInstallingUpdate(true);
    try {
      await installAvailableAppUpdate(availableUpdate.version);
      await restartApp();
    } catch (error) {
      console.error("Application update installation failed", error);
      setUpdateError(t("about.appUpdateFailedDetail", { error: error instanceof Error ? error.message : String(error) }));
    } finally {
      setInstallingUpdate(false);
    }
  }, [availableUpdate, t]);

  return (
    <ConfigProvider locale={antdLocale} theme={themeConfig}>
      <AntApp>
        {!startupReady ? (
          <StartupScreen progress={startupProgress} onSkip={() => finishStartup("skipped")} />
        ) : (
          <AppLayout
            activeKey={activeKey}
            onNavigate={handleNavigate}
            updateVersion={availableUpdate?.version}
            onOpenUpdate={() => {
              setUpdateError(null);
              setUpdatePromptOpen(true);
            }}
          >
            <ActivePage pageKey={activeKey} onPaint={handlePagePaint} />
          </AppLayout>
        )}
        <Modal
          open={closeDialogOpen}
          title={t("app.closeDialogTitle")}
          closable={false}
          maskClosable={false}
          keyboard={false}
          footer={[
            <Button
              key="tray"
              type="primary"
              loading={resolvingClose}
              onClick={() => void handleCloseChoice("tray")}
            >
              {t("app.closeToTray")}
            </Button>,
            <Button
              key="quit"
              danger
              disabled={resolvingClose}
              onClick={() => void handleCloseChoice("quit")}
            >
              {t("app.quitDirectly")}
            </Button>,
          ]}
        >
          {closeError && (
            <Alert
              type="error"
              showIcon
              message={closeError}
              style={{ marginBottom: 16 }}
            />
          )}
          <Typography.Paragraph>{t("app.closeDialogDescription")}</Typography.Paragraph>
          <Checkbox
            checked={rememberCloseChoice}
            disabled={resolvingClose}
            onChange={(event) => setRememberCloseChoice(event.target.checked)}
          >
            {t("app.rememberCloseChoice")}
          </Checkbox>
        </Modal>
        <Modal
          open={updatePromptOpen}
          title={t("about.appUpdateAvailable", { version: availableUpdate?.version })}
          okText={t("about.appUpdateInstall")}
          cancelText={t("providers.cancel")}
          confirmLoading={installingUpdate}
          onOk={() => void installAvailableUpdate()}
          onCancel={() => {
            setUpdateError(null);
            setUpdatePromptOpen(false);
          }}
        >
          {updateError && <Alert type="error" showIcon message={updateError} style={{ marginBottom: 16 }} />}
          {t("about.appUpdatePrompt")}
        </Modal>
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
