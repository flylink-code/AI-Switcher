import React from "react";
import ApiOutlined from "@ant-design/icons/es/icons/ApiOutlined";
import { useThemeStore } from "@/stores/themeStore";

export const AppBrand: React.FC = () => {
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const isDark = resolvedTheme === "dark";

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: "10px",
        paddingLeft: "8px",
        userSelect: "none",
      }}
    >
      <div
        style={{
          width: "28px",
          height: "28px",
          borderRadius: "8px",
          backgroundColor: "#3B82F6",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "#FFFFFF",
          boxShadow: "0 2px 4px rgba(59, 130, 246, 0.25)",
          fontSize: 16,
        }}
      >
        <ApiOutlined />
      </div>
      <span
        style={{
          fontSize: "15px",
          fontWeight: 700,
          letterSpacing: "-0.01em",
          color: isDark ? "#F2F4F7" : "#111827",
        }}
      >
        AI-Switcher
      </span>
      <span
        style={{
          fontSize: "10px",
          fontWeight: 600,
          padding: "1px 6px",
          borderRadius: "4px",
          backgroundColor: isDark ? "#222A35" : "#E8ECF1",
          color: isDark ? "#9CA3AF" : "#6B7280",
          textTransform: "uppercase",
        }}
      >
        V2
      </span>
    </div>
  );
};
