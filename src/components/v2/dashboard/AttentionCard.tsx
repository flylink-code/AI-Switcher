import React from "react";
import { useTranslation } from "react-i18next";
import AlertOutlined from "@ant-design/icons/es/icons/AlertOutlined";
import CheckCircleOutlined from "@ant-design/icons/es/icons/CheckCircleOutlined";
import WarningOutlined from "@ant-design/icons/es/icons/WarningOutlined";
import { useThemeStore } from "@/stores/themeStore";
import type { PageKey } from "@/lib/pageRegistry";

export interface AttentionIssue {
  id: string;
  message: string;
  level: "info" | "warning" | "error";
  page?: PageKey;
  action?: string;
}

export interface AttentionCardProps {
  issues?: AttentionIssue[];
  providerCount?: number;
  onNavigate?: (key: PageKey) => void;
}

export const AttentionCard: React.FC<AttentionCardProps> = ({
  issues = [],
  providerCount = 0,
  onNavigate,
}) => {
  const { t } = useTranslation();
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const isDark = resolvedTheme === "dark";

  const hasIssues = issues.length > 0;

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
      }}
    >
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <span style={{ fontSize: "15px", fontWeight: 600, color: isDark ? "#F2F4F7" : "#111827" }}>
          {t("workbench.attentionTitle", { defaultValue: "需要关注" })}
        </span>
        <span
          style={{
            fontSize: "11px",
            padding: "2px 8px",
            borderRadius: "999px",
            backgroundColor: !hasIssues
              ? isDark
                ? "rgba(34, 197, 94, 0.12)"
                : "#F0FDF4"
              : isDark
                ? "rgba(234, 179, 8, 0.12)"
                : "#FEFCE8",
            color: !hasIssues ? "#22C55E" : "#D97706",
            fontWeight: 600,
          }}
        >
          {!hasIssues
            ? t("workbench.stripHealthy", { defaultValue: "System Healthy" })
            : t("workbench.attentionCount", {
                count: issues.length,
                defaultValue: "{{count}} Alerts",
              })}
        </span>
      </div>

      <div style={{ margin: "16px 0", display: "flex", flexDirection: "column", gap: "10px" }}>
        {!hasIssues ? (
          <div style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: "8px",
                fontSize: "13px",
                color: isDark ? "#D1D5DB" : "#374151",
              }}
            >
              <CheckCircleOutlined style={{ color: "#22C55E" }} />
              <span>
                {t("workbench.attentionAllClear", {
                  count: providerCount,
                  defaultValue: "当前代理与 {{count}} 个供应商运行正常",
                })}
              </span>
            </div>
          </div>
        ) : (
          issues.map((issue) => (
            <div
              key={issue.id}
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: "8px",
                fontSize: "13px",
                color: issue.level === "error" ? "#EF4444" : "#F59E0B",
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                {issue.level === "error" ? <AlertOutlined /> : <WarningOutlined />}
                <span>{issue.message}</span>
              </div>
              {issue.page && issue.action && onNavigate ? (
                <button
                  type="button"
                  onClick={() => onNavigate(issue.page!)}
                  style={{
                    border: "none",
                    background: "transparent",
                    color: "#3B82F6",
                    fontSize: "12px",
                    fontWeight: 600,
                    cursor: "pointer",
                    flexShrink: 0,
                  }}
                >
                  {issue.action}
                </button>
              ) : null}
            </div>
          ))
        )}
      </div>

      <div
        style={{
          fontSize: "12px",
          color: isDark ? "#6B7280" : "#9CA3AF",
          paddingTop: "8px",
          borderTop: `1px solid ${isDark ? "#242D38" : "#F3F4F6"}`,
        }}
      >
        {t("workbench.lastCheckedAt", {
          time: new Date().toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" }),
          defaultValue: "最后检查于 {{time}}",
        })}
      </div>
    </div>
  );
};
