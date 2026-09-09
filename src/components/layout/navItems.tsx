import React from "react";
import {
  ApiOutlined,
  AppstoreOutlined,
  ClusterOutlined,
  BarChartOutlined,
  FolderOutlined,
  MessageOutlined,
  SettingOutlined,
} from "@ant-design/icons";
import type { PageKey } from "@/lib/pageRegistry";

export interface NavItemDef {
  key: PageKey;
  labelKey: string;
  defaultLabel: string;
  icon: React.ReactNode;
}

/** Primary navigation items shared by the left SideNav. */
export const NAV_ITEMS: NavItemDef[] = [
  { key: "workbench", labelKey: "navigation.dashboard", defaultLabel: "概览", icon: <AppstoreOutlined /> },
  { key: "providers", labelKey: "navigation.providers", defaultLabel: "供应商", icon: <ClusterOutlined /> },
  { key: "gateway", labelKey: "navigation.gateway", defaultLabel: "网关", icon: <ApiOutlined /> },
  { key: "usage", labelKey: "navigation.usage", defaultLabel: "用量统计", icon: <BarChartOutlined /> },
  { key: "workspace", labelKey: "navigation.workspace", defaultLabel: "工作区", icon: <FolderOutlined /> },
  { key: "sessions", labelKey: "navigation.sessions", defaultLabel: "会话", icon: <MessageOutlined /> },
  { key: "settings", labelKey: "navigation.settings", defaultLabel: "设置", icon: <SettingOutlined /> },
];

/** Map a (possibly sub-page) activeKey to its primary navigation key. */
export function isPrimaryActive(navKey: PageKey, activeKey: PageKey): boolean {
  if (navKey === activeKey) return true;
  if (navKey === "workspace" && ["workspace", "mcp", "prompts", "skills", "agents", "plugins", "profiles"].includes(activeKey)) {
    return true;
  }
  if (navKey === "settings" && ["settings", "about", "environment", "localization", "agentTools", "localProxy"].includes(activeKey)) {
    return true;
  }
  if (navKey === "gateway" && ["gateway", "proxy", "antigravity"].includes(activeKey)) {
    return true;
  }
  return false;
}
