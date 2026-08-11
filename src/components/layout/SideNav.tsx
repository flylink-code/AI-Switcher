import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Tooltip } from "antd";
import MenuFoldOutlined from "@ant-design/icons/es/icons/MenuFoldOutlined";
import MenuUnfoldOutlined from "@ant-design/icons/es/icons/MenuUnfoldOutlined";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { PageKey } from "@/lib/pageRegistry";
import { NAV_ITEMS, isPrimaryActive } from "./navItems";
import appLogo from "@/assets/app-logo.png";

const appWindow = getCurrentWindow();

export interface SideNavProps {
  activeKey: PageKey;
  onNavigate: (key: PageKey) => void;
}

/**
 * Left primary sidebar (184px expanded / 56px icon-only collapsed).
 * Pure navigation — Agent switching lives per page.
 * Business items at top, Settings pinned at bottom.
 */
export const SideNav: React.FC<SideNavProps> = ({ activeKey, onNavigate }) => {
  const { t } = useTranslation();

  const [collapsed, setCollapsed] = useState<boolean>(() => {
    if (typeof localStorage !== "undefined") {
      return localStorage.getItem("cs.sideNavCollapsed") === "true";
    }
    return false;
  });

  useEffect(() => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem("cs.sideNavCollapsed", String(collapsed));
    }
  }, [collapsed]);

  const mainItems = NAV_ITEMS.filter((item) => item.key !== "settings");
  const settingsItem = NAV_ITEMS.find((item) => item.key === "settings");

  const renderNavItem = (item: (typeof NAV_ITEMS)[number]) => {
    const isActive = isPrimaryActive(item.key, activeKey);
    const label = t(item.labelKey, { defaultValue: item.defaultLabel });

    const navButton = (
      <button
        key={item.key}
        type="button"
        aria-current={isActive ? "page" : undefined}
        onClick={() => onNavigate(item.key)}
        className="side-nav-item"
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: collapsed ? "center" : "flex-start",
          gap: collapsed ? 0 : "8px",
          width: collapsed ? "38px" : "100%",
          height: "38px",
          margin: collapsed ? "0 auto" : 0,
          padding: collapsed ? 0 : "0 10px",
          border: "none",
          borderLeft: isActive
            ? collapsed
              ? "none"
              : "2px solid var(--color-brand)"
            : "2px solid transparent",
          borderRadius: "6px",
          backgroundColor: isActive
            ? "var(--color-bg-selected-subtle, rgba(22, 119, 255, 0.08))"
            : "transparent",
          color: isActive ? "var(--color-brand)" : "var(--color-text-primary)",
          fontWeight: isActive ? "var(--font-weight-semibold)" : "var(--font-weight-regular)",
          fontSize: "var(--font-size-md)",
          textAlign: "left",
          cursor: "pointer",
          whiteSpace: "nowrap",
          transition: "all 0.15s ease",
          boxSizing: "border-box",
        }}
        onMouseEnter={(e) => {
          if (!isActive) e.currentTarget.style.backgroundColor = "var(--color-bg-subtle)";
        }}
        onMouseLeave={(e) => {
          if (!isActive) e.currentTarget.style.backgroundColor = "transparent";
        }}
      >
        <span style={{ fontSize: "16px", display: "inline-flex", alignItems: "center" }}>
          {item.icon}
        </span>
        {!collapsed && (
          <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}>{label}</span>
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
  };

  return (
    <nav
      aria-label={t("navigation.primary", { defaultValue: "主导航" })}
      className="side-nav"
      style={{
        width: collapsed ? 56 : 184,
        minWidth: collapsed ? 56 : 176,
        maxWidth: collapsed ? 56 : 188,
        flexShrink: 0,
        display: "flex",
        flexDirection: "column",
        gap: "4px",
        padding: collapsed ? "10px 4px" : "10px 8px",
        backgroundColor: "var(--color-bg-surface)",
        borderRight: "1px solid var(--color-border-subtle, var(--color-border))",
        boxSizing: "border-box",
        height: "100%",
        transition: "width 0.2s cubic-bezier(0.2, 0, 0, 1), min-width 0.2s cubic-bezier(0.2, 0, 0, 1), max-width 0.2s cubic-bezier(0.2, 0, 0, 1)",
      }}
    >
      {/* Sidebar Brand Header — brand only; collapse toggle lives at the bottom */}
      <div
        data-tauri-drag-region
        onDoubleClick={() => {
          void appWindow.toggleMaximize();
        }}
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: collapsed ? "center" : "flex-start",
          padding: collapsed ? "6px 0 10px 0" : "6px 4px 10px 6px",
          borderBottom: "1px solid var(--color-border-subtle, rgba(0,0,0,0.06))",
          marginBottom: "4px",
          userSelect: "none",
          cursor: "default",
          minHeight: "36px",
          boxSizing: "border-box",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: "8px", overflow: "hidden" }}>
          <img
            src={appLogo}
            alt="Logo"
            style={{ width: 18, height: 18, objectFit: "contain", pointerEvents: "none", flexShrink: 0 }}
          />
          {!collapsed && (
            <span
              style={{
                fontWeight: 600,
                fontSize: "14px",
                color: "var(--color-text-primary)",
                letterSpacing: "-0.2px",
                pointerEvents: "none",
                whiteSpace: "nowrap",
              }}
            >
              AI-Switcher
            </span>
          )}
        </div>
      </div>

      {/* Main Navigation Items */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: "2px", overflowY: "auto" }}>
        {mainItems.map(renderNavItem)}
      </div>

      {/* Bottom Divider, Settings Item & Collapse Toggle (low visual weight, same axis as nav items) */}
      <div style={{ borderTop: "1px solid var(--color-border-subtle, rgba(0,0,0,0.06))", paddingTop: "6px", marginTop: "4px", display: "flex", flexDirection: "column", gap: "2px" }}>
        {settingsItem && renderNavItem(settingsItem)}
        <Tooltip
          title={collapsed ? t("common.expand", { defaultValue: "展开侧边栏" }) : t("common.collapse", { defaultValue: "收起侧边栏" })}
          placement="right"
        >
          <button
            type="button"
            onClick={() => setCollapsed(!collapsed)}
            className="side-nav-item side-nav-collapse-toggle"
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: collapsed ? "center" : "flex-start",
              gap: collapsed ? 0 : "8px",
              width: collapsed ? "38px" : "100%",
              height: "38px",
              margin: collapsed ? "0 auto" : 0,
              padding: collapsed ? 0 : "0 10px",
              border: "none",
              borderLeft: "2px solid transparent",
              borderRadius: "6px",
              backgroundColor: "transparent",
              color: "var(--color-text-secondary)",
              fontSize: "var(--font-size-md)",
              cursor: "pointer",
              whiteSpace: "nowrap",
              transition: "all 0.15s ease",
              boxSizing: "border-box",
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.backgroundColor = "var(--color-bg-subtle)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.backgroundColor = "transparent";
            }}
          >
            <span style={{ fontSize: "16px", display: "inline-flex", alignItems: "center" }}>
              {collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
            </span>
            {!collapsed && (
              <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", textAlign: "left" }}>
                {t("common.collapse", { defaultValue: "收起侧边栏" })}
              </span>
            )}
          </button>
        </Tooltip>
      </div>
    </nav>
  );
};
