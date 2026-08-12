import { Component, useCallback, useEffect, useMemo, useReducer, useRef, useState, type ErrorInfo, type ReactNode } from "react";

interface ErrorBoundaryProps {
  children?: ReactNode;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  public state: ErrorBoundaryState = {
    hasError: false,
    error: null,
  };

  public static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error("Uncaught rendering error:", error, errorInfo);
  }

  public render() {
    if (this.state.hasError) {
      return (
        <div style={{ padding: 24 }}>
          <Alert
            type="error"
            showIcon
            message="页面渲染异常"
            description={this.state.error?.message || "发生了未捕获的渲染错误。"}
            action={
              <Button onClick={() => this.setState({ hasError: false, error: null })}>
                重试
              </Button>
            }
          />
        </div>
      );
    }
    return this.props.children;
  }
}
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
import { AppShell } from "@/components/layout/AppShell";
import { DesktopShell } from "@/components/v2/shell/DesktopShell";
import { DashboardV2 } from "@/components/v2/dashboard/DashboardV2";
import { ServicePageV2 } from "@/components/v2/service/ServicePageV2";
import { UsagePageV2 } from "@/components/v2/usage/UsagePageV2";
import { SettingsPageV2 } from "@/components/v2/settings/SettingsPageV2";
import { ImportPreviewDialog } from "@/components/ImportPreviewDialog";
import { useThemeStore } from "@/stores/themeStore";
import { useAppStore } from "@/stores/appStore";
import { StartupScreen } from "@/components/StartupScreen";
import { runStartupWarmup, type StartupProgress } from "@/lib/startupWarmup";
import {
  reportFrontendPerformance,
  reportFrontendStartup,
  restartApp,
  resolveCloseRequest,
  confirmImportPreview,
} from "@/services/api";
import { checkForAppUpdate, installAvailableAppUpdate, type AppUpdate } from "@/lib/appUpdater";
import {
  getLoadedPage,
  preloadPage,
  type PageKey,
} from "@/lib/pageRegistry";
import { NavigationContext } from "@/lib/navigation";
import type { ImportPreview } from "@/types/backend";
import { message as staticMessage } from "antd";

export default function App() {
  const { t, i18n } = useTranslation();
  const resolved = useThemeStore((s) => s.resolved);
  const language = useAppStore((s) => s.language);
  const uiMode = useAppStore((s) => s.uiMode);

  // Default to Overview as the workspace home.
  const [activeKey, setActiveKey] = useState<PageKey>("workbench");
  const [startupReady, setStartupReady] = useState(false);
  const [closeDialogOpen, setCloseDialogOpen] = useState(false);
  const [rememberCloseChoice, setRememberCloseChoice] = useState(false);
  const [resolvingClose, setResolvingClose] = useState(false);
  const [closeError, setCloseError] = useState<string | null>(null);
  const [availableUpdate, setAvailableUpdate] = useState<AppUpdate | null>(null);
  const [updatePromptOpen, setUpdatePromptOpen] = useState(false);
  const [installingUpdate, setInstallingUpdate] = useState(false);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [deeplinkPreview, setDeeplinkPreview] = useState<ImportPreview | null>(null);
  const [deeplinkConfirming, setDeeplinkConfirming] = useState(false);
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
      setActiveKey("workbench");
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

  useEffect(() => {
    let active = true;
    const disposers: Array<() => void> = [];
    const hasTauri =
      typeof window !== "undefined" &&
      Boolean((window as unknown as Record<string, unknown>).__TAURI_INTERNALS__);
    if (!hasTauri) return;
    void listen<ImportPreview>("deeplink-import", (event) => {
      setDeeplinkPreview(event.payload);
    }).then((dispose) => {
      if (active) disposers.push(dispose);
      else dispose();
    });
    void listen<{ message?: string }>("deeplink-error", (event) => {
      void staticMessage.error(event.payload?.message ?? t("deeplink.invalid"));
    }).then((dispose) => {
      if (active) disposers.push(dispose);
      else dispose();
    });
    return () => {
      active = false;
      for (const dispose of disposers) dispose();
    };
  }, [t]);

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
      cssVar: { key: "ai-switcher" },
      zeroRuntime: false,
      token: resolved === "dark" ? {
        colorPrimary: "#3b82f6",
        colorBgBase: "#0b0f19",
        colorBgLayout: "#0b0f19",
        colorBgContainer: "#111827",
        colorBgElevated: "#1f2937",
        colorFillSecondary: "#1f2937",
        colorFillTertiary: "#1e293b",
        colorBorder: "#374151",
        colorBorderSecondary: "#1f2937",
        colorText: "#f9fafb",
        colorTextSecondary: "#9ca3af",
        colorTextTertiary: "#6b6b6b",
        borderRadius: 6,
        borderRadiusLG: 8,
      } : {
        colorPrimary: "#007aff",
        colorBgLayout: "#f4f5f8",
        colorBgContainer: "#ffffff",
        colorBgElevated: "#ffffff",
        colorFillTertiary: "#f1f5f9",
        colorBorder: "#e2e8f0",
        colorBorderSecondary: "#f1f5f9",
        colorText: "#0f172a",
        colorTextSecondary: "#475569",
        colorTextTertiary: "#94a3b8",
        borderRadius: 6,
        borderRadiusLG: 8,
      },
      components: resolved === "dark" ? {
        Layout: {
          bodyBg: "#0b0f19",
          headerBg: "transparent",
          headerColor: "#f9fafb",
          siderBg: "#111827",
        },
        Menu: {
          darkItemBg: "#111827",
          darkSubMenuItemBg: "#111827",
          darkItemColor: "#9ca3af",
          darkItemHoverColor: "#f9fafb",
          darkItemHoverBg: "#1f2937",
          darkItemSelectedColor: "#ffffff",
          darkItemSelectedBg: "#2563eb",
          darkGroupTitleColor: "#6b6b6b",
          itemBorderRadius: 6,
          itemMarginInline: 6,
        },
        Card: {
          paddingLG: 14,
          borderRadiusLG: 8,
        },
        Button: {
          controlHeight: 32,
          controlHeightSM: 26,
          borderRadius: 6,
        },
        Input: {
          controlHeight: 32,
          controlHeightSM: 26,
          borderRadius: 6,
        },
        Select: {
          controlHeight: 32,
          controlHeightSM: 26,
          borderRadius: 6,
        },
        Table: {
          cellPaddingBlock: 8,
          cellPaddingInline: 12,
          borderRadius: 8,
        },
      } : {
        Layout: {
          headerBg: "transparent",
          siderBg: "#ffffff",
        },
        Menu: {
          itemBorderRadius: 6,
          itemMarginInline: 6,
        },
        Card: {
          paddingLG: 14,
          borderRadiusLG: 8,
        },
        Button: {
          controlHeight: 32,
          controlHeightSM: 26,
          borderRadius: 6,
        },
        Input: {
          controlHeight: 32,
          controlHeightSM: 26,
          borderRadius: 6,
        },
        Select: {
          controlHeight: 32,
          controlHeightSM: 26,
          borderRadius: 6,
        },
        Table: {
          cellPaddingBlock: 8,
          cellPaddingInline: 12,
          borderRadius: 8,
        },
      },
    }),
    [resolved],
  );
  const csp = useMemo(() => {
    // Tauri adds a per-document nonce to bundled inline styles. Reuse it for
    // Ant Design's runtime styles; otherwise WebView2 blocks the generated
    // dark-theme variables even though the <style> elements exist in the DOM.
    const nonce = document.querySelector<HTMLStyleElement>("style[nonce]")?.nonce;
    return nonce ? { nonce } : undefined;
  }, []);

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

  const handleConfirmDeeplink = useCallback(async () => {
    if (!deeplinkPreview) return;
    setDeeplinkConfirming(true);
    try {
      const result = await confirmImportPreview(deeplinkPreview);
      void staticMessage.success(
        t("providers.importSummary", {
          imported: result.imported,
          skipped: result.skipped,
        }),
      );
      setDeeplinkPreview(null);
    } catch (error) {
      void staticMessage.error(error instanceof Error ? error.message : String(error));
    } finally {
      setDeeplinkConfirming(false);
    }
  }, [deeplinkPreview, t]);

  return (
    <ConfigProvider csp={csp} locale={antdLocale} theme={themeConfig}>
      <AntApp>
        {!startupReady ? (
          <StartupScreen progress={startupProgress} onSkip={() => finishStartup("skipped")} />
        ) : (
          <NavigationContext.Provider value={handleNavigate}>
            {uiMode === "v2" ? (
              <DesktopShell
                activeKey={activeKey}
                onNavigate={handleNavigate}
                updateVersion={availableUpdate?.version}
                onOpenUpdate={() => {
                  setUpdateError(null);
                  setUpdatePromptOpen(true);
                }}
              >
                <ErrorBoundary key={activeKey}>
                  <ActivePage pageKey={activeKey} onPaint={handlePagePaint} onNavigate={handleNavigate} />
                </ErrorBoundary>
              </DesktopShell>
            ) : (
              <AppShell
                activeKey={activeKey}
                onNavigate={handleNavigate}
                updateVersion={availableUpdate?.version}
                onOpenUpdate={() => {
                  setUpdateError(null);
                  setUpdatePromptOpen(true);
                }}
              >
                <ErrorBoundary key={activeKey}>
                  <ActivePage pageKey={activeKey} onPaint={handlePagePaint} onNavigate={handleNavigate} />
                </ErrorBoundary>
              </AppShell>
            )}
          </NavigationContext.Provider>
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
        <ImportPreviewDialog
          open={deeplinkPreview !== null}
          preview={deeplinkPreview}
          confirming={deeplinkConfirming}
          onCancel={() => setDeeplinkPreview(null)}
          onConfirm={() => void handleConfirmDeeplink()}
        />
      </AntApp>
    </ConfigProvider>
  );
}

function ActivePage({
  pageKey,
  onPaint,
  onNavigate,
}: {
  pageKey: PageKey;
  onPaint: (key: PageKey) => void;
  onNavigate: (key: PageKey) => void;
}) {
  const { t } = useTranslation();
  const uiMode = useAppStore((s) => s.uiMode);
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

  if (uiMode === "v2") {
    if (pageKey === "workbench") {
      return <DashboardV2 onNavigate={onNavigate} />;
    }
    if (pageKey === "providers") {
      return <ServicePageV2 initialTab="providers" onNavigate={onNavigate} />;
    }
    if (pageKey === "proxy") {
      return <ServicePageV2 initialTab="proxy" onNavigate={onNavigate} />;
    }
    if (pageKey === "antigravity") {
      return <ServicePageV2 initialTab="accounts" onNavigate={onNavigate} />;
    }
    if (
      pageKey === "workspace" ||
      pageKey === "mcp" ||
      pageKey === "prompts" ||
      pageKey === "skills" ||
      pageKey === "agents" ||
      pageKey === "plugins"
    ) {
      return <ServicePageV2 initialTab="workspace" onNavigate={onNavigate} />;
    }
    if (pageKey === "usage") {
      return <UsagePageV2 />;
    }
    if (pageKey === "about") {
      return <SettingsPageV2 initialTab="about" onNavigate={onNavigate} />;
    }
    if (
      pageKey === "settings" ||
      pageKey === "sessions" ||
      pageKey === "environment" ||
      pageKey === "localization"
    ) {
      return <SettingsPageV2 initialTab="general" onNavigate={onNavigate} />;
    }
  }

  return <Page />;
}
