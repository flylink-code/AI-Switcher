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
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { languages } from "@/i18n";
import type { PageKey } from "@/lib/pageRegistry";
import { providerListOptions, proxyStatusOptions } from "@/lib/appQueries";
import { setAppLanguage } from "@/services/api";
import { usageSourceIcon } from "@/components/UsageSourceIcons";

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

function proxyStatusLabel(
  t: (key: string, options?: Record<string, unknown>) => string,
  phase: string | undefined,
  port: number | undefined,
): string {
  switch (phase) {
    case "running":
      return t("workspace.proxyRunning", { port: port ?? "—" });
    case "starting":
      return t("workspace.proxyStarting");
    case "error":
      return t("workspace.proxyError");
    case "stopped":
    default:
      return t("workspace.proxyStopped");
  }
}

function proxyBadgeStatus(phase: string | undefined): "success" | "processing" | "error" | "default" {
  switch (phase) {
    case "running":
      return "success";
    case "starting":
      return "processing";
    case "error":
      return "error";
    case "stopped":
    default:
      return "default";
  }
}

export function AppLayout({ activeKey, onNavigate, updateVersion, onOpenUpdate, children }: AppLayoutProps) {
  const { t, i18n } = useTranslation();
  const themeMode = useThemeStore((s) => s.mode);
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const language = useAppStore((s) => s.language);
  const setLanguage = useAppStore((s) => s.setLanguage);
  const providersTarget = usePagePreferencesStore((s) => s.providersTarget);
  const proxyTarget = usePagePreferencesStore((s) => s.proxyTarget);
  const { message } = AntApp.useApp();
  const { token } = theme.useToken();

  const proxyQuery = useQuery(proxyStatusOptions(proxyTarget));
  const providersQuery = useQuery(providerListOptions(providersTarget));
  const currentProvider = providersQuery.data?.find((provider) => provider.isCurrent);

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

  let providersTargetLabel: string;
  switch (providersTarget) {
    case "claude_code":
      providersTargetLabel = t("workspace.claude_code");
      break;
    case "claude_desktop":
      providersTargetLabel = t("workspace.claude_desktop");
      break;
    case "codex":
      providersTargetLabel = t("workspace.codex");
      break;
    default: {
      const _exhaustive: never = providersTarget;
      providersTargetLabel = _exhaustive;
      break;
    }
  }

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
            <div className="app-header-status">
              <Tooltip title={t("workspace.openProxy")}>
                <Button
                  type="text"
                  size="small"
                  className="app-header-status-btn"
                  onClick={() => onNavigate("proxy")}
                >
                  <Badge status={proxyBadgeStatus(proxyQuery.data?.phase)} />
                  <Typography.Text
                    type={proxyQuery.data?.phase === "error" ? "danger" : "secondary"}
                    style={{ fontSize: 12 }}
                  >
                    {proxyStatusLabel(t, proxyQuery.data?.phase, proxyQuery.data?.port)}
                  </Typography.Text>
                </Button>
              </Tooltip>
              <span className="app-header-status-sep" aria-hidden>
                ·
              </span>
              <Tooltip
                title={
                  currentProvider
                    ? `${providersTargetLabel} · ${t("workspace.currentProvider", { name: currentProvider.name })}`
                    : `${providersTargetLabel} · ${t("workspace.noProvider")}`
                }
              >
                <Button
                  type="text"
                  size="small"
                  className="app-header-status-btn app-header-provider"
                  onClick={() => onNavigate("providers")}
                >
                  <span className="app-header-target-icon" aria-hidden>
                    {usageSourceIcon(providersTarget, { size: 14 })}
                  </span>
                  <Typography.Text type="secondary" style={{ fontSize: 12, flexShrink: 0 }}>
                    {t("nav.providers")}
                  </Typography.Text>
                  <Typography.Text
                    ellipsis
                    style={{
                      fontSize: 12,
                      fontWeight: 600,
                      maxWidth: 160,
                      color: currentProvider ? token.colorText : token.colorTextTertiary,
                    }}
                  >
                    {currentProvider?.name ?? t("workspace.noProvider")}
                  </Typography.Text>
                </Button>
              </Tooltip>
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
