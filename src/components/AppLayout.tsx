import { useMemo } from "react";
import { Layout, Menu, Select, Space, Tooltip, Typography } from "antd";
import {
  ApiOutlined,
  ClusterOutlined,
  FileTextOutlined,
  ThunderboltOutlined,
  BarChartOutlined,
  DesktopOutlined,
  BulbOutlined,
  BulbFilled,
  LaptopOutlined,
  GlobalOutlined,
  NodeIndexOutlined,
} from "@ant-design/icons";
import { useTranslation } from "react-i18next";
import { useThemeStore, type ThemeMode } from "@/stores/themeStore";
import { useAppStore } from "@/stores/appStore";
import { languages } from "@/i18n";

const { Sider, Header, Content } = Layout;

export interface NavItem {
  key: string;
  icon: React.ReactNode;
}

export const NAV_ITEMS: NavItem[] = [
  { key: "providers", icon: <ApiOutlined /> },
  { key: "proxy", icon: <NodeIndexOutlined /> },
  { key: "mcp", icon: <ClusterOutlined /> },
  { key: "prompts", icon: <FileTextOutlined /> },
  { key: "skills", icon: <ThunderboltOutlined /> },
  { key: "usage", icon: <BarChartOutlined /> },
  { key: "environment", icon: <DesktopOutlined /> },
];

interface AppLayoutProps {
  activeKey: string;
  onNavigate: (key: string) => void;
  children: React.ReactNode;
}

const themeIcons: Record<ThemeMode, React.ReactNode> = {
  light: <BulbOutlined />,
  dark: <BulbFilled />,
  system: <LaptopOutlined />,
};

export function AppLayout({ activeKey, onNavigate, children }: AppLayoutProps) {
  const { t, i18n } = useTranslation();
  const themeMode = useThemeStore((s) => s.mode);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const language = useAppStore((s) => s.language);
  const setLanguage = useAppStore((s) => s.setLanguage);

  const menuItems = useMemo(
    () =>
      NAV_ITEMS.map((it) => ({
        key: it.key,
        icon: it.icon,
        label: t(`nav.${it.key}`),
      })),
    [t],
  );

  return (
    <Layout style={{ height: "100vh" }}>
      <Sider width={210} theme={useThemeStore((s) => s.resolved) === "dark" ? "dark" : "light"}>
        <div
          style={{
            height: 56,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--ant-color-text)",
            fontWeight: 600,
            fontSize: 16,
            borderBottom: "1px solid var(--ant-color-border-secondary)",
          }}
        >
          {t("app.name")}
        </div>
        <Menu
          mode="inline"
          selectedKeys={[activeKey]}
          items={menuItems}
          onClick={({ key }) => onNavigate(key)}
          style={{ borderInlineEnd: "none" }}
        />
      </Sider>
      <Layout>
        <Header
          style={{
            background: "var(--ant-color-bg-container)",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            paddingInline: 24,
            borderBottom: "1px solid var(--ant-color-border-secondary)",
          }}
        >
          <Typography.Text type="secondary">{t("app.tagline")}</Typography.Text>
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
        <Content style={{ overflow: "auto", padding: 24 }}>{children}</Content>
      </Layout>
    </Layout>
  );
}
