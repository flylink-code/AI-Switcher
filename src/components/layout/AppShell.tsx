import React from "react";
import { useTranslation } from "react-i18next";
import { Layout, theme } from "antd";
import type { PageKey } from "@/lib/pageRegistry";
import { WindowChrome } from "./WindowChrome";
import { SideNav } from "./SideNav";
import { ContextHeader } from "./ContextHeader";

export interface AppShellProps {
  activeKey: PageKey;
  onNavigate: (key: PageKey) => void;
  updateVersion?: string | null;
  onOpenUpdate?: () => void;
  appVersion?: string | null;
  children: React.ReactNode;
}

const PRIMARY_PAGES = new Set<PageKey>([
  "workbench",
  "providers",
  "usage",
  "antigravity",
  "workspace",
  "sessions",
  "settings",
]);

export const AppShell: React.FC<AppShellProps> = ({
  activeKey,
  onNavigate,
  updateVersion,
  onOpenUpdate,
  children,
}) => {
  const { t } = useTranslation();
  const { token } = theme.useToken();

  const isPrimaryPage = PRIMARY_PAGES.has(activeKey);

  // Compute header meta for secondary detail pages only
  const getSecondaryHeaderMeta = (key: PageKey): { title: string; parentKey: PageKey; parentLabel: string } => {
    switch (key) {
      case "proxy":
        return {
          title: t("navigation.proxy", { defaultValue: "本地代理" }),
          parentKey: "settings",
          parentLabel: t("navigation.settings", { defaultValue: "设置" }),
        };
      case "environment":
        return {
          title: t("nav.environment", { defaultValue: "环境信息" }),
          parentKey: "settings",
          parentLabel: t("navigation.settings", { defaultValue: "设置" }),
        };
      case "agentTools":
        return {
          title: t("nav.agentTools", { defaultValue: "Agent 工具" }),
          parentKey: "settings",
          parentLabel: t("navigation.settings", { defaultValue: "设置" }),
        };
      case "localization":
        return {
          title: t("nav.localization", { defaultValue: "汉化与本地化" }),
          parentKey: "settings",
          parentLabel: t("navigation.settings", { defaultValue: "设置" }),
        };
      case "about":
        return {
          title: t("settings.sectionAbout", { defaultValue: "关于" }),
          parentKey: "settings",
          parentLabel: t("navigation.settings", { defaultValue: "设置" }),
        };
      default:
        return {
          title: "",
          parentKey: "workbench",
          parentLabel: t("navigation.dashboard", { defaultValue: "概览" }),
        };
    }
  };

  const secondaryMeta = !isPrimaryPage ? getSecondaryHeaderMeta(activeKey) : null;

  return (
    <div
      className="app-shell"
      style={{
        height: "100vh",
        width: "100vw",
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        backgroundColor: token.colorBgLayout,
        color: token.colorText,
      }}
    >
      <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "row" }}>
        <SideNav activeKey={activeKey} onNavigate={onNavigate} />

        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
          <WindowChrome updateVersion={updateVersion} onOpenUpdate={onOpenUpdate} />

          {secondaryMeta && secondaryMeta.title ? (
            <ContextHeader
              title={secondaryMeta.title}
              showBack
              onBack={() => onNavigate(secondaryMeta.parentKey)}
              backText={secondaryMeta.parentLabel}
            />
          ) : null}

          <Layout.Content
            className="app-content-area"
            style={{
              flex: 1,
              minWidth: 0,
              minHeight: 0,
              overflow: "auto",
              padding: "16px var(--page-padding-x, 16px) var(--page-padding-y, 20px)",
              backgroundColor: token.colorBgLayout,
            }}
          >
            {children}
          </Layout.Content>
        </div>
      </div>
    </div>
  );
};
