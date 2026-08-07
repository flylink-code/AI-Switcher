import { useEffect, useState, type ComponentType } from "react";
import { Layout, Menu, Spin, Typography, theme } from "antd";
import { useTranslation } from "react-i18next";
import ControlOutlined from "@ant-design/icons/es/icons/ControlOutlined";
import ApiOutlined from "@ant-design/icons/es/icons/ApiOutlined";
import FileTextOutlined from "@ant-design/icons/es/icons/FileTextOutlined";
import ToolOutlined from "@ant-design/icons/es/icons/ToolOutlined";
import RobotOutlined from "@ant-design/icons/es/icons/RobotOutlined";
import BlockOutlined from "@ant-design/icons/es/icons/BlockOutlined";
import HistoryOutlined from "@ant-design/icons/es/icons/HistoryOutlined";
import TranslationOutlined from "@ant-design/icons/es/icons/TranslationOutlined";
import DesktopOutlined from "@ant-design/icons/es/icons/DesktopOutlined";
import InfoCircleOutlined from "@ant-design/icons/es/icons/InfoCircleOutlined";
import {
  getLoadedPage,
  preloadPage,
  type PageKey,
} from "@/lib/pageRegistry";
import { useAppStore } from "@/stores/appStore";

const { Sider, Content } = Layout;

/**
 * Low-frequency configuration pages embedded in the settings view.
 * Order = display order in the left sub-navigation.
 * Antigravity is a top-level page (like proxy), not embedded here.
 */
const SETTINGS_PAGES: PageKey[] = [
  "profiles",
  "mcp",
  "prompts",
  "skills",
  "agents",
  "codexPlugins",
  "sessions",
  "localization",
  "environment",
  "about",
];

const SETTINGS_ICONS: Record<PageKey, React.ReactNode> = {
  profiles: <ControlOutlined />,
  mcp: <ApiOutlined />,
  prompts: <FileTextOutlined />,
  skills: <ToolOutlined />,
  agents: <RobotOutlined />,
  codexPlugins: <BlockOutlined />,
  sessions: <HistoryOutlined />,
  localization: <TranslationOutlined />,
  environment: <DesktopOutlined />,
  about: <InfoCircleOutlined />,
  workbench: null,
  providers: null,
  proxy: null,
  antigravity: null,
  usage: null,
  settings: null,
};

export default function SettingsPage() {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const language = useAppStore((s) => s.language);

  const visiblePages = SETTINGS_PAGES.filter(
    (key) => key !== "localization" || language === "zh-CN",
  );
  const [activeKey, setActiveKey] = useState<PageKey>(visiblePages[0]);
  const effectiveKey = visiblePages.includes(activeKey) ? activeKey : visiblePages[0];

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 16,
        height: "100%",
        minHeight: 0,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 12, flex: "0 0 auto" }}>
        <Typography.Title level={4} style={{ margin: 0 }}>
          {t("settings.title")}
        </Typography.Title>
      </div>
      <Layout
        style={{
          flex: 1,
          minHeight: 0,
          background: "transparent",
          overflow: "hidden",
        }}
      >
        <Sider
          width={200}
          style={{
            background: "transparent",
            overflow: "auto",
            minHeight: 0,
          }}
        >
          <Menu
            mode="inline"
            selectedKeys={[effectiveKey]}
            onClick={({ key }) => setActiveKey(key as PageKey)}
            items={visiblePages.map((key) => ({
              key,
              icon: SETTINGS_ICONS[key],
              label: t(`nav.${key}`),
            }))}
            style={{ borderInlineEnd: "none", background: "transparent" }}
          />
        </Sider>
        <Content
          style={{
            minWidth: 0,
            minHeight: 0,
            overflow: "auto",
            paddingLeft: 24,
            borderLeft: `1px solid ${token.colorBorderSecondary}`,
          }}
        >
          <EmbeddedPage pageKey={effectiveKey} />
        </Content>
      </Layout>
    </div>
  );
}

/** Lazy-loads and renders a registered page inside the settings view. */
function EmbeddedPage({ pageKey }: { pageKey: PageKey }) {
  const [Page, setPage] = useState<ComponentType | undefined>(() =>
    getLoadedPage(pageKey),
  );

  useEffect(() => {
    const loaded = getLoadedPage(pageKey);
    if (loaded) {
      setPage(() => loaded);
      return;
    }
    let cancelled = false;
    void preloadPage(pageKey).then((P) => {
      if (!cancelled) setPage(() => P);
    });
    return () => {
      cancelled = true;
    };
  }, [pageKey]);

  if (!Page) {
    return (
      <div style={{ display: "flex", justifyContent: "center", paddingTop: 48 }}>
        <Spin />
      </div>
    );
  }
  return <Page />;
}
