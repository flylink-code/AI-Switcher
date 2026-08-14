import { useEffect, useState, type ComponentType } from "react";
import { Segmented, Spin } from "antd";
import { useTranslation } from "react-i18next";
import {
  getLoadedPage,
  preloadPage,
  type PageKey,
} from "@/lib/pageRegistry";
import { AgentTargetSwitcher } from "@/components/AgentTargetSwitcher";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { ProviderTarget } from "@/types/backend";

/**
 * Workspace resource pages supported per agent.
 */
const AGENT_SUPPORTED_TABS: Record<ProviderTarget, PageKey[]> = {
  claude_code: ["profiles", "mcp", "prompts", "skills", "agents", "plugins"],
  claude_desktop: ["profiles", "mcp"],
  codex: ["mcp", "prompts", "skills", "agents", "plugins"],
  opencode: ["mcp", "prompts"],
  pi: ["mcp", "prompts", "skills"],
  dsh: ["mcp", "prompts"],
};

const WORKSPACE_PAGES: PageKey[] = [
  "profiles",
  "mcp",
  "prompts",
  "skills",
  "agents",
  "plugins",
];

const WORKSPACE_TAB_KEY = "cs.workspaceTab";

/** Old Claude/Codex plugin tabs → unified plugins hub. */
function migrateWorkspaceTab(key: string): string {
  if (key === "claudePlugins" || key === "codexPlugins") {
    if (typeof localStorage !== "undefined" && !localStorage.getItem("cs.pluginsTarget")) {
      localStorage.setItem("cs.pluginsTarget", key === "codexPlugins" ? "codex" : "claude_code");
    }
    return "plugins";
  }
  return key;
}

function isWorkspacePage(key: string): key is PageKey {
  return WORKSPACE_PAGES.some((page) => page === key);
}

const NAV_FALLBACKS: Record<string, string> = {
  profiles: "项目",
  mcp: "MCP 服务",
  prompts: "提示词",
  skills: "技能",
  agents: "Agent 智能体",
  plugins: "插件",
};

/**
 * Workspace: Agent-driven resource management center.
 * Select an Agent on top; the secondary tabs only present features supported by that Agent.
 */
export default function WorkspacePage() {
  const { t } = useTranslation();
  const workspaceTarget = usePagePreferencesStore((s) => s.workspaceTarget);
  const setWorkspaceTarget = usePagePreferencesStore((s) => s.setWorkspaceTarget);

  const supportedTabs = AGENT_SUPPORTED_TABS[workspaceTarget] ?? ["mcp"];

  const [activeTab, setActiveTab] = useState<PageKey>(() => {
    if (typeof localStorage !== "undefined") {
      const stored = migrateWorkspaceTab(localStorage.getItem(WORKSPACE_TAB_KEY) ?? "");
      if (isWorkspacePage(stored) && supportedTabs.includes(stored)) return stored;
    }
    return supportedTabs[0] ?? "mcp";
  });

  useEffect(() => {
    if (!supportedTabs.includes(activeTab)) {
      setActiveTab(supportedTabs[0] ?? "mcp");
    }
  }, [workspaceTarget, supportedTabs, activeTab]);

  useEffect(() => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(WORKSPACE_TAB_KEY, activeTab);
    }
  }, [activeTab]);

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
        <div>
          <AgentTargetSwitcher value={workspaceTarget} onChange={setWorkspaceTarget} />
        </div>
        <div>
          <Segmented<PageKey>
            className="app-segmented-switcher"
            size="small"
            value={activeTab}
            onChange={(key) => setActiveTab(key)}
            options={supportedTabs.map((key) => ({
              value: key,
              label: t(`nav.${key}`, { defaultValue: NAV_FALLBACKS[key] ?? key }),
            }))}
          />
        </div>
      </div>
      <EmbeddedPage pageKey={activeTab} target={workspaceTarget} />
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
