import React from "react";
import { useThemeStore } from "@/stores/themeStore";

export interface SettingsRowProps {
  label: string;
  description?: string;
  control: React.ReactNode;
}

export const SettingsRow: React.FC<SettingsRowProps> = ({ label, description, control }) => {
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const isDark = resolvedTheme === "dark";

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        padding: "12px 16px",
        borderBottom: `1px solid ${isDark ? "#222A35" : "#F3F4F6"}`,
      }}
    >
      <div style={{ paddingRight: "16px" }}>
        <div style={{ fontSize: "13px", fontWeight: 500, color: isDark ? "#F2F4F7" : "#111827" }}>
          {label}
        </div>
        {description && (
          <div style={{ fontSize: "12px", color: isDark ? "#9CA3AF" : "#6B7280", marginTop: "2px" }}>
            {description}
          </div>
        )}
      </div>

      <div style={{ flexShrink: 0 }}>{control}</div>
    </div>
  );
};
