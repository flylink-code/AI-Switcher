import type { ComponentType } from "react";

export const PAGE_KEYS = [
  "workbench",
  "settings",
  "providers",
  "profiles",
  "proxy",
  "antigravity",
  "mcp",
  "prompts",
  "skills",
  "agents",
  "codexPlugins",
  "sessions",
  "usage",
  "localization",
  "environment",
  "about",
] as const;

export type PageKey = (typeof PAGE_KEYS)[number];
type PageModule = { default: ComponentType };
type PageLoader = () => Promise<PageModule>;

const pageLoaders: Record<PageKey, PageLoader> = {
  workbench: () => import("@/pages/WorkbenchPage"),
  settings: () => import("@/pages/SettingsPage"),
  providers: () => import("@/pages/ProvidersPage"),
  profiles: () => import("@/pages/ProfilesPage"),
  proxy: () => import("@/pages/ProxyPage"),
  antigravity: () => import("@/pages/AntigravityPage"),
  mcp: () => import("@/pages/McpPage"),
  prompts: () => import("@/pages/PromptsPage"),
  skills: () => import("@/pages/SkillsPage"),
  agents: () => import("@/pages/AgentsPage"),
  codexPlugins: () => import("@/pages/CodexPluginsPage"),
  sessions: () => import("@/pages/SessionsPage"),
  usage: () => import("@/pages/UsagePage"),
  localization: () => import("@/pages/DesktopLocalizationPage"),
  environment: () => import("@/pages/EnvironmentPage"),
  about: () => import("@/pages/AboutPage"),
};

const modulePromises = new Map<PageKey, Promise<PageModule>>();
const loadedPages = new Map<PageKey, ComponentType>();

export function preloadPage(key: PageKey): Promise<ComponentType> {
  const loaded = loadedPages.get(key);
  if (loaded) return Promise.resolve(loaded);

  let promise = modulePromises.get(key);
  if (!promise) {
    promise = pageLoaders[key]().catch((error) => {
      modulePromises.delete(key);
      throw error;
    });
    modulePromises.set(key, promise);
  }

  return promise.then((module) => {
    loadedPages.set(key, module.default);
    return module.default;
  });
}

export function getLoadedPage(key: PageKey): ComponentType | undefined {
  return loadedPages.get(key);
}

export function isPageKey(value: string): value is PageKey {
  return PAGE_KEYS.some((key) => key === value);
}
