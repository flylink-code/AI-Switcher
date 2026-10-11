import { useEffect, useMemo, useState, type ComponentType } from "react";
import { Segmented, Space, Spin } from "antd";
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
        <div>
          <Segmented<WorkspaceResourceTab>
            className="app-segmented-switcher"
            size="small"
            value={activeTab}
            onChange={(key) => {
              if (isWorkspacePage(key)) setWorkspaceTab(key);
            }}
            options={supportedTabs.map((key) => ({
              value: key,
              label: t(`nav.${key}`, { defaultValue: NAV_FALLBACKS[key] ?? key }),
            }))}
          />
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
