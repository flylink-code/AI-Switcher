import { useEffect, useState, type ComponentType } from "react";
import { Segmented, Spin } from "antd";
import { useTranslation } from "react-i18next";
import {
  getLoadedPage,
  preloadPage,
  type PageKey,
} from "@/lib/pageRegistry";

/**
 * Workspace resource pages. Order = display order in the secondary navigation.
 */
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

/**
 * Workspace: the resource management center (Projects / MCP / Prompts /
 * Skills / Agents / Plugins). Resources are embedded under a compact
 * segmented navigation instead of living inside Settings.
 */
export default function WorkspacePage() {
  const { t } = useTranslation();

  const [activeTab, setActiveTab] = useState<PageKey>(() => {
    if (typeof localStorage !== "undefined") {
      const stored = migrateWorkspaceTab(localStorage.getItem(WORKSPACE_TAB_KEY) ?? "");
      if (isWorkspacePage(stored)) return stored;
    }
    return "profiles";
  });

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
      <Segmented<PageKey>
        value={activeTab}
        onChange={(key) => setActiveTab(key)}
        options={WORKSPACE_PAGES.map((key) => ({
          value: key,
          label: t(`nav.${key}`),
        }))}
      />
      <EmbeddedPage pageKey={activeTab} />
    </div>
  );
}

/** Lazy-loads and renders a registered page inside the workspace view. */
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
