import React from "react";
import appLogo from "@/assets/app-logo.png";

export interface StatusBarProps {
  appVersion?: string | null;
  className?: string;
  style?: React.CSSProperties;
}

/**
 * Bottom status bar. Pure branding + version now — Proxy / current-provider
 * status moved to the per-page views (workbench cards, proxy page).
 */
export const StatusBar: React.FC<StatusBarProps> = ({
  appVersion,
  className = "",
  style,
}) => {
  return (
    <footer
      className={`app-status-bar ${className}`.trim()}
      style={{
        height: "var(--app-statusbar-height)",
        padding: "0 var(--space-4)",
        backgroundColor: "var(--color-bg-surface)",
        borderTop: "1px solid var(--color-border-subtle)",
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        fontSize: "var(--font-size-xs)",
        color: "var(--color-text-secondary)",
        userSelect: "none",
        flexShrink: 0,
        boxSizing: "border-box",
        ...style,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: "var(--space-2)" }}>
        <img src={appLogo} alt="AI-Switcher" style={{ width: 14, height: 14 }} />
        <span style={{ fontWeight: "var(--font-weight-semibold)", color: "var(--color-text-primary)" }}>
          AI-Switcher
        </span>
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: "var(--space-3)" }}>
        {appVersion && <span>v{appVersion}</span>}
      </div>
    </footer>
  );
};
