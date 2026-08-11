import React from "react";
import { Dropdown, type MenuProps } from "antd";
import { DownOutlined } from "@ant-design/icons";
import { useTranslation } from "react-i18next";
import type { PageKey } from "@/lib/pageRegistry";
import { NAV_ITEMS, isPrimaryActive, type NavItemDef } from "./navItems";

export interface NavigationDockProps {
  activeKey: PageKey;
  onNavigate: (key: PageKey) => void;
}

/**
 * Top content-aligned navigation dock (replaces the left icon rail).
 * Pure navigation — Agent switching now lives per page (ContextHeader extra /
 * dashboard cards), not in the global chrome.
 *
 * Narrow windows (<1280px, pure CSS media query): low-frequency items
 * (Accounts / Workspace / Settings) collapse into a "More" dropdown — the
 * dock never wraps.
 */
export const NavigationDock: React.FC<NavigationDockProps> = ({
  activeKey,
  onNavigate,
}) => {
  const { t } = useTranslation();

  const renderNavButton = (item: NavItemDef, extraClass = "") => {
    const isActive = isPrimaryActive(item.key, activeKey);
    const label = t(item.labelKey, { defaultValue: item.defaultLabel });
    return (
      <button
        key={item.key}
        type="button"
        aria-current={isActive ? "page" : undefined}
        onClick={() => onNavigate(item.key)}
        className={`navigation-dock-item ${extraClass}`.trim()}
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: "var(--space-2)",
          padding: "6px 12px",
          border: "none",
          borderRadius: "var(--radius-md)",
          backgroundColor: isActive ? "var(--color-brand-subtle)" : "transparent",
          color: isActive ? "var(--color-brand)" : "var(--color-text-primary)",
          fontWeight: isActive ? "var(--font-weight-semibold)" : "var(--font-weight-regular)",
          fontSize: "var(--font-size-md)",
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
  };

  const lowFrequencyItems = NAV_ITEMS.filter((item) => item.lowFrequency);
  const activeLowFrequency = lowFrequencyItems.find((item) => isPrimaryActive(item.key, activeKey));

  const moreMenu: MenuProps["items"] = lowFrequencyItems.map((item) => ({
    key: item.key,
    icon: item.icon,
    label: t(item.labelKey, { defaultValue: item.defaultLabel }),
  }));

  return (
    <div
      className="navigation-dock"
      style={{
        padding: "var(--space-3) var(--page-padding-x) 0",
        flexShrink: 0,
        boxSizing: "border-box",
      }}
    >
      <div
        className="navigation-dock-surface"
        style={{
          display: "flex",
          alignItems: "center",
          gap: "var(--space-3)",
          minHeight: 44,
          padding: "4px 8px",
          backgroundColor: "var(--color-bg-surface)",
          border: "1px solid var(--color-border)",
          borderRadius: "var(--radius-lg)",
          boxSizing: "border-box",
        }}
      >
        <nav
          aria-label={t("navigation.primary", { defaultValue: "主导航" })}
          style={{ display: "flex", alignItems: "center", gap: "var(--space-1)", flex: 1, minWidth: 0 }}
        >
          {NAV_ITEMS.map((item) =>
            renderNavButton(item, item.lowFrequency ? "nav-item-low-frequency" : ""),
          )}

          <Dropdown
            menu={{ items: moreMenu, onClick: ({ key }) => onNavigate(key as PageKey) }}
            trigger={["click"]}
          >
            <button
              type="button"
              className="navigation-dock-item navigation-dock-more"
              style={{
                display: "none",
                alignItems: "center",
                gap: "var(--space-2)",
                padding: "6px 12px",
                border: "none",
                borderRadius: "var(--radius-md)",
                backgroundColor: activeLowFrequency ? "var(--color-brand-subtle)" : "transparent",
                color: activeLowFrequency ? "var(--color-brand)" : "var(--color-text-primary)",
                fontWeight: activeLowFrequency ? "var(--font-weight-semibold)" : "var(--font-weight-regular)",
                fontSize: "var(--font-size-md)",
                cursor: "pointer",
                whiteSpace: "nowrap",
              }}
            >
              <span>
                {activeLowFrequency
                  ? t(activeLowFrequency.labelKey, { defaultValue: activeLowFrequency.defaultLabel })
                  : t("navigation.more", { defaultValue: "更多" })}
              </span>
              <DownOutlined style={{ fontSize: 10 }} />
            </button>
          </Dropdown>
        </nav>
      </div>
    </div>
  );
};
