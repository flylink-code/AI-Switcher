import { useEffect } from "react";
import { App as AntApp, Layout, theme } from "antd";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/appStore";
import type { PageKey } from "@/lib/pageRegistry";
import { setAppLanguage } from "@/services/api";
import { TitleBar } from "@/components/TitleBar";
import appLogo from "@/assets/app-logo.png";

const { Content } = Layout;

interface AppLayoutProps {
  activeKey: PageKey;
  onNavigate: (key: PageKey) => void;
  updateVersion?: string | null;
  onOpenUpdate?: () => void;
  children: React.ReactNode;
}

/**
 * Sidebar-free shell: custom title bar on top, single content area below.
 * Navigation lives in the workbench (home), the title-bar back button,
 * and the settings view behind the gear icon.
 */
export function AppLayout({ activeKey, onNavigate, updateVersion, onOpenUpdate, children }: AppLayoutProps) {
  const { t } = useTranslation();
  const language = useAppStore((s) => s.language);
  const { message } = AntApp.useApp();
  const { token } = theme.useToken();

  useEffect(() => {
    void setAppLanguage(language).catch(() => {
      void message.warning(t("common.trayLanguageSyncFailed"));
    });
  }, [language, message, t]);

  return (
    <div style={{ height: "100vh", display: "flex", flexDirection: "column", overflow: "hidden" }}>
      <TitleBar
        showBack={activeKey !== "workbench"}
        onBack={() => onNavigate("workbench")}
        updateVersion={updateVersion}
        onOpenUpdate={onOpenUpdate}
      />
      <Content
        className="app-content"
        style={{
          flex: 1,
          minWidth: 0,
          minHeight: 0,
          overflow: "auto",
          padding: "20px 24px 24px",
          background: token.colorBgLayout,
        }}
      >
        {children}
      </Content>
      <footer className="app-status-footer">
        <div className="app-status-footer-left">
          <img src={appLogo} alt="AI-Switcher" className="app-status-logo" />
          <span className="app-status-title">AI-Switcher</span>
          <span className="app-status-divider">·</span>
          <span>{t("workbench.running", { defaultValue: "运行中" })}</span>
        </div>
        <div className="app-status-footer-right">
          <span>v1.1.0</span>
        </div>
      </footer>
    </div>
  );
}
