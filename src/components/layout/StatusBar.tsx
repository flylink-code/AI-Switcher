import React from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { proxyStatusOptions, providerListOptions } from "@/lib/appQueries";
import { StatusBadge } from "@/components/ui/StatusBadge";
import appLogo from "@/assets/app-logo.png";

export interface StatusBarProps {
  appVersion?: string | null;
  className?: string;
  style?: React.CSSProperties;
}

export const StatusBar: React.FC<StatusBarProps> = ({
  appVersion,
  className = "",
  style,
}) => {
  const { t } = useTranslation();
  const target = usePagePreferencesStore((s) => s.workspaceTarget);

  const proxyQuery = useQuery(proxyStatusOptions(target));
  const providersQuery = useQuery(providerListOptions(target));

  const proxyRunning = proxyQuery.data?.running ?? false;
  const proxyPort = proxyQuery.data?.port ?? (target === "codex" ? 15822 : target === "opencode" ? 15824 : 15821);

  const currentProvider = providersQuery.data?.providers.find((p) => p.isCurrent);
  const providerName = currentProvider ? currentProvider.name : t("workbench.noCurrentProvider", { defaultValue: "无" });

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
        ...style,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: "var(--space-3)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: "var(--space-2)" }}>
          <img src={appLogo} alt="AI-Switcher" style={{ width: 14, height: 14 }} />
          <span style={{ fontWeight: "var(--font-weight-semibold)", color: "var(--color-text-primary)" }}>
            AI-Switcher
          </span>
        </div>

        <span style={{ color: "var(--color-border-strong)" }}>|</span>

        <StatusBadge
          status={proxyRunning ? "running" : "stopped"}
          label={proxyRunning ? `Proxy :${proxyPort}` : "Proxy Stopped"}
        />

        <span style={{ color: "var(--color-border-strong)" }}>|</span>

        <span>
          {t("workbench.currentProviderLabel", { defaultValue: "当前供应商" })}:{" "}
          <strong style={{ color: "var(--color-text-primary)" }}>{providerName}</strong>
        </span>
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: "var(--space-3)" }}>
        {appVersion && <span>v{appVersion}</span>}
      </div>
    </footer>
  );
};
