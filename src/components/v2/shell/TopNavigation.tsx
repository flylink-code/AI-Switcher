import React from "react";
import { useTranslation } from "react-i18next";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import CloudServerOutlined from "@ant-design/icons/es/icons/CloudServerOutlined";
import DashboardOutlined from "@ant-design/icons/es/icons/DashboardOutlined";
import SettingOutlined from "@ant-design/icons/es/icons/SettingOutlined";
import type { PageKey } from "@/lib/pageRegistry";
import { useThemeStore } from "@/stores/themeStore";

export type V2MainTab = "workbench" | "service" | "usage" | "settings";

export interface TopNavigationProps {
  activeKey: PageKey;
  onNavigate: (key: PageKey) => void;
}

export const TopNavigation: React.FC<TopNavigationProps> = ({ activeKey, onNavigate }) => {
  const { t } = useTranslation();
  const resolvedTheme = useThemeStore((s) => s.resolved);

  const getActiveTab = (key: PageKey): V2MainTab => {
    if (key === "workbench") return "workbench";
    if (key === "usage") return "usage";
    if (
      key === "settings" ||
      key === "sessions" ||
      key === "environment" ||
      key === "localization" ||
      key === "about"
    ) {
      return "settings";
    }
    // providers / proxy / antigravity / workspace / mcp / plugins …
    return "service";
  };

  const currentTab = getActiveTab(activeKey);

  const navItems: Array<{ key: V2MainTab; label: string; icon: React.ReactNode; targetPage: PageKey }> = [
    {
      key: "workbench",
      label: t("navigation.dashboard", { defaultValue: "仪表盘" }),
      icon: <DashboardOutlined />,
      targetPage: "workbench",
    },
    {
      key: "service",
      label: t("navigation.service", { defaultValue: "服务" }),
      icon: <CloudServerOutlined />,
      targetPage: "providers",
    },
    {
      key: "usage",
      label: t("navigation.usage", { defaultValue: "用量" }),
      icon: <BarChartOutlined />,
      targetPage: "usage",
    },
    {
      key: "settings",
      label: t("navigation.settings", { defaultValue: "设置" }),
      icon: <SettingOutlined />,
      targetPage: "settings",
    },
  ];

  const isDark = resolvedTheme === "dark";

  return (
    <div
      style={{
        display: "inline-flex",
        alignItems: "center",
        padding: "3px",
        borderRadius: "999px",
        backgroundColor: isDark ? "#171C24" : "#F3F4F6",
        border: `1px solid ${isDark ? "#222A35" : "#E5E7EB"}`,
        gap: "2px",
      }}
    >
      {navItems.map((item) => {
        const isActive = currentTab === item.key;
        return (
          <button
            key={item.key}
            type="button"
            onClick={() => onNavigate(item.targetPage)}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: "6px",
              padding: "6px 16px",
              borderRadius: "999px",
              fontSize: "13px",
              fontWeight: isActive ? 600 : 500,
              border: "none",
              cursor: "pointer",
              transition: "all 0.15s ease-in-out",
              backgroundColor: isActive ? (isDark ? "#F8FAFC" : "#111827") : "transparent",
              color: isActive
                ? isDark
                  ? "#111827"
                  : "#FFFFFF"
                : isDark
                  ? "#9CA3AF"
                  : "#5B6474",
              boxShadow: isActive ? "0 1px 3px rgba(0,0,0,0.1)" : "none",
            }}
          >
            {item.icon}
            <span>{item.label}</span>
          </button>
        );
      })}
    </div>
  );
};
