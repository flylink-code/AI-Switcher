import React from "react";
import AppstoreOutlined from "@ant-design/icons/es/icons/AppstoreOutlined";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import ClusterOutlined from "@ant-design/icons/es/icons/ClusterOutlined";
import FolderOutlined from "@ant-design/icons/es/icons/FolderOutlined";
import RobotOutlined from "@ant-design/icons/es/icons/RobotOutlined";
import SettingOutlined from "@ant-design/icons/es/icons/SettingOutlined";
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
  { key: "agentHub", labelKey: "navigation.agentHub", defaultLabel: "Agent", icon: <RobotOutlined /> },
  { key: "providers", labelKey: "navigation.providers", defaultLabel: "供应商", icon: <ClusterOutlined /> },
  { key: "usage", labelKey: "navigation.usage", defaultLabel: "用量统计", icon: <BarChartOutlined /> },
  { key: "workspace", labelKey: "navigation.workspace", defaultLabel: "工作区", icon: <FolderOutlined /> },
  { key: "settings", labelKey: "navigation.settings", defaultLabel: "设置", icon: <SettingOutlined /> },
];

/** Map a (possibly sub-page) activeKey to its primary navigation key. */
export function isPrimaryActive(navKey: PageKey, activeKey: PageKey): boolean {
  if (navKey === activeKey) return true;
  if (navKey === "agentHub" && (activeKey === "agentHub" || activeKey === "agentTools")) {
    return true;
  }
  if (navKey === "providers" && ["providers", "gateway", "proxy", "antigravity"].includes(activeKey)) {
    return true;
  }
  if (
    navKey === "workspace" &&
    ["workspace", "mcp", "prompts", "skills", "agents", "plugins", "profiles", "sessions"].includes(activeKey)
  ) {
    return true;
  }
  if (navKey === "settings" && ["settings", "about", "environment", "localization", "localProxy"].includes(activeKey)) {
    return true;
  }
  return false;
}
