import { useEffect, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Select, Tooltip } from "antd";
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
import { TitlebarVersionButton } from "./TitlebarVersionButton";

const appWindow = getCurrentWindow();

const themeIcons: Record<ThemeMode, ReactNode> = {
  light: <BulbOutlined />,
  dark: <BulbFilled />,
  system: <LaptopOutlined />,
};

export interface TitlebarActionsProps {
  updateVersion?: string | null;
  onOpenUpdate?: () => void;
}

/**
 * Right side of the frameless title bar.
 * Version stays quiet; layout / theme / language share one tray;
 * window caption buttons sit in their own group, flush to the edge.
 */
export function TitlebarActions({ updateVersion, onOpenUpdate }: TitlebarActionsProps) {
  const { t, i18n } = useTranslation();
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

  const minimizeLabel = t("common.minimize", { defaultValue: "最小化" });
  const maximizeLabel = maximized
    ? t("common.restore", { defaultValue: "向下还原" })
    : t("common.maximize", { defaultValue: "最大化" });
  const closeLabel = t("common.close", { defaultValue: "关闭" });

  return (
    <div className="titlebar-actions no-drag">
      <div className="titlebar-tools">
        <TitlebarVersionButton updateVersion={updateVersion} onOpenUpdate={onOpenUpdate} />

        <div className="titlebar-tool-tray" role="toolbar" aria-label={t("common.titlebarTools", { defaultValue: "标题栏工具" })}>
          <LayoutModeSwitcher className="titlebar-layout-switch" />
          <span className="titlebar-tool-sep" aria-hidden="true" />

          <Tooltip title={t("common.theme")}>
            <Select<ThemeMode>
              className="titlebar-icon-select"
              size="small"
              variant="borderless"
              value={themeMode}
              onChange={setThemeMode}
              popupMatchSelectWidth={false}
              suffixIcon={null}
              aria-label={t("common.theme")}
              labelRender={() => (
                <span className="titlebar-glyph">{themeIcons[themeMode]}</span>
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
              className="titlebar-icon-select"
              size="small"
              variant="borderless"
              value={language}
              onChange={(v) => {
                setLanguage(v);
                void i18n.changeLanguage(v);
              }}
              popupMatchSelectWidth={false}
              suffixIcon={null}
              aria-label={t("common.language")}
              labelRender={() => (
                <span className="titlebar-glyph">
                  <GlobalOutlined />
                </span>
              )}
              options={languages.map((l) => ({ value: l.value, label: l.label }))}
            />
          </Tooltip>
        </div>
      </div>

      <div className="app-titlebar-controls">
        <Tooltip title={minimizeLabel} placement="bottom" mouseEnterDelay={0.3}>
          <button type="button" aria-label={minimizeLabel} onClick={() => void appWindow.minimize()}>
            <MinusOutlined />
          </button>
        </Tooltip>
        <Tooltip title={maximizeLabel} placement="bottom" mouseEnterDelay={0.3}>
          <button type="button" aria-label={maximizeLabel} onClick={() => void appWindow.toggleMaximize()}>
            {maximized ? <FullscreenExitOutlined /> : <FullscreenOutlined />}
          </button>
        </Tooltip>
        <Tooltip title={closeLabel} placement="bottom" mouseEnterDelay={0.3}>
          <button
            type="button"
            className="app-titlebar-close"
            aria-label={closeLabel}
            onClick={() => void appWindow.close()}
          >
            <CloseOutlined />
          </button>
        </Tooltip>
      </div>
    </div>
  );
}
