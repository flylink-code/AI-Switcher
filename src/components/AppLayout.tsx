import { useMemo } from "react";
import { Layout, Menu, Select, Space, Tooltip, Typography, theme } from "antd";
import ApiOutlined from "@ant-design/icons/es/icons/ApiOutlined";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import BulbFilled from "@ant-design/icons/es/icons/BulbFilled";
import BulbOutlined from "@ant-design/icons/es/icons/BulbOutlined";
import ClusterOutlined from "@ant-design/icons/es/icons/ClusterOutlined";
import DesktopOutlined from "@ant-design/icons/es/icons/DesktopOutlined";
import FileTextOutlined from "@ant-design/icons/es/icons/FileTextOutlined";
import GlobalOutlined from "@ant-design/icons/es/icons/GlobalOutlined";
import InfoCircleOutlined from "@ant-design/icons/es/icons/InfoCircleOutlined";
import LaptopOutlined from "@ant-design/icons/es/icons/LaptopOutlined";
import NodeIndexOutlined from "@ant-design/icons/es/icons/NodeIndexOutlined";
import ThunderboltOutlined from "@ant-design/icons/es/icons/ThunderboltOutlined";
import { useTranslation } from "react-i18next";
import { useThemeStore, type ThemeMode } from "@/stores/themeStore";
import { useAppStore } from "@/stores/appStore";
import { languages } from "@/i18n";
import type { PageKey } from "@/lib/pageRegistry";

const { Sider, Header, Content } = Layout;

export interface NavItem {
  key: PageKey;
  icon: React.ReactNode;
}

export const NAV_ITEMS: NavItem[] = [
  { key: "providers", icon: <ApiOutlined /> },
  { key: "proxy", icon: <NodeIndexOutlined /> },
  { key: "mcp", icon: <ClusterOutlined /> },
  { key: "prompts", icon: <FileTextOutlined /> },
  { key: "skills", icon: <ThunderboltOutlined /> },
  { key: "usage", icon: <BarChartOutlined /> },
  { key: "localization", icon: <GlobalOutlined /> },
  { key: "environment", icon: <DesktopOutlined /> },
  { key: "about", icon: <InfoCircleOutlined /> },
];

interface AppLayoutProps {
  activeKey: PageKey;
  onNavigate: (key: PageKey) => void;
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
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const language = useAppStore((s) => s.language);
  const setLanguage = useAppStore((s) => s.setLanguage);
  const { token } = theme.useToken();

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
    <Layout style={{ height: "100vh", minWidth: 0, minHeight: 0, overflow: "hidden" }}>
      <Sider
        width={210}
        theme={resolvedTheme === "dark" ? "dark" : "light"}
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
          {t("app.name")}
        </div>
        <Menu
          mode="inline"
          selectedKeys={[activeKey]}
          items={menuItems}
          onClick={({ key }) => onNavigate(key as PageKey)}
          style={{ borderInlineEnd: "none", background: "transparent" }}
        />
      </Sider>
      <Layout style={{ minWidth: 0, minHeight: 0, overflow: "hidden" }}>
        <Header
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
          <Typography.Text type="secondary" ellipsis style={{ minWidth: 0 }}>
            {t("app.tagline")}
          </Typography.Text>
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
        <Content style={{ minWidth: 0, minHeight: 0, overflow: "auto", padding: 24 }}>
          {children}
        </Content>
      </Layout>
    </Layout>
  );
}
