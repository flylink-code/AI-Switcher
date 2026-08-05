import { useEffect, useMemo } from "react";
import {
  App as AntApp,
  Badge,
  Button,
  Layout,
  Menu,
  Select,
  Space,
  Tooltip,
  Typography,
  theme,
} from "antd";
import type { MenuProps } from "antd";
import ApiOutlined from "@ant-design/icons/es/icons/ApiOutlined";
import AppstoreAddOutlined from "@ant-design/icons/es/icons/AppstoreAddOutlined";
import AppstoreOutlined from "@ant-design/icons/es/icons/AppstoreOutlined";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import BulbFilled from "@ant-design/icons/es/icons/BulbFilled";
import BulbOutlined from "@ant-design/icons/es/icons/BulbOutlined";
import ClusterOutlined from "@ant-design/icons/es/icons/ClusterOutlined";
import DashboardOutlined from "@ant-design/icons/es/icons/DashboardOutlined";
import DesktopOutlined from "@ant-design/icons/es/icons/DesktopOutlined";
import FileTextOutlined from "@ant-design/icons/es/icons/FileTextOutlined";
import GlobalOutlined from "@ant-design/icons/es/icons/GlobalOutlined";
import InfoCircleOutlined from "@ant-design/icons/es/icons/InfoCircleOutlined";
import LaptopOutlined from "@ant-design/icons/es/icons/LaptopOutlined";
import MessageOutlined from "@ant-design/icons/es/icons/MessageOutlined";
import NodeIndexOutlined from "@ant-design/icons/es/icons/NodeIndexOutlined";
import TeamOutlined from "@ant-design/icons/es/icons/TeamOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useThemeStore, type ThemeMode } from "@/stores/themeStore";
import { useAppStore } from "@/stores/appStore";
import { languages } from "@/i18n";
import type { PageKey } from "@/lib/pageRegistry";
import { managedAppsRuntimeStatusOptions } from "@/lib/appQueries";
import { setAppLanguage } from "@/services/api";
import { usageSourceIcon } from "@/components/UsageSourceIcons";
import type { ProviderTarget } from "@/types/backend";

const { Sider, Header, Content } = Layout;
const SHELL_HEADER_HEIGHT = 60;

export interface NavItem {
  key: PageKey;
  icon: React.ReactNode;
  group: "core" | "extensions" | "data" | "system";
}

export const NAV_ITEMS: NavItem[] = [
  { key: "overview", icon: <DashboardOutlined />, group: "core" },
  { key: "providers", icon: <ApiOutlined />, group: "core" },
  { key: "profiles", icon: <AppstoreOutlined />, group: "core" },
  { key: "proxy", icon: <NodeIndexOutlined />, group: "core" },
  { key: "mcp", icon: <ClusterOutlined />, group: "extensions" },
  { key: "prompts", icon: <FileTextOutlined />, group: "extensions" },
  { key: "skills", icon: <ThunderboltOutlined />, group: "extensions" },
  { key: "agents", icon: <TeamOutlined />, group: "extensions" },
  { key: "codexPlugins", icon: <AppstoreAddOutlined />, group: "extensions" },
  { key: "sessions", icon: <MessageOutlined />, group: "data" },
  { key: "usage", icon: <BarChartOutlined />, group: "data" },
  { key: "localization", icon: <GlobalOutlined />, group: "system" },
  { key: "environment", icon: <DesktopOutlined />, group: "system" },
  { key: "about", icon: <InfoCircleOutlined />, group: "system" },
];

const NAV_GROUPS: Array<{ id: NavItem["group"]; labelKey: string }> = [
  { id: "core", labelKey: "nav.groupCore" },
  { id: "extensions", labelKey: "nav.groupExtensions" },
  { id: "data", labelKey: "nav.groupData" },
  { id: "system", labelKey: "nav.groupSystem" },
];

const HEADER_RUNTIME_APPS: Array<{
  target: ProviderTarget;
  runningKey: "claudeCode" | "claudeDesktop" | "codex";
  shortLabelKey: string;
}> = [
  { target: "claude_code", runningKey: "claudeCode", shortLabelKey: "workspace.claude_code" },
  { target: "claude_desktop", runningKey: "claudeDesktop", shortLabelKey: "workspace.claude_desktop" },
  { target: "codex", runningKey: "codex", shortLabelKey: "workspace.codex" },
];

interface AppLayoutProps {
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

export function AppLayout({ activeKey, onNavigate, updateVersion, onOpenUpdate, children }: AppLayoutProps) {
  const { t, i18n } = useTranslation();
  const themeMode = useThemeStore((s) => s.mode);
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const language = useAppStore((s) => s.language);
  const setLanguage = useAppStore((s) => s.setLanguage);
  const { message } = AntApp.useApp();
  const { token } = theme.useToken();

  const runtimeQuery = useQuery(managedAppsRuntimeStatusOptions);

  useEffect(() => {
    void setAppLanguage(language).catch(() => {
      void message.warning(t("common.trayLanguageSyncFailed"));
    });
  }, [language, message, t]);

  const menuItems = useMemo<MenuProps["items"]>(() => {
    const visible = NAV_ITEMS.filter((it) => it.key !== "localization" || language === "zh-CN");
    return NAV_GROUPS.map((group) => ({
      type: "group" as const,
      key: `group-${group.id}`,
      label: t(group.labelKey),
      children: visible
        .filter((it) => it.group === group.id)
        .map((it) => ({
          key: it.key,
          icon: it.icon,
          label: t(`nav.${it.key}`),
        })),
    })).filter((group) => (group.children?.length ?? 0) > 0);
  }, [language, t]);

  return (
    <Layout style={{ height: "100vh", minWidth: 0, minHeight: 0, overflow: "hidden" }}>
      <Sider
        width={220}
        theme={resolvedTheme === "dark" ? "dark" : "light"}
        className="app-sider"
        style={{
          height: "100vh",
          minHeight: 0,
          overflow: "auto",
          borderInlineEnd: `1px solid ${token.colorBorderSecondary}`,
        }}
      >
        <div
          className="app-brand"
          style={{
            height: SHELL_HEADER_HEIGHT,
            minHeight: SHELL_HEADER_HEIGHT,
            maxHeight: SHELL_HEADER_HEIGHT,
            boxSizing: "border-box",
            display: "flex",
            alignItems: "center",
            justifyContent: "flex-start",
            gap: 10,
            paddingInline: 16,
            flexShrink: 0,
            color: token.colorText,
            fontWeight: 600,
            fontSize: 15,
            borderBottom: `1px solid ${token.colorBorderSecondary}`,
          }}
        >
          <span className="app-brand-mark" aria-hidden>
            AS
          </span>
          {updateVersion ? (
            <Badge dot offset={[-2, 3]}>
              <Button type="link" size="small" className="app-brand-name" onClick={onOpenUpdate}>
                {t("app.name")}
              </Button>
            </Badge>
          ) : (
            <span className="app-brand-name">{t("app.name")}</span>
          )}
        </div>
        <Menu
          mode="inline"
          theme={resolvedTheme === "dark" ? "dark" : "light"}
          selectedKeys={[activeKey]}
          items={menuItems}
          onClick={({ key }) => {
            if (key.startsWith("group-")) return;
            onNavigate(key as PageKey);
          }}
          style={{ borderInlineEnd: "none", background: "transparent" }}
        />
      </Sider>
      <Layout style={{ minWidth: 0, minHeight: 0, overflow: "hidden" }}>
        <Header
          className="app-header"
          style={{
            height: SHELL_HEADER_HEIGHT,
            minHeight: SHELL_HEADER_HEIGHT,
            maxHeight: SHELL_HEADER_HEIGHT,
            lineHeight: "normal",
            boxSizing: "border-box",
            flex: "0 0 auto",
            minWidth: 0,
            background: "transparent",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 16,
            paddingBlock: 0,
            paddingInline: 24,
            overflow: "hidden",
            borderBottom: "none",
          }}
        >
          <div className="app-header-main">
            <div className="app-header-status" role="status" aria-label={t("workspace.openEnvironment")}>
              {HEADER_RUNTIME_APPS.map((app, index) => {
                const name = t(app.shortLabelKey);
                const running = Boolean(runtimeQuery.data?.[app.runningKey]);
                let shortLabel: string;
                switch (app.target) {
                  case "claude_code":
                    shortLabel = "Code";
                    break;
                  case "claude_desktop":
                    shortLabel = "Desktop";
                    break;
                  case "codex":
                    shortLabel = "Codex";
                    break;
                  default: {
                    const _exhaustive: never = app.target;
                    shortLabel = _exhaustive;
                    break;
                  }
                }
                return (
                  <span key={app.target} className="app-header-runtime-item">
                    {index > 0 ? (
                      <span className="app-header-status-sep" aria-hidden>
                        ·
                      </span>
                    ) : null}
                    <Tooltip
                      title={
                        running
                          ? t("workspace.appRunning", { name })
                          : t("workspace.appStopped", { name })
                      }
                    >
                      <Button
                        type="text"
                        size="small"
                        className="app-header-status-btn"
                        onClick={() => onNavigate("environment")}
                      >
                        <Badge status={running ? "success" : "default"} />
                        <span className="app-header-target-icon" aria-hidden>
                          {usageSourceIcon(app.target, { size: 14 })}
                        </span>
                        <Typography.Text
                          style={{
                            fontSize: 12,
                            fontWeight: running ? 600 : 500,
                            color: running ? token.colorText : token.colorTextTertiary,
                          }}
                        >
                          {shortLabel}
                        </Typography.Text>
                      </Button>
                    </Tooltip>
                  </span>
                );
              })}
            </div>
          </div>
          <Space size={8} style={{ flexShrink: 0 }}>
            <Tooltip title={t("common.theme")}>
              <Select<ThemeMode>
                size="small"
                value={themeMode}
                onChange={setThemeMode}
                style={{ width: 108 }}
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
                value={language}
                onChange={(v) => {
                  setLanguage(v);
                  void i18n.changeLanguage(v);
                }}
                style={{ width: 118 }}
                suffixIcon={<GlobalOutlined />}
                options={languages.map((l) => ({ value: l.value, label: l.label }))}
              />
            </Tooltip>
          </Space>
        </Header>
        <Content
          className="app-content"
          style={{ minWidth: 0, minHeight: 0, overflow: "auto", padding: "20px 28px 28px", background: token.colorBgLayout }}
        >
          {children}
        </Content>
      </Layout>
    </Layout>
  );
}
