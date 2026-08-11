import React from "react";
import { useTranslation } from "react-i18next";
import { Layout, theme } from "antd";
import type { PageKey } from "@/lib/pageRegistry";
import { TitleBar } from "@/components/TitleBar";
import { SideNav } from "./SideNav";
import { ContextHeader } from "./ContextHeader";
import { StatusBar } from "./StatusBar";
import { LABEL_KEYS } from "@/components/AgentTargetSwitcher";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
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
  const providersTarget = usePagePreferencesStore((s) => s.providersTarget);
  const proxyTarget = usePagePreferencesStore((s) => s.proxyTarget);

  // Compute page title and description according to activeKey.
  // Agent-scoped pages name their own target in the description; each owns an
  // independent persisted target (no global Agent context anymore).
  const getHeaderMeta = (key: PageKey): { title: string; description?: string } => {
    switch (key) {
      case "workbench":
        return {
          title: t("navigation.dashboard", { defaultValue: "概览" }),
          description: t("workbench.subtitle", { defaultValue: "AI-Switcher 控制中心与状态概览" }),
        };
      case "providers":
        return {
          title: t("navigation.providers", { defaultValue: "供应商服务" }),
          description: t("providers.subtitleFor", {
            client: t(LABEL_KEYS[providersTarget]),
            defaultValue: t("providers.subtitle", { defaultValue: "管理当前客户端可用的 API Provider" }),
          }),
        };
      case "proxy":
        return {
          title: t("navigation.proxy", { defaultValue: "代理控制中心" }),
          description: t("proxy.subtitleFor", {
            client: t(LABEL_KEYS[proxyTarget]),
            defaultValue: t("proxy.subtitle", { defaultValue: "本地请求路由与自动故障切换控制" }),
          }),
        };
      case "usage":
        return {
          title: t("navigation.usage", { defaultValue: "用量与统计" }),
          description: t("usage.subtitle", { defaultValue: "Token 用量与请求趋势诊断" }),
        };
      case "antigravity":
        return {
          title: t("navigation.accounts", { defaultValue: "Accounts & Quotas" }),
          description: t("antigravity.subtitle", { defaultValue: "Google / Antigravity 账号池与额度监控" }),
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
        };
      case "settings":
      case "sessions":
      case "about":
      case "environment":
      case "localization":
        return {
          title: t("navigation.settings", { defaultValue: "设置" }),
          description: t("settings.subtitle", { defaultValue: "系统选项与配置" }),
        };
      default:
        return {
          title: "AI-Switcher",
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

      {/* Body: left primary sidebar + right column (header + content) */}
      <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "row" }}>
        <SideNav
          activeKey={activeKey}
          onNavigate={onNavigate}
        />

        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
          <ContextHeader
            title={headerMeta.title}
            description={headerMeta.description}
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
