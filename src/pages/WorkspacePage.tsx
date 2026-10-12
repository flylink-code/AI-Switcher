import { useEffect, useMemo, useState, type ComponentType } from "react";
import { Button, Dropdown, Segmented, Space, Spin } from "antd";
import DownOutlined from "@ant-design/icons/es/icons/DownOutlined";
import { useTranslation } from "react-i18next";
import {
  getLoadedPage,
  preloadPage,
  type PageKey,
} from "@/lib/pageRegistry";
import { AgentTargetSwitcher, PROVIDER_TARGET_OPTIONS } from "@/components/AgentTargetSwitcher";
import { usePagePreferencesStore, type WorkspaceResourceTab } from "@/stores/pagePreferencesStore";
import type { ProviderTarget } from "@/types/backend";

/**
 * Workspace resource pages supported per agent.
 */
const AGENT_SUPPORTED_TABS: Record<ProviderTarget, WorkspaceResourceTab[]> = {
  claude_code: ["profiles", "mcp", "prompts", "skills", "agents", "plugins", "sessions"],
  claude_desktop: ["profiles", "mcp"],
  codex: ["mcp", "prompts", "skills", "agents", "plugins", "sessions"],
  opencode: ["mcp", "prompts", "sessions"],
  pi: ["mcp", "prompts", "skills", "sessions"],
  dsh: [],
  cline: ["mcp", "prompts", "skills", "sessions"],
};

const WORKSPACE_PAGES: WorkspaceResourceTab[] = [
  "profiles",
  "mcp",
  "prompts",
  "skills",
  "agents",
  "plugins",
  "sessions",
];

function isWorkspacePage(key: string): key is WorkspaceResourceTab {
  return WORKSPACE_PAGES.some((page) => page === key);
}

/** Beyond this many tabs, the rest (plugins, sessions) fold into a "More" menu. */
const MAX_VISIBLE_TABS = 5;
/** Matches no option, so the segmented control shows no selection while a "More" tab is active. */
const NO_SEGMENT_SELECTED = "__more__" as WorkspaceResourceTab;

const NAV_FALLBACKS: Record<string, string> = {
  profiles: "项目",
  mcp: "MCP 服务",
  prompts: "提示词",
  skills: "技能",
  agents: "Agent 智能体",
  plugins: "插件",
  sessions: "会话",
};

/**
 * Workspace: Agent-driven resource management center.
 * Select an Agent on top; the secondary tabs only present features supported by that Agent.
 */
export default function WorkspacePage() {
  const { t } = useTranslation();
  const workspaceTarget = usePagePreferencesStore((s) => s.workspaceTarget);
  const setWorkspaceTarget = usePagePreferencesStore((s) => s.setWorkspaceTarget);

  const workspaceTab = usePagePreferencesStore((state) => state.workspaceTab);
  const setWorkspaceTab = usePagePreferencesStore((state) => state.setWorkspaceTab);
  const supportedTabs = useMemo(
    () => AGENT_SUPPORTED_TABS[workspaceTarget] ?? ["mcp"],
    [workspaceTarget],
  );
  const activeTab: WorkspaceResourceTab = supportedTabs.includes(workspaceTab) ? workspaceTab : (supportedTabs[0] ?? "mcp");

  const hasOverflow = supportedTabs.length > MAX_VISIBLE_TABS;
  const visibleTabs = hasOverflow ? supportedTabs.slice(0, MAX_VISIBLE_TABS) : supportedTabs;
  const overflowTabs = hasOverflow ? supportedTabs.slice(MAX_VISIBLE_TABS) : [];
  const overflowActive = overflowTabs.includes(activeTab);
  const tabOption = (key: WorkspaceResourceTab) => ({
    value: key,
    label: t(`nav.${key}`, { defaultValue: NAV_FALLBACKS[key] ?? key }),
  });

  useEffect(() => {
    if (!supportedTabs.includes(workspaceTab)) {
      const fallback = supportedTabs[0];
      if (fallback && isWorkspacePage(fallback)) setWorkspaceTab(fallback);
    }
  }, [supportedTabs, setWorkspaceTab, workspaceTab]);

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--space-4)",
        minWidth: 0,
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        <Space wrap>
          <AgentTargetSwitcher
            value={workspaceTarget}
            onChange={setWorkspaceTarget}
            targets={PROVIDER_TARGET_OPTIONS}
          />
        </Space>
        {supportedTabs.length > 0 && (
        <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
          {visibleTabs.length > 0 && (
            <Segmented<WorkspaceResourceTab>
              className="app-segmented-switcher"
              size="small"
              value={overflowActive ? NO_SEGMENT_SELECTED : activeTab}
              onChange={(key) => {
                if (isWorkspacePage(key)) setWorkspaceTab(key);
              }}
              options={visibleTabs.map((key) => tabOption(key))}
            />
          )}
          {overflowTabs.length > 0 && (
            <Dropdown
              trigger={["click"]}
              menu={{
                selectedKeys: overflowActive ? [activeTab] : [],
                items: overflowTabs.map((key) => ({
                  key,
                  label: t(`nav.${key}`, { defaultValue: NAV_FALLBACKS[key] ?? key }),
                })),
                onClick: ({ key }) => {
                  if (isWorkspacePage(key)) setWorkspaceTab(key);
                },
              }}
            >
              <Button size="small" type={overflowActive ? "primary" : "default"} aria-label="workspace-more-tabs">
                {overflowActive
                  ? t(`nav.${activeTab}`, { defaultValue: NAV_FALLBACKS[activeTab] ?? activeTab })
                  : t("nav.more", { defaultValue: "更多" })}
                <DownOutlined />
              </Button>
            </Dropdown>
          )}
        </div>
        )}
      </div>
      {supportedTabs.length > 0 && <EmbeddedPage pageKey={activeTab} target={workspaceTarget} />}
    </div>
  );
}

/** Lazy-loads and renders a registered page inside the workspace view. */
function EmbeddedPage({ pageKey, target }: { pageKey: PageKey; target: ProviderTarget }) {
  const [Page, setPage] = useState<ComponentType<{ target?: unknown }> | undefined>(() =>
    getLoadedPage(pageKey),
  );

  useEffect(() => {
    const loaded = getLoadedPage(pageKey);
    if (loaded) {
      setPage(() => loaded as ComponentType<{ target?: unknown }>);
      return;
    }
    let cancelled = false;
    void preloadPage(pageKey).then((P) => {
      if (!cancelled) setPage(() => P as ComponentType<{ target?: unknown }>);
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
  return <Page target={target} />;
}
