import React from "react";
import { useThemeStore } from "@/stores/themeStore";

export interface SettingsGroupProps {
  title?: string;
  children: React.ReactNode;
}

export const SettingsGroup: React.FC<SettingsGroupProps> = ({ title, children }) => {
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const isDark = resolvedTheme === "dark";

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
      {title && (
        <span style={{ fontSize: "13px", fontWeight: 600, color: isDark ? "#9CA3AF" : "#6B7280", paddingLeft: "4px" }}>
          {title}
        </span>
      )}
      <div
        style={{
          borderRadius: "12px",
          backgroundColor: isDark ? "#1A212B" : "#FFFFFF",
          border: `1px solid ${isDark ? "#242D38" : "#E8ECF1"}`,
          overflow: "hidden",
        }}
      >
        {children}
      </div>
    </div>
  );
};
