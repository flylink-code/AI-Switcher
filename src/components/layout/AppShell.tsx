import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Layout, theme } from "antd";
import type { PageKey } from "@/lib/pageRegistry";
import { TitleBar } from "@/components/TitleBar";
import { Sidebar } from "./Sidebar";
import { ContextHeader } from "./ContextHeader";
import { StatusBar } from "./StatusBar";

const SIDEBAR_COLLAPSED_KEY = "cs.sidebarCollapsed";

export interface AppShellProps {
  activeKey: PageKey;
  onNavigate: (key: PageKey) => void;
  updateVersion?: string | null;
  onOpenUpdate?: () => void;
  appVersion?: string | null;
  children: React.ReactNode;
}

export const AppShell: React.FC<AppShellProps> = ({
  activeKey,
  onNavigate,
  updateVersion,
  onOpenUpdate,
  appVersion,
  children,
}) => {
  const { t } = useTranslation();
  const { token } = theme.useToken();

  const [collapsed, setCollapsed] = useState<boolean>(() => {
    if (typeof localStorage !== "undefined") {
      return localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "true";
    }
    return false;
  });

  useEffect(() => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(collapsed));
    }
  }, [collapsed]);

  const toggleCollapse = () => setCollapsed((prev) => !prev);

  // Compute page title and description according to activeKey
  const getHeaderMeta = (key: PageKey): { title: string; description?: string; showClientSwitcher: boolean } => {
    switch (key) {
      case "workbench":
        return {
          title: t("navigation.dashboard", { defaultValue: "概览" }),
          description: t("workbench.subtitle", { defaultValue: "AI-Switcher 控制中心与状态概览" }),
          showClientSwitcher: true,
        };
      case "providers":
        return {
          title: t("navigation.providers", { defaultValue: "供应商服务" }),
          description: t("providers.subtitle", { defaultValue: "管理当前客户端可用的 API Provider" }),
          showClientSwitcher: true,
        };
      case "proxy":
        return {
          title: t("navigation.proxy", { defaultValue: "代理控制中心" }),
          description: t("proxy.subtitle", { defaultValue: "本地请求路由与自动故障切换控制" }),
          showClientSwitcher: true,
        };
      case "usage":
        return {
          title: t("navigation.usage", { defaultValue: "用量与统计" }),
          description: t("usage.subtitle", { defaultValue: "Token 用量与请求趋势诊断" }),
          // Usage analytics is driven by an explicit Data Source filter, not
          // by the global Current Client context.
          showClientSwitcher: false,
        };
      case "antigravity":
        return {
          title: t("navigation.accounts", { defaultValue: "Accounts & Quotas" }),
          description: t("antigravity.subtitle", { defaultValue: "Google / Antigravity 账号池与额度监控" }),
          showClientSwitcher: false,
        };
      case "workspace":
      case "mcp":
      case "prompts":
      case "skills":
      case "agents":
      case "codexPlugins":
      case "profiles":
        return {
          title: t("navigation.workspace", { defaultValue: "工作区" }),
          description: t("workspace.subtitle", { defaultValue: "项目、MCP、Prompts、Skills 与配置资源管理" }),
          showClientSwitcher: true,
        };
      case "settings":
      case "sessions":
      case "about":
      case "environment":
      case "localization":
        return {
          title: t("navigation.settings", { defaultValue: "设置" }),
          description: t("settings.subtitle", { defaultValue: "系统选项与配置" }),
          showClientSwitcher: false,
        };
      default:
        return {
          title: "AI-Switcher",
          showClientSwitcher: false,
        };
    }
  };

  const headerMeta = getHeaderMeta(activeKey);

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
      {/* Top Custom Window TitleBar */}
      <TitleBar
        showBack={activeKey !== "workbench"}
        onBack={() => onNavigate("workbench")}
        updateVersion={updateVersion}
        onOpenUpdate={onOpenUpdate}
      />

      {/* Main Body with Sidebar + Content */}
      <div style={{ flex: 1, display: "flex", flexDirection: "row", minHeight: 0, overflow: "hidden" }}>
        <Sidebar
          activeKey={activeKey}
          onNavigate={onNavigate}
          collapsed={collapsed}
          onToggleCollapse={toggleCollapse}
        />

        <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0, minHeight: 0 }}>
          <ContextHeader
            title={headerMeta.title}
            description={headerMeta.description}
            showClientSwitcher={headerMeta.showClientSwitcher}
          />

          <Layout.Content
            className="app-content-area"
            style={{
              flex: 1,
              minWidth: 0,
              minHeight: 0,
              overflow: "auto",
              padding: "var(--page-padding-y) var(--page-padding-x)",
              backgroundColor: token.colorBgLayout,
            }}
          >
            {children}
          </Layout.Content>
        </div>
      </div>

      {/* Bottom Fixed Runtime Status Bar */}
      <StatusBar appVersion={appVersion} />
    </div>
  );
};
