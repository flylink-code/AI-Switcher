import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Badge, Select, theme, Tooltip } from "antd";
import { useTranslation } from "react-i18next";
import ArrowLeftOutlined from "@ant-design/icons/es/icons/ArrowLeftOutlined";
import BulbFilled from "@ant-design/icons/es/icons/BulbFilled";
import BulbOutlined from "@ant-design/icons/es/icons/BulbOutlined";
import CloseOutlined from "@ant-design/icons/es/icons/CloseOutlined";
import FullscreenExitOutlined from "@ant-design/icons/es/icons/FullscreenExitOutlined";
import FullscreenOutlined from "@ant-design/icons/es/icons/FullscreenOutlined";
import GlobalOutlined from "@ant-design/icons/es/icons/GlobalOutlined";
import LaptopOutlined from "@ant-design/icons/es/icons/LaptopOutlined";
import MinusOutlined from "@ant-design/icons/es/icons/MinusOutlined";
import SettingOutlined from "@ant-design/icons/es/icons/SettingOutlined";
import { languages } from "@/i18n";
import { useNavigatePage } from "@/lib/navigation";
import { useAppStore } from "@/stores/appStore";
import { useThemeStore, type ThemeMode } from "@/stores/themeStore";
import appLogo from "@/assets/app-logo.png";

const appWindow = getCurrentWindow();

export const TITLE_BAR_HEIGHT = 38;

const themeIcons: Record<ThemeMode, React.ReactNode> = {
  light: <BulbOutlined />,
  dark: <BulbFilled />,
  system: <LaptopOutlined />,
};

/**
 * Custom frameless title bar (VS Code / Discord style).
 * Layout: [drag region with app name] [theme/language selects] [window controls].
 * The selects deliberately live OUTSIDE the drag region so their dropdowns
 * are not swallowed by the window-drag gesture.
 */
interface TitleBarProps {
  showBack?: boolean;
  onBack?: () => void;
  updateVersion?: string | null;
  onOpenUpdate?: () => void;
}

export function TitleBar({ showBack, onBack, updateVersion, onOpenUpdate }: TitleBarProps) {
  const { t, i18n } = useTranslation();
  const { token } = theme.useToken();
  const [maximized, setMaximized] = useState(false);

  const themeMode = useThemeStore((s) => s.mode);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const navigate = useNavigatePage();
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
      className="app-titlebar"
      style={{
        height: TITLE_BAR_HEIGHT,
        minHeight: TITLE_BAR_HEIGHT,
        background: token.colorBgContainer,
        borderBottom: `1px solid ${token.colorBorderSecondary}`,
        color: token.colorText,
      }}
    >
      {showBack ? (
        <Tooltip title={t("settings.back")}>
          <button
            type="button"
            className="app-titlebar-icon-btn app-titlebar-back"
            aria-label={t("settings.back")}
            onClick={onBack}
          >
            <ArrowLeftOutlined />
          </button>
        </Tooltip>
      ) : null}
      <div
        className="app-titlebar-drag"
        data-tauri-drag-region
        onDoubleClick={(e) => {
          if ((e.target as HTMLElement).closest(".app-titlebar-controls")) return;
          void appWindow.toggleMaximize();
        }}
      >
        <div className="app-titlebar-brand">
          <img src={appLogo} alt="AI-Switcher" className="app-titlebar-logo" />
          <span className="app-titlebar-title">{t("app.name")}</span>
        </div>
        {updateVersion ? (
          <Badge dot offset={[-2, 4]}>
            <button
              type="button"
              className="app-titlebar-update"
              onClick={onOpenUpdate}
            >
              {t("about.appUpdateAvailable", { version: updateVersion })}
            </button>
          </Badge>
        ) : null}
      </div>
      <div className="app-titlebar-settings">
        <Tooltip title={t("settings.title")}>
          <button
            type="button"
            className="app-titlebar-icon-btn"
            aria-label={t("settings.title")}
            onClick={() => navigate("settings")}
          >
            <SettingOutlined />
          </button>
        </Tooltip>
        <Tooltip title={t("common.theme")}>
          <Select<ThemeMode>
            size="small"
            variant="borderless"
            value={themeMode}
            onChange={setThemeMode}
            style={{ width: 104 }}
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
            style={{ width: 110 }}
            suffixIcon={<GlobalOutlined />}
            options={languages.map((l) => ({ value: l.value, label: l.label }))}
          />
        </Tooltip>
      </div>
      <div className="app-titlebar-controls">
        <button
          type="button"
          aria-label={t("common.minimize")}
          onClick={() => void appWindow.minimize()}
        >
          <MinusOutlined />
        </button>
        <button
          type="button"
          aria-label={t("common.maximize")}
          onClick={() => void appWindow.toggleMaximize()}
        >
          {maximized ? <FullscreenExitOutlined /> : <FullscreenOutlined />}
        </button>
        <button
          type="button"
          className="app-titlebar-close"
          aria-label={t("common.close")}
          onClick={() => void appWindow.close()}
        >
          <CloseOutlined />
        </button>
      </div>
    </div>
  );
}
