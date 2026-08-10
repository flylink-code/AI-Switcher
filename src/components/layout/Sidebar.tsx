import React from "react";
import { Tooltip } from "antd";
import { useTranslation } from "react-i18next";
import {
  AppstoreOutlined,
  ClusterOutlined,
  ApiOutlined,
  BarChartOutlined,
  UserOutlined,
  FolderOutlined,
  SettingOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
} from "@ant-design/icons";
import type { PageKey } from "@/lib/pageRegistry";
import appLogo from "@/assets/app-logo.png";

export interface NavItemDef {
  key: PageKey;
  labelKey: string;
  defaultLabel: string;
  icon: React.ReactNode;
  group?: "overview" | "runtime" | "resources" | "system";
}

export const NAV_ITEMS: NavItemDef[] = [
  { key: "workbench", labelKey: "navigation.dashboard", defaultLabel: "概览", icon: <AppstoreOutlined />, group: "overview" },
  { key: "providers", labelKey: "navigation.providers", defaultLabel: "供应商", icon: <ClusterOutlined />, group: "runtime" },
  { key: "proxy", labelKey: "navigation.proxy", defaultLabel: "代理控制", icon: <ApiOutlined />, group: "runtime" },
  { key: "usage", labelKey: "navigation.usage", defaultLabel: "用量统计", icon: <BarChartOutlined />, group: "runtime" },
  { key: "antigravity", labelKey: "navigation.accounts", defaultLabel: "账号与额度", icon: <UserOutlined />, group: "resources" },
  { key: "mcp", labelKey: "navigation.workspace", defaultLabel: "工作区资源", icon: <FolderOutlined />, group: "resources" },
  { key: "settings", labelKey: "navigation.settings", defaultLabel: "设置", icon: <SettingOutlined />, group: "system" },
];

export interface SidebarProps {
  activeKey: PageKey;
  onNavigate: (key: PageKey) => void;
  collapsed: boolean;
  onToggleCollapse: () => void;
  className?: string;
  style?: React.CSSProperties;
}

export const Sidebar: React.FC<SidebarProps> = ({
  activeKey,
  onNavigate,
  collapsed,
  onToggleCollapse,
  className = "",
  style,
}) => {
  const { t } = useTranslation();

  // Helper to map legacy activeKey to primary navigation key
  const isPrimaryActive = (navKey: PageKey) => {
    if (navKey === activeKey) return true;
    // Map workspace sub-pages
    if (navKey === "mcp" && ["mcp", "prompts", "skills", "agents", "codexPlugins", "sessions", "profiles"].includes(activeKey)) {
      return true;
    }
    // Map settings sub-pages
    if (navKey === "settings" && ["settings", "about", "environment", "localization"].includes(activeKey)) {
      return true;
    }
    return false;
  };

  return (
    <aside
      className={`app-sidebar ${className}`.trim()}
      style={{
        width: collapsed ? "var(--app-sidebar-collapsed-width)" : "var(--app-sidebar-width)",
        minWidth: collapsed ? "var(--app-sidebar-collapsed-width)" : "var(--app-sidebar-width)",
        backgroundColor: "var(--color-bg-subtle)",
        borderRight: "1px solid var(--color-border)",
        display: "flex",
        flexDirection: "column",
        height: "100%",
        transition: "width 0.2s ease, min-width 0.2s ease",
        userSelect: "none",
        flexShrink: 0,
        boxSizing: "border-box",
        ...style,
      }}
    >
      {/* Brand Header */}
      <div
        style={{
          height: "var(--app-header-height)",
          display: "flex",
          alignItems: "center",
          padding: collapsed ? "0 16px" : "0 20px",
          gap: "var(--space-3)",
          borderBottom: "1px solid var(--color-border-subtle)",
          overflow: "hidden",
        }}
      >
        <img src={appLogo} alt="AI-Switcher" style={{ width: 24, height: 24, flexShrink: 0 }} />
        {!collapsed && (
          <span
            style={{
              fontWeight: "var(--font-weight-bold)",
              fontSize: "var(--font-size-lg)",
              color: "var(--color-text-primary)",
              whiteSpace: "nowrap",
            }}
          >
            AI-Switcher
          </span>
        )}
      </div>

      {/* Nav List */}
      <div style={{ flex: 1, padding: "var(--space-3) var(--space-2)", overflowY: "auto", display: "flex", flexDirection: "column", gap: "var(--space-1)" }}>
        {NAV_ITEMS.map((item) => {
          const isActive = isPrimaryActive(item.key);
          const label = t(item.labelKey, { defaultValue: item.defaultLabel });

          const navButton = (
            <button
              key={item.key}
              type="button"
              onClick={() => onNavigate(item.key)}
              style={{
                display: "flex",
                alignItems: "center",
                gap: "var(--space-3)",
                width: "100%",
                padding: collapsed ? "10px 0" : "8px 12px",
                justifyContent: collapsed ? "center" : "flex-start",
                border: "none",
                borderRadius: "var(--radius-md)",
                backgroundColor: isActive ? "var(--color-brand-subtle)" : "transparent",
                color: isActive ? "var(--color-brand)" : "var(--color-text-primary)",
                fontWeight: isActive ? "var(--font-weight-semibold)" : "var(--font-weight-regular)",
                fontSize: "var(--font-size-md)",
                cursor: "pointer",
                transition: "background-color 0.15s ease, color 0.15s ease",
              }}
              onMouseEnter={(e) => {
                if (!isActive) e.currentTarget.style.backgroundColor = "var(--color-bg-surface)";
              }}
              onMouseLeave={(e) => {
                if (!isActive) e.currentTarget.style.backgroundColor = "transparent";
              }}
            >
              <span style={{ fontSize: "16px", display: "flex", alignItems: "center", justifyContent: "center" }}>
                {item.icon}
              </span>
              {!collapsed && (
                <span style={{ whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                  {label}
                </span>
              )}
            </button>
          );

          if (collapsed) {
            return (
              <Tooltip key={item.key} title={label} placement="right">
                {navButton}
              </Tooltip>
            );
          }

          return navButton;
        })}
      </div>

      {/* Footer Collapse Button */}
      <div
        style={{
          padding: "var(--space-2)",
          borderTop: "1px solid var(--color-border-subtle)",
          display: "flex",
          justifyContent: collapsed ? "center" : "flex-end",
        }}
      >
        <button
          type="button"
          onClick={onToggleCollapse}
          title={collapsed ? t("navigation.expand", { defaultValue: "展开边栏" }) : t("navigation.collapse", { defaultValue: "收起边栏" })}
          style={{
            background: "none",
            border: "none",
            color: "var(--color-text-secondary)",
            padding: "8px",
            borderRadius: "var(--radius-sm)",
            cursor: "pointer",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
          onMouseEnter={(e) => { e.currentTarget.style.backgroundColor = "var(--color-bg-surface)"; }}
          onMouseLeave={(e) => { e.currentTarget.style.backgroundColor = "transparent"; }}
        >
          {collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
        </button>
      </div>
    </aside>
  );
};
