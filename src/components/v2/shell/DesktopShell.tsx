import React, { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Badge, Select, Tooltip } from "antd";
import { useTranslation } from "react-i18next";
import BulbFilled from "@ant-design/icons/es/icons/BulbFilled";
import BulbOutlined from "@ant-design/icons/es/icons/BulbOutlined";
import CloseOutlined from "@ant-design/icons/es/icons/CloseOutlined";
import FullscreenExitOutlined from "@ant-design/icons/es/icons/FullscreenExitOutlined";
import FullscreenOutlined from "@ant-design/icons/es/icons/FullscreenOutlined";
import GlobalOutlined from "@ant-design/icons/es/icons/GlobalOutlined";
import LaptopOutlined from "@ant-design/icons/es/icons/LaptopOutlined";
import MinusOutlined from "@ant-design/icons/es/icons/MinusOutlined";
import LayoutOutlined from "@ant-design/icons/es/icons/LayoutOutlined";
import { languages } from "@/i18n";
import type { PageKey } from "@/lib/pageRegistry";
import { useAppStore } from "@/stores/appStore";
import { useThemeStore, type ThemeMode } from "@/stores/themeStore";
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

const themeIcons: Record<ThemeMode, React.ReactNode> = {
  light: <BulbOutlined />,
  dark: <BulbFilled />,
  system: <LaptopOutlined />,
};

const PRIMARY_PAGES = new Set<PageKey>([
  "workbench",
  "providers",
  "proxy",
  "usage",
  "antigravity",
  "workspace",
  "settings",
  "about",
]);

export const DesktopShell: React.FC<DesktopShellProps> = ({
  activeKey,
  onNavigate,
  updateVersion,
  onOpenUpdate,
  children,
}) => {
  const { t, i18n } = useTranslation();
  const [maximized, setMaximized] = useState(false);

  const themeMode = useThemeStore((s) => s.mode);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const language = useAppStore((s) => s.language);
  const setLanguage = useAppStore((s) => s.setLanguage);
  const uiMode = useAppStore((s) => s.uiMode);
  const setUiMode = useAppStore((s) => s.setUiMode);

  const isDark = resolvedTheme === "dark";

  useEffect(() => {
    let disposed = false;
    const sync = () => {
      void appWindow.isMaximized().then((v) => {
        if (!disposed) setMaximized(v);
      });
    };
    sync();
    const unlisten = appWindow.onResized(sync);
    return () => {
      disposed = true;
      void unlisten.then((f) => f());
    };
  }, []);

  const isPrimaryPage = PRIMARY_PAGES.has(activeKey);

  const getSecondaryHeaderMeta = (key: PageKey) => {
    switch (key) {
      case "proxy":
        return {
          title: t("navigation.proxy", { defaultValue: "本地代理" }),
          parentKey: "settings" as PageKey,
          parentLabel: t("navigation.settings", { defaultValue: "设置" }),
        };
      case "sessions":
        return {
          title: t("nav.sessions", { defaultValue: "会话管理" }),
          parentKey: "settings" as PageKey,
          parentLabel: t("navigation.settings", { defaultValue: "设置" }),
        };
      case "environment":
        return {
          title: t("nav.environment", { defaultValue: "环境信息" }),
          parentKey: "settings" as PageKey,
          parentLabel: t("navigation.settings", { defaultValue: "设置" }),
        };
      case "localization":
        return {
          title: t("nav.localization", { defaultValue: "汉化与本地化" }),
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
          height: "52px",
          minHeight: "52px",
          display: "flex",
          alignItems: "center",
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
          }}
        >
          <AppBrand />
        </div>

        {/* Center: Top Segmented Navigation Dock */}
        <div
          data-tauri-drag-region
          onDoubleClick={() => void appWindow.toggleMaximize()}
          style={{
            flex: 1,
            display: "flex",
            justifyContent: "center",
            alignItems: "center",
            height: "100%",
          }}
        >
          <div className="no-drag">
            <TopNavigation activeKey={activeKey} onNavigate={onNavigate} />
          </div>
        </div>

        {/* Right: Actions, Theme, Language, UI Mode Toggle, Window Controls */}
        <div
          className="no-drag"
          style={{
            display: "flex",
            alignItems: "center",
            height: "100%",
            gap: "8px",
          }}
        >
          {updateVersion ? (
            <Badge dot offset={[-2, 4]}>
              <button
                type="button"
                onClick={onOpenUpdate}
                style={{
                  fontSize: "12px",
                  padding: "3px 8px",
                  borderRadius: "6px",
                  border: "none",
                  cursor: "pointer",
                  background: isDark ? "rgba(59, 130, 246, 0.15)" : "#EFF6FF",
                  color: "#3B82F6",
                  fontWeight: 500,
                }}
              >
                {t("about.appUpdateAvailable", { version: updateVersion })}
              </button>
            </Badge>
          ) : null}

          {/* V1 / V2 Switcher Tooltip */}
          <Tooltip title={`切换系统 UI 模式 (当前: ${uiMode.toUpperCase()})`}>
            <button
              type="button"
              onClick={() => setUiMode(uiMode === "v1" ? "v2" : "v1")}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: "4px",
                padding: "4px 8px",
                borderRadius: "6px",
                border: `1px solid ${isDark ? "#2A3442" : "#E2E8F0"}`,
                backgroundColor: "transparent",
                color: isDark ? "#9CA3AF" : "#64748B",
                fontSize: "12px",
                cursor: "pointer",
              }}
            >
              <LayoutOutlined />
              <span>{uiMode.toUpperCase()}</span>
            </button>
          </Tooltip>

          <Tooltip title={t("common.theme")}>
            <Select<ThemeMode>
              size="small"
              variant="borderless"
              value={themeMode}
              onChange={setThemeMode}
              style={{ width: 96 }}
              suffixIcon={themeIcons[themeMode]}
              options={[
                { value: "light", label: t("common.themeLight") },
                { value: "dark", label: t("common.themeDark") },
                { value: "system", label: t("common.themeSystem") },
              ]}
            />
          </Tooltip>

          <Tooltip title={t("common.language")}>
            <Select
              size="small"
              variant="borderless"
              value={language}
              onChange={(v) => {
                setLanguage(v);
                void i18n.changeLanguage(v);
              }}
              style={{ width: 96 }}
              suffixIcon={<GlobalOutlined />}
              options={languages.map((l) => ({ value: l.value, label: l.label }))}
            />
          </Tooltip>

          {/* Native Window Controls */}
          <div style={{ display: "flex", alignItems: "stretch", height: "100%", marginLeft: "4px" }}>
            <button
              type="button"
              onClick={() => void appWindow.minimize()}
              style={{
                width: "44px",
                height: "100%",
                border: "none",
                background: "transparent",
                color: isDark ? "#9CA3AF" : "#64748B",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                cursor: "pointer",
              }}
            >
              <MinusOutlined />
            </button>
            <button
              type="button"
              onClick={() => void appWindow.toggleMaximize()}
              style={{
                width: "44px",
                height: "100%",
                border: "none",
                background: "transparent",
                color: isDark ? "#9CA3AF" : "#64748B",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                cursor: "pointer",
              }}
            >
              {maximized ? <FullscreenExitOutlined /> : <FullscreenOutlined />}
            </button>
            <button
              type="button"
              onClick={() => void appWindow.close()}
              style={{
                width: "44px",
                height: "100%",
                border: "none",
                background: "transparent",
                color: isDark ? "#9CA3AF" : "#64748B",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                cursor: "pointer",
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.backgroundColor = "#E81123";
                e.currentTarget.style.color = "#FFFFFF";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.backgroundColor = "transparent";
                e.currentTarget.style.color = isDark ? "#9CA3AF" : "#64748B";
              }}
            >
              <CloseOutlined />
            </button>
          </div>
        </div>
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
