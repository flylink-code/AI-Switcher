import React from "react";
import { useTranslation } from "react-i18next";
import ArrowRightOutlined from "@ant-design/icons/es/icons/ArrowRightOutlined";
import { useThemeStore } from "@/stores/themeStore";
import type { PageKey } from "@/lib/pageRegistry";

export interface CurrentRuntimeCardProps {
  onNavigate: (key: PageKey) => void;
  activeProviderName?: string;
  activeModelName?: string;
  proxyPort?: number;
  isRunning?: boolean;
}

export const CurrentRuntimeCard: React.FC<CurrentRuntimeCardProps> = ({
  onNavigate,
  activeProviderName = "—",
  activeModelName = "—",
  proxyPort = 0,
  isRunning = false,
}) => {
  const { t } = useTranslation();
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const isDark = resolvedTheme === "dark";

  return (
    <div
      style={{
        borderRadius: "14px",
        padding: "20px",
        backgroundColor: isDark ? "#1A212B" : "#FFFFFF",
        border: `1px solid ${isDark ? "#242D38" : "#E8ECF1"}`,
        display: "flex",
        flexDirection: "column",
        justifyContent: "space-between",
        gap: "16px",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
          <div
            style={{
              width: "8px",
              height: "8px",
              borderRadius: "50%",
              backgroundColor: isRunning ? "#22C55E" : "#9CA3AF",
              boxShadow: isRunning ? "0 0 8px rgba(34, 197, 94, 0.4)" : "none",
            }}
          />
          <span style={{ fontSize: "15px", fontWeight: 600, color: isDark ? "#F2F4F7" : "#111827" }}>
            {t("workbench.currentRuntimeTitle", { defaultValue: "当前运行" })}
          </span>
        </div>
        <span
          style={{
            fontSize: "12px",
            padding: "2px 8px",
            borderRadius: "999px",
            backgroundColor: isRunning
              ? isDark
                ? "rgba(34, 197, 94, 0.15)"
                : "#DCFCE7"
              : isDark
                ? "rgba(156, 163, 175, 0.15)"
                : "#F3F4F6",
            color: isRunning ? "#166534" : isDark ? "#9CA3AF" : "#6B7280",
            fontWeight: 600,
          }}
        >
          {isRunning
            ? t("proxy.running", { defaultValue: "Running" })
            : t("proxy.stopped", { defaultValue: "Stopped" })}
        </span>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "repeat(2, 1fr)", gap: "12px" }}>
        <div
          style={{
            padding: "10px 12px",
            borderRadius: "10px",
            backgroundColor: isDark ? "#151B23" : "#F4F6F8",
          }}
        >
          <div style={{ fontSize: "11px", color: isDark ? "#9CA3AF" : "#6B7280" }}>Provider</div>
          <div
            style={{
              fontSize: "14px",
              fontWeight: 600,
              color: isDark ? "#F2F4F7" : "#111827",
              marginTop: "2px",
            }}
          >
            {activeProviderName}
          </div>
        </div>

        <div
          style={{
            padding: "10px 12px",
            borderRadius: "10px",
            backgroundColor: isDark ? "#151B23" : "#F4F6F8",
          }}
        >
          <div style={{ fontSize: "11px", color: isDark ? "#9CA3AF" : "#6B7280" }}>Model</div>
          <div
            style={{
              fontSize: "14px",
              fontWeight: 600,
              color: isDark ? "#F2F4F7" : "#111827",
              marginTop: "2px",
            }}
          >
            {activeModelName}
          </div>
        </div>
      </div>

      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          fontSize: "12px",
          color: isDark ? "#9CA3AF" : "#6B7280",
          paddingTop: "8px",
          borderTop: `1px solid ${isDark ? "#242D38" : "#F3F4F6"}`,
        }}
      >
        <span>
          Local Proxy:{" "}
          <code style={{ color: "#3B82F6" }}>
            {proxyPort > 0 ? `127.0.0.1:${proxyPort}` : "—"}
          </code>
        </span>
        <button
          type="button"
          onClick={() => onNavigate("providers")}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: "4px",
            border: "none",
            background: "transparent",
            color: "#3B82F6",
            fontSize: "12px",
            fontWeight: 600,
            cursor: "pointer",
          }}
        >
          <span>{t("workbench.viewProvidersLink", { defaultValue: "管理供应商" })}</span>
          <ArrowRightOutlined />
        </button>
      </div>
    </div>
  );
};
