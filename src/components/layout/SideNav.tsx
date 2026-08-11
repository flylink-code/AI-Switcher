import React from "react";
import { useTranslation } from "react-i18next";
import type { PageKey } from "@/lib/pageRegistry";
import { NAV_ITEMS, isPrimaryActive } from "./navItems";

export interface SideNavProps {
  activeKey: PageKey;
  onNavigate: (key: PageKey) => void;
}

/**
 * Left primary sidebar (replaces the top NavigationDock).
 * Pure navigation — Agent switching lives per page, not in the global chrome.
 * All items always visible; no collapse/"More" behavior.
 */
export const SideNav: React.FC<SideNavProps> = ({ activeKey, onNavigate }) => {
  const { t } = useTranslation();

  return (
    <nav
      aria-label={t("navigation.primary", { defaultValue: "主导航" })}
      className="side-nav"
      style={{
        width: 208,
        flexShrink: 0,
        display: "flex",
        flexDirection: "column",
        gap: 2,
        padding: "var(--space-3) var(--space-2)",
        backgroundColor: "var(--color-bg-surface)",
        borderRight: "1px solid var(--color-border)",
        boxSizing: "border-box",
        overflowY: "auto",
      }}
    >
      {NAV_ITEMS.map((item) => {
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
              gap: "var(--space-2)",
              width: "100%",
              padding: "8px 12px",
              border: "none",
              borderRadius: "var(--radius-md)",
              backgroundColor: isActive ? "var(--color-brand-subtle)" : "transparent",
              color: isActive ? "var(--color-brand)" : "var(--color-text-primary)",
              fontWeight: isActive ? "var(--font-weight-semibold)" : "var(--font-weight-regular)",
              fontSize: "var(--font-size-md)",
              textAlign: "left",
              cursor: "pointer",
              whiteSpace: "nowrap",
              transition: "background-color 0.15s ease, color 0.15s ease",
            }}
            onMouseEnter={(e) => {
              if (!isActive) e.currentTarget.style.backgroundColor = "var(--color-bg-subtle)";
            }}
            onMouseLeave={(e) => {
              if (!isActive) e.currentTarget.style.backgroundColor = "transparent";
            }}
          >
            <span style={{ fontSize: "15px", display: "inline-flex", alignItems: "center" }}>
              {item.icon}
            </span>
            <span>{label}</span>
          </button>
        );
      })}
    </nav>
  );
};
