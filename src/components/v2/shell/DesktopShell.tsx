import React from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import type { PageKey } from "@/lib/pageRegistry";
import { useThemeStore } from "@/stores/themeStore";
import { TitlebarActions } from "@/components/layout/TitlebarActions";
import { WINDOW_CHROME_HEIGHT } from "@/components/layout/WindowChrome";
import { AppBrand } from "./AppBrand";
import { TopNavigation } from "./TopNavigation";
import { ContextHeader } from "@/components/layout/ContextHeader";

const appWindow = getCurrentWindow();

export interface DesktopShellProps {
  activeKey: PageKey;
  onNavigate: (key: PageKey) => void;
  updateVersion?: string | null;
  onOpenUpdate?: () => void;
  children: React.ReactNode;
}

const PRIMARY_PAGES = new Set<PageKey>([
  "workbench",
  "agentHub",
  "providers",
  "accounts",
  "usage",
  "workspace",
  "settings",
]);

export const DesktopShell: React.FC<DesktopShellProps> = ({
  activeKey,
  onNavigate,
  updateVersion,
  onOpenUpdate,
  children,
}) => {
  const { t } = useTranslation();
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const isDark = resolvedTheme === "dark";

  const isPrimaryPage = PRIMARY_PAGES.has(activeKey);

  const getSecondaryHeaderMeta = (key: PageKey) => {
    switch (key) {
      case "environment":
        return {
          title: t("nav.environment", { defaultValue: "环境信息" }),
          parentKey: "settings" as PageKey,
          parentLabel: t("navigation.settings", { defaultValue: "设置" }),
        };
      case "localProxy":
        return {
          title: t("nav.localProxy", { defaultValue: "本地代理" }),
          parentKey: "settings" as PageKey,
          parentLabel: t("navigation.settings", { defaultValue: "设置" }),
        };
      case "about":
        return {
          title: t("settings.sectionAbout", { defaultValue: "关于" }),
          parentKey: "settings" as PageKey,
          parentLabel: t("navigation.settings", { defaultValue: "设置" }),
        };
      default:
        return null;
    }
  };

  const secondaryMeta = !isPrimaryPage ? getSecondaryHeaderMeta(activeKey) : null;

  return (
    <div
      className="v2-desktop-shell"
      style={{
        height: "100vh",
        width: "100vw",
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        backgroundColor: isDark ? "#11161D" : "#F7F8FA",
        color: isDark ? "#F2F4F7" : "#111827",
        fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif",
      }}
    >
      {/* Integrated V2 Top App Bar (Titlebar + Drag Region + Top Nav + Window Controls) */}
      <header
        style={{
          height: WINDOW_CHROME_HEIGHT,
          minHeight: WINDOW_CHROME_HEIGHT,
          display: "flex",
          alignItems: "stretch",
          justifyContent: "space-between",
          paddingLeft: "16px",
          paddingRight: "0px",
          borderBottom: `1px solid ${isDark ? "#1E2632" : "#E8ECF1"}`,
          backgroundColor: isDark ? "#151B23" : "#FFFFFF",
          userSelect: "none",
          zIndex: 100,
        }}
      >
        {/* Left: App Brand Logo */}
        <div
          data-tauri-drag-region
          onDoubleClick={() => void appWindow.toggleMaximize()}
          style={{
            display: "flex",
            alignItems: "center",
            height: "100%",
            flex: "0 0 auto",
            minWidth: 0,
          }}
        >
          <AppBrand />
        </div>

        {/* Center: Top Segmented Navigation Dock (minWidth:0 so ResizeObserver can detect squeeze) */}
        <div
          data-tauri-drag-region
          onDoubleClick={() => void appWindow.toggleMaximize()}
          className="v2-top-nav-slot"
          style={{
            flex: "1 1 auto",
            minWidth: 0,
            display: "flex",
            justifyContent: "center",
            alignItems: "center",
            height: "100%",
            overflow: "hidden",
            padding: "0 8px",
          }}
        >
          <div className="no-drag" style={{ maxWidth: "100%", minWidth: 0 }}>
            <TopNavigation activeKey={activeKey} onNavigate={onNavigate} />
          </div>
        </div>

        <TitlebarActions updateVersion={updateVersion} onOpenUpdate={onOpenUpdate} />
      </header>

      {/* Secondary Sub-Header for Detail Navigation */}
      {secondaryMeta && (
        <div style={{ padding: "8px 24px 0 24px" }}>
          <ContextHeader
            title={secondaryMeta.title}
            showBack
            onBack={() => onNavigate(secondaryMeta.parentKey)}
            backText={secondaryMeta.parentLabel}
          />
        </div>
      )}

      {/* Main Responsive Content Surface (Max-width 1280px, Centered with breathing room) */}
      <main
        style={{
          flex: 1,
          overflowY: "auto",
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          padding: "20px 24px 28px 24px",
        }}
      >
        <div
          style={{
            width: "100%",
            maxWidth: "1280px",
            flex: 1,
            display: "flex",
            flexDirection: "column",
          }}
        >
          {children}
        </div>
      </main>
    </div>
  );
};
