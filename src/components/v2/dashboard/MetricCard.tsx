import React from "react";
import { useThemeStore } from "@/stores/themeStore";

export interface MetricCardProps {
  title: string;
  value: string | number;
  subtitle?: string;
  icon?: React.ReactNode;
  statusColor?: string;
  trend?: string;
}

export const MetricCard: React.FC<MetricCardProps> = ({
  title,
  value,
  subtitle,
  icon,
  statusColor = "#3B82F6",
  trend,
}) => {
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const isDark = resolvedTheme === "dark";

  return (
    <div
      style={{
        borderRadius: "14px",
        padding: "16px 20px",
        backgroundColor: isDark ? "#1A212B" : "#FFFFFF",
        border: `1px solid ${isDark ? "#242D38" : "#E8ECF1"}`,
        display: "flex",
        flexDirection: "column",
        justifyContent: "space-between",
        minHeight: "115px",
        boxShadow: isDark ? "none" : "0 1px 3px rgba(0,0,0,0.03)",
        transition: "transform 0.15s ease, box-shadow 0.15s ease",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <span style={{ fontSize: "13px", fontWeight: 500, color: isDark ? "#9CA3AF" : "#6B7280" }}>
          {title}
        </span>
        {icon && (
          <div
            style={{
              width: "28px",
              height: "28px",
              borderRadius: "8px",
              backgroundColor: isDark ? "rgba(255,255,255,0.05)" : "#F4F6F8",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: statusColor,
            }}
          >
            {icon}
          </div>
        )}
      </div>

      <div style={{ marginTop: "8px", display: "flex", alignItems: "baseline", gap: "8px" }}>
        <span style={{ fontSize: "26px", fontWeight: 700, letterSpacing: "-0.02em", color: isDark ? "#F2F4F7" : "#111827" }}>
          {value}
        </span>
        {trend && (
          <span style={{ fontSize: "12px", fontWeight: 600, color: trend.startsWith("↑") ? "#22C55E" : "#EF4444" }}>
            {trend}
          </span>
        )}
      </div>

      {subtitle && (
        <div style={{ marginTop: "4px", fontSize: "12px", color: isDark ? "#6B7280" : "#9CA3AF" }}>
          {subtitle}
        </div>
      )}
    </div>
  );
};
