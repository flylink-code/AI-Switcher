import React from "react";
import { useTranslation } from "react-i18next";
import type { PageKey } from "@/lib/pageRegistry";
import { NAV_ITEMS, isPrimaryActive } from "./navItems";
import appLogo from "@/assets/app-logo.png";

export interface SideNavProps {
  activeKey: PageKey;
  onNavigate: (key: PageKey) => void;
}

/**
 * Left primary sidebar (184px fixed width).
 * Pure navigation — Agent switching lives per page.
 * Business items at top, Settings pinned at bottom.
 */
export const SideNav: React.FC<SideNavProps> = ({ activeKey, onNavigate }) => {
  const { t } = useTranslation();

  const mainItems = NAV_ITEMS.filter((item) => item.key !== "settings");
  const settingsItem = NAV_ITEMS.find((item) => item.key === "settings");

  const renderNavItem = (item: (typeof NAV_ITEMS)[number]) => {
    const isActive = isPrimaryActive(item.key, activeKey);
    const label = t(item.labelKey, { defaultValue: item.defaultLabel });

    return (
      <button
        key={item.key}
        type="button"
        aria-current={isActive ? "page" : undefined}
        onClick={() => onNavigate(item.key)}
        className="side-nav-item"
        style={{
          display: "flex",
          alignItems: "center",
          gap: "8px",
          width: "100%",
          height: "38px",
          padding: "0 10px",
          border: "none",
          borderLeft: isActive ? "2px solid var(--color-brand)" : "2px solid transparent",
          borderRadius: "6px",
          backgroundColor: isActive ? "var(--color-bg-selected-subtle, rgba(22, 119, 255, 0.08))" : "transparent",
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
        <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}>{label}</span>
      </button>
    );
  };

  return (
    <nav
      aria-label={t("navigation.primary", { defaultValue: "主导航" })}
      className="side-nav"
      style={{
        width: 184,
        minWidth: 176,
        maxWidth: 188,
        flexShrink: 0,
        display: "flex",
        flexDirection: "column",
        gap: "4px",
        padding: "10px 8px",
        backgroundColor: "var(--color-bg-surface)",
        borderRight: "1px solid var(--color-border-subtle, var(--color-border))",
        boxSizing: "border-box",
        height: "100%",
      }}
    >
      {/* Sidebar Brand Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "8px",
          padding: "4px 8px 10px 8px",
          borderBottom: "1px solid var(--color-border-subtle, rgba(0,0,0,0.06))",
          marginBottom: "4px",
        }}
      >
        <img src={appLogo} alt="Logo" style={{ width: 18, height: 18, objectFit: "contain" }} />
        <span
          style={{
            fontWeight: 600,
            fontSize: "14px",
            color: "var(--color-text-primary)",
            letterSpacing: "-0.2px",
          }}
        >
          AI-Switcher
        </span>
      </div>

      {/* Main Navigation Items */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: "2px", overflowY: "auto" }}>
        {mainItems.map(renderNavItem)}
      </div>

      {/* Bottom Divider & Settings Item */}
      <div style={{ borderTop: "1px solid var(--color-border-subtle, rgba(0,0,0,0.06))", paddingTop: "6px", marginTop: "4px" }}>
        {settingsItem && renderNavItem(settingsItem)}
      </div>
    </nav>
  );
};
