import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import ControlOutlined from "@ant-design/icons/es/icons/ControlOutlined";
import InfoCircleOutlined from "@ant-design/icons/es/icons/InfoCircleOutlined";
import { useThemeStore } from "@/stores/themeStore";
import type { PageKey } from "@/lib/pageRegistry";
import SettingsPage from "@/pages/SettingsPage";
import AboutPage from "@/pages/AboutPage";

export type SettingsSubTab = "general" | "about";

export interface SettingsPageV2Props {
  initialTab?: SettingsSubTab;
  onNavigate?: (key: PageKey) => void;
}

export const SettingsPageV2: React.FC<SettingsPageV2Props> = ({
  initialTab = "general",
  onNavigate,
}) => {
  const { t } = useTranslation();
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const isDark = resolvedTheme === "dark";

  const [activeTab, setActiveTab] = useState<SettingsSubTab>(initialTab);

  useEffect(() => {
    setActiveTab(initialTab);
  }, [initialTab]);

  const subNavItems: Array<{ key: SettingsSubTab; label: string; icon: React.ReactNode }> = [
    {
      key: "general",
      label: t("navigation.settings", { defaultValue: "设置" }),
      icon: <ControlOutlined />,
    },
    {
      key: "about",
      label: t("settings.sectionAbout", { defaultValue: "关于" }),
      icon: <InfoCircleOutlined />,
    },
  ];

  const handleTabChange = (tab: SettingsSubTab) => {
    setActiveTab(tab);
    onNavigate?.(tab === "about" ? "about" : "settings");
  };

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "16px",
        maxWidth: "980px",
        margin: "0 auto",
        width: "100%",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          paddingBottom: "8px",
          borderBottom: `1px solid ${isDark ? "#242D38" : "#E8ECF1"}`,
        }}
      >
        <div
          style={{
            display: "inline-flex",
            alignItems: "center",
            padding: "3px",
            borderRadius: "8px",
            backgroundColor: isDark ? "#151B23" : "#F4F6F8",
            gap: "2px",
          }}
        >
          {subNavItems.map((item) => {
            const isActive = activeTab === item.key;
            return (
              <button
                key={item.key}
                type="button"
                onClick={() => handleTabChange(item.key)}
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: "6px",
                  padding: "5px 14px",
                  borderRadius: "6px",
                  fontSize: "12px",
                  fontWeight: isActive ? 600 : 500,
                  border: "none",
                  cursor: "pointer",
                  transition: "all 0.15s ease-in-out",
                  backgroundColor: isActive ? (isDark ? "#1A212B" : "#FFFFFF") : "transparent",
                  color: isActive
                    ? isDark
                      ? "#F2F4F7"
                      : "#111827"
                    : isDark
                      ? "#9CA3AF"
                      : "#6B7280",
                  boxShadow: isActive ? (isDark ? "none" : "0 1px 2px rgba(0,0,0,0.05)") : "none",
                }}
              >
                {item.icon}
                <span>{item.label}</span>
              </button>
            );
          })}
        </div>
      </div>

      <div style={{ minHeight: "450px" }}>
        {activeTab === "about" ? <AboutPage /> : <SettingsPage />}
      </div>
    </div>
  );
};
