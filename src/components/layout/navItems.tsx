import React from "react";
import {
  AppstoreOutlined,
  ClusterOutlined,
  BarChartOutlined,
  UserOutlined,
  FolderOutlined,
  SettingOutlined,
} from "@ant-design/icons";
import type { PageKey } from "@/lib/pageRegistry";

export interface NavItemDef {
  key: PageKey;
  labelKey: string;
  defaultLabel: string;
  icon: React.ReactNode;
}

/** Primary navigation items shared by the left SideNav. Proxy lives under Settings. */
export const NAV_ITEMS: NavItemDef[] = [
  { key: "workbench", labelKey: "navigation.dashboard", defaultLabel: "概览", icon: <AppstoreOutlined /> },
  { key: "providers", labelKey: "navigation.providers", defaultLabel: "供应商", icon: <ClusterOutlined /> },
  { key: "usage", labelKey: "navigation.usage", defaultLabel: "用量统计", icon: <BarChartOutlined /> },
  { key: "antigravity", labelKey: "navigation.accounts", defaultLabel: "账号与额度", icon: <UserOutlined /> },
  { key: "workspace", labelKey: "navigation.workspace", defaultLabel: "工作区", icon: <FolderOutlined /> },
  { key: "settings", labelKey: "navigation.settings", defaultLabel: "设置", icon: <SettingOutlined /> },
];

/** Map a (possibly sub-page) activeKey to its primary navigation key. */
export function isPrimaryActive(navKey: PageKey, activeKey: PageKey): boolean {
  if (navKey === activeKey) return true;
  if (navKey === "workspace" && ["workspace", "mcp", "prompts", "skills", "agents", "codexPlugins", "profiles"].includes(activeKey)) {
    return true;
  }
  if (navKey === "settings" && ["settings", "sessions", "about", "environment", "localization", "proxy"].includes(activeKey)) {
    return true;
  }
  return false;
}
