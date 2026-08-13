import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Badge, Select, theme, Tooltip } from "antd";
import { useTranslation } from "react-i18next";
import BulbFilled from "@ant-design/icons/es/icons/BulbFilled";
import BulbOutlined from "@ant-design/icons/es/icons/BulbOutlined";
import CloseOutlined from "@ant-design/icons/es/icons/CloseOutlined";
import FullscreenExitOutlined from "@ant-design/icons/es/icons/FullscreenExitOutlined";
import FullscreenOutlined from "@ant-design/icons/es/icons/FullscreenOutlined";
import GlobalOutlined from "@ant-design/icons/es/icons/GlobalOutlined";
import LaptopOutlined from "@ant-design/icons/es/icons/LaptopOutlined";
import MinusOutlined from "@ant-design/icons/es/icons/MinusOutlined";
import { languages } from "@/i18n";
import { useAppStore } from "@/stores/appStore";
import { useThemeStore, type ThemeMode } from "@/stores/themeStore";
import { LayoutModeSwitcher } from "./LayoutModeSwitcher";

const appWindow = getCurrentWindow();

export const WINDOW_CHROME_HEIGHT = 48;

const themeIcons: Record<ThemeMode, React.ReactNode> = {
  light: <BulbOutlined />,
  dark: <BulbFilled />,
  system: <LaptopOutlined />,
};

export interface WindowChromeProps {
  updateVersion?: string | null;
  onOpenUpdate?: () => void;
  extraLeft?: React.ReactNode;
}

export function WindowChrome({ updateVersion, onOpenUpdate, extraLeft }: WindowChromeProps) {
  const { t, i18n } = useTranslation();
  const { token } = theme.useToken();
  const [maximized, setMaximized] = useState(false);

  const themeMode = useThemeStore((s) => s.mode);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const language = useAppStore((s) => s.language);
  const setLanguage = useAppStore((s) => s.setLanguage);

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

  return (
    <div
      className="workspace-window-chrome"
      style={{
        height: WINDOW_CHROME_HEIGHT,
        minHeight: WINDOW_CHROME_HEIGHT,
        display: "flex",
        alignItems: "stretch",
        justifyContent: "space-between",
        paddingLeft: "var(--page-padding-x, 16px)",
        paddingRight: 0,
        backgroundColor: token.colorBgContainer,
        borderBottom: `1px solid ${token.colorBorderSecondary}`,
        userSelect: "none",
        boxSizing: "border-box",
      }}
    >
      {/* Left side drag region + optional breadcrumb/back */}
      <div
        data-tauri-drag-region
        onDoubleClick={(e) => {
          if ((e.target as HTMLElement).closest("button, select, input, .no-drag")) return;
          void appWindow.toggleMaximize();
        }}
        style={{
          flex: 1,
          height: "100%",
          display: "flex",
          alignItems: "center",
          gap: "12px",
          minWidth: 0,
        }}
      >
        {extraLeft}
      </div>

      {/* Right controls: Theme, Language, Updates, Window buttons */}
      <div
        className="no-drag"
        style={{
          display: "flex",
          alignItems: "stretch",
          height: "100%",
          gap: "6px",
          flexShrink: 0,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: "6px", paddingRight: "6px" }}>
          {updateVersion ? (
            <Badge dot offset={[-2, 4]}>
              <button
                type="button"
                className="app-titlebar-update"
                onClick={onOpenUpdate}
                style={{
                  fontSize: "12px",
                  padding: "2px 8px",
                  borderRadius: "4px",
                  border: "none",
                  cursor: "pointer",
                  background: "var(--color-bg-subtle, rgba(0,0,0,0.04))",
                  color: token.colorPrimary,
                }}
              >
                {t("about.appUpdateAvailable", { version: updateVersion })}
              </button>
            </Badge>
          ) : null}

          <LayoutModeSwitcher />

          <Tooltip title={t("common.theme")}>
            <Select<ThemeMode>
              size="small"
              variant="borderless"
              value={themeMode}
              onChange={setThemeMode}
              style={{ width: 36 }}
              popupMatchSelectWidth={false}
              suffixIcon={null}
              labelRender={() => (
                <span style={{ display: "inline-flex", alignItems: "center" }}>{themeIcons[themeMode]}</span>
              )}
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
              style={{ width: 36 }}
              popupMatchSelectWidth={false}
              suffixIcon={null}
              labelRender={() => (
                <span style={{ display: "inline-flex", alignItems: "center" }}>
                  <GlobalOutlined />
                </span>
              )}
              options={languages.map((l) => ({ value: l.value, label: l.label }))}
            />
          </Tooltip>
        </div>

        {/* Windows Standard Frameless Window Controls */}
        <div
          className="app-titlebar-controls"
          style={{
            display: "flex",
            alignItems: "stretch",
            height: "100%",
          }}
        >
          <Tooltip title={t("common.minimize", { defaultValue: "最小化" })} placement="bottom" mouseEnterDelay={0.3}>
            <button
              type="button"
              onClick={() => void appWindow.minimize()}
              style={{
                width: "48px",
                height: "100%",
                border: "none",
                borderRadius: 0,
                background: "transparent",
                color: "var(--color-text-secondary)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                fontSize: "13px",
                cursor: "pointer",
                transition: "background-color 0.15s ease, color 0.15s ease",
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.backgroundColor = "color-mix(in srgb, var(--color-text-primary, #000) 14%, transparent)";
                e.currentTarget.style.color = "var(--color-text-primary)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.backgroundColor = "transparent";
                e.currentTarget.style.color = "var(--color-text-secondary)";
              }}
              onMouseDown={(e) => {
                e.currentTarget.style.backgroundColor = "color-mix(in srgb, var(--color-text-primary, #000) 22%, transparent)";
              }}
              onMouseUp={(e) => {
                e.currentTarget.style.backgroundColor = "color-mix(in srgb, var(--color-text-primary, #000) 14%, transparent)";
              }}
            >
              <MinusOutlined />
            </button>
          </Tooltip>

          <Tooltip
            title={maximized ? t("common.restore", { defaultValue: "向下还原" }) : t("common.maximize", { defaultValue: "最大化" })}
            placement="bottom"
            mouseEnterDelay={0.3}
          >
            <button
              type="button"
              onClick={() => void appWindow.toggleMaximize()}
              style={{
                width: "48px",
                height: "100%",
                border: "none",
                borderRadius: 0,
                background: "transparent",
                color: "var(--color-text-secondary)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                fontSize: "13px",
                cursor: "pointer",
                transition: "background-color 0.15s ease, color 0.15s ease",
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.backgroundColor = "color-mix(in srgb, var(--color-text-primary, #000) 14%, transparent)";
                e.currentTarget.style.color = "var(--color-text-primary)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.backgroundColor = "transparent";
                e.currentTarget.style.color = "var(--color-text-secondary)";
              }}
              onMouseDown={(e) => {
                e.currentTarget.style.backgroundColor = "color-mix(in srgb, var(--color-text-primary, #000) 22%, transparent)";
              }}
              onMouseUp={(e) => {
                e.currentTarget.style.backgroundColor = "color-mix(in srgb, var(--color-text-primary, #000) 14%, transparent)";
              }}
            >
              {maximized ? <FullscreenExitOutlined /> : <FullscreenOutlined />}
            </button>
          </Tooltip>

          <Tooltip title={t("common.close", { defaultValue: "关闭" })} placement="bottom" mouseEnterDelay={0.3}>
            <button
              type="button"
              className="app-titlebar-close"
              onClick={() => void appWindow.close()}
              style={{
                width: "48px",
                height: "100%",
                border: "none",
                borderRadius: 0,
                background: "transparent",
                color: "var(--color-text-secondary)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                fontSize: "13px",
                cursor: "pointer",
                transition: "background-color 0.15s ease, color 0.15s ease",
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.backgroundColor = "#e81123";
                e.currentTarget.style.color = "#ffffff";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.backgroundColor = "transparent";
                e.currentTarget.style.color = "var(--color-text-secondary)";
              }}
              onMouseDown={(e) => {
                e.currentTarget.style.backgroundColor = "#c50e1f";
              }}
              onMouseUp={(e) => {
                e.currentTarget.style.backgroundColor = "#e81123";
              }}
            >
              <CloseOutlined />
            </button>
          </Tooltip>
        </div>
      </div>
    </div>
  );
}
