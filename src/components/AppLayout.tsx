import { useEffect, useMemo } from "react";
import {
  App as AntApp,
  Badge,
  Button,
  Layout,
  Menu,
  Segmented,
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
import CloudDownloadOutlined from "@ant-design/icons/es/icons/CloudDownloadOutlined";
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
import type { ProviderTarget } from "@/types/backend";

const { Sider, Header, Content } = Layout;

export interface NavItem {
  key: PageKey;
  icon: React.ReactNode;
  group: "core" | "extensions" | "data" | "system";
}

export const NAV_ITEMS: NavItem[] = [
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

export function AppLayout({ activeKey, onNavigate, updateVersion, onOpenUpdate, children }: AppLayoutProps) {
  const { t, i18n } = useTranslation();
  const themeMode = useThemeStore((s) => s.mode);
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const language = useAppStore((s) => s.language);
  const setLanguage = useAppStore((s) => s.setLanguage);
  const workspaceTarget = usePagePreferencesStore((s) => s.workspaceTarget);
  const setWorkspaceTarget = usePagePreferencesStore((s) => s.setWorkspaceTarget);
  const { message } = AntApp.useApp();
  const { token } = theme.useToken();

  const proxyQuery = useQuery(proxyStatusOptions(workspaceTarget));
  const providersQuery = useQuery(providerListOptions(workspaceTarget));
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

  const workspaceOptions: Array<{ label: string; value: ProviderTarget }> = [
    { value: "claude_code", label: t("workspace.claude_code") },
    { value: "claude_desktop", label: t("workspace.claude_desktop") },
    { value: "codex", label: t("workspace.codex") },
  ];

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
          style={{
            height: 56,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: token.colorText,
            fontWeight: 600,
            fontSize: 16,
            borderBottom: `1px solid ${token.colorBorderSecondary}`,
          }}
        >
          {updateVersion ? (
            <Badge dot offset={[-2, 3]}>
              <Button type="link" size="small" icon={<CloudDownloadOutlined />} onClick={onOpenUpdate}>
                {t("app.name")}
              </Button>
            </Badge>
          ) : (
            t("app.name")
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
            flex: "0 0 auto",
            minWidth: 0,
            background: token.colorBgContainer,
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 16,
            paddingInline: 24,
            borderBottom: `1px solid ${token.colorBorderSecondary}`,
          }}
        >
          <Space size={12} wrap style={{ minWidth: 0, flex: 1 }}>
            <Segmented<ProviderTarget>
              size="small"
              value={workspaceTarget}
              options={workspaceOptions}
              onChange={setWorkspaceTarget}
              aria-label={t("workspace.target")}
            />
            <Tooltip title={t("workspace.openProxy")}>
              <Button type="text" size="small" onClick={() => onNavigate("proxy")}>
                <Typography.Text
                  type={proxyQuery.data?.phase === "error" ? "danger" : "secondary"}
                  style={{ fontSize: 12 }}
                >
                  {proxyStatusLabel(t, proxyQuery.data?.phase, proxyQuery.data?.port)}
                </Typography.Text>
              </Button>
            </Tooltip>
            <Typography.Text type="secondary" ellipsis style={{ maxWidth: 280, fontSize: 12 }}>
              {currentProvider
                ? t("workspace.currentProvider", { name: currentProvider.name })
                : t("workspace.noProvider")}
            </Typography.Text>
          </Space>
          <Space>
            <Tooltip title={t("common.theme")}>
              <Select<ThemeMode>
                size="small"
                value={themeMode}
                onChange={setThemeMode}
                style={{ width: 120 }}
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
                style={{ width: 130 }}
                suffixIcon={<GlobalOutlined />}
                options={languages.map((l) => ({ value: l.value, label: l.label }))}
              />
            </Tooltip>
          </Space>
        </Header>
        <Content
          className="app-content"
          style={{ minWidth: 0, minHeight: 0, overflow: "auto", padding: 24, background: token.colorBgLayout }}
        >
          {children}
        </Content>
      </Layout>
    </Layout>
  );
}
